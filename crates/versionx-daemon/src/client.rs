//! Client-side of the daemon protocol.
//!
//! Designed for fire-and-forget use from the CLI:
//!   - [`Client::connect`] establishes the socket.
//!   - [`Client::call`] sends one request and awaits the matching response.
//!   - [`Client::subscribe`] opens a streaming subscription.
//!
//! The client multiplexes a single connection over many in-flight calls
//! by keying on the JSON-RPC `id`. Responses are matched to pending
//! oneshot channels via a small request table.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use parking_lot::Mutex;
use serde::Deserialize;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tracing::debug;

use crate::codec::JsonFrameCodec;
use crate::paths::DaemonPaths;
use crate::protocol::{Message, Notification, Request, ResponsePayload, methods};
use crate::transport::{DuplexStream, connect, framed};

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("daemon is not running (no socket at {socket})")]
    NotRunning { socket: String },
    #[error("daemon call '{method}' failed: {code} {message}")]
    Rpc { method: String, code: i32, message: String },
    #[error("transport error: {0}")]
    Transport(#[from] std::io::Error),
    #[error("codec error: {0}")]
    Codec(String),
    #[error("daemon disconnected before responding")]
    Disconnected,
    #[error("serde error: {0}")]
    Serde(#[from] serde_json::Error),
}

pub type ClientResult<T> = Result<T, ClientError>;

#[derive(Debug)]
pub struct Client {
    tx_out: mpsc::Sender<OutboundMessage>,
    pending: Arc<Mutex<HashMap<String, oneshot::Sender<ResponsePayload>>>>,
    _writer_handle: JoinHandle<()>,
    _reader_handle: JoinHandle<()>,
    notification_tx: broadcast::Sender<Notification>,
}

enum OutboundMessage {
    Request(Request),
}

use tokio::sync::broadcast;

impl Client {
    /// Connect to the daemon at `paths`. Returns [`ClientError::NotRunning`]
    /// if no daemon is listening.
    pub async fn connect(paths: &DaemonPaths) -> ClientResult<Self> {
        let stream = match connect(paths).await {
            Ok(s) => s,
            Err(e)
                if e.kind() == std::io::ErrorKind::NotFound
                    || e.kind() == std::io::ErrorKind::ConnectionRefused =>
            {
                return Err(ClientError::NotRunning { socket: paths.socket.to_string() });
            }
            Err(e) => return Err(ClientError::Transport(e)),
        };
        Ok(Self::from_framed(framed(stream)))
    }

    fn from_framed(stream: tokio_util::codec::Framed<DuplexStream, JsonFrameCodec>) -> Self {
        let (tx_out, mut rx_out) = mpsc::channel::<OutboundMessage>(64);
        let pending: Arc<Mutex<HashMap<String, oneshot::Sender<ResponsePayload>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let (notification_tx, _) = broadcast::channel::<Notification>(256);

        let (mut sink, mut source) = stream.split();

        let writer_handle = tokio::spawn(async move {
            while let Some(msg) = rx_out.recv().await {
                let wire = match msg {
                    OutboundMessage::Request(r) => Message::Request(r),
                };
                if sink.send(wire).await.is_err() {
                    break;
                }
            }
            // Drop the sink explicitly.
            let _ = sink.close().await;
        });

        let pending_reader = pending.clone();
        let notif_reader = notification_tx.clone();
        let reader_handle = tokio::spawn(async move {
            while let Some(frame) = source.next().await {
                match frame {
                    Ok(Message::Response(r)) => {
                        let waker = pending_reader.lock().remove(&r.id);
                        if let Some(tx) = waker {
                            let _ = tx.send(r.payload);
                        } else {
                            debug!(id = %r.id, "response for unknown request");
                        }
                    }
                    Ok(Message::Notification(n)) => {
                        let _ = notif_reader.send(n);
                    }
                    Ok(Message::Request(_)) => {
                        // Server-initiated requests aren't a thing in 0.3.
                        debug!("ignoring server-initiated request");
                    }
                    Err(e) => {
                        debug!("reader error: {e}");
                        break;
                    }
                }
            }
            // Wake everyone still waiting so they see Disconnected.
            let mut pending = pending_reader.lock();
            pending.drain(); // dropping senders closes the oneshot
        });

        Self {
            tx_out,
            pending,
            _writer_handle: writer_handle,
            _reader_handle: reader_handle,
            notification_tx,
        }
    }

    /// One-shot RPC call — sends the request and awaits the response.
    pub async fn call<P: serde::Serialize, R: for<'de> Deserialize<'de>>(
        &self,
        method: &str,
        params: P,
    ) -> ClientResult<R> {
        let req = Request::new(method, serde_json::to_value(params)?);
        let (tx, rx) = oneshot::channel::<ResponsePayload>();
        self.pending.lock().insert(req.id.clone(), tx);

        self.tx_out
            .send(OutboundMessage::Request(req))
            .await
            .map_err(|_| ClientError::Disconnected)?;

        let payload = rx.await.map_err(|_| ClientError::Disconnected)?;
        match payload {
            ResponsePayload::Result { result } => Ok(serde_json::from_value(result)?),
            ResponsePayload::Error { error } => Err(ClientError::Rpc {
                method: method.into(),
                code: error.code,
                message: error.message,
            }),
        }
    }

    /// Subscribe to server notifications on `channels`. Returns a
    /// broadcast receiver that yields every matching notification.
    pub async fn subscribe(
        &self,
        channels: &[&str],
    ) -> ClientResult<broadcast::Receiver<Notification>> {
        let _: serde_json::Value = self
            .call(
                methods::SUBSCRIBE,
                serde_json::json!({
                    "channels": channels.iter().map(|c| (*c).to_string()).collect::<Vec<_>>(),
                }),
            )
            .await?;
        Ok(self.notification_tx.subscribe())
    }

    /// Send a graceful shutdown request. The daemon will respond before
    /// draining connections.
    pub async fn shutdown(&self) -> ClientResult<()> {
        let _: serde_json::Value = self.call(methods::SHUTDOWN, serde_json::json!({})).await?;
        Ok(())
    }

    /// Call `ping`. Useful for liveness checks.
    pub async fn ping(&self) -> ClientResult<()> {
        let _: serde_json::Value = self.call(methods::PING, serde_json::json!({})).await?;
        Ok(())
    }

    /// Fetch [`ServerInfo`] (uptime, pid, version).
    pub async fn server_info(&self) -> ClientResult<ServerInfo> {
        self.call(methods::SERVER_INFO, serde_json::json!({})).await
    }

    pub fn notification_tx(&self) -> broadcast::Sender<Notification> {
        self.notification_tx.clone()
    }
}

#[derive(Debug, Deserialize)]
pub struct ServerInfo {
    pub version: String,
    pub pid: u32,
    pub uptime_seconds: u64,
}

/// Attempt a quick ping to see whether a daemon is up.
pub async fn is_running(paths: &DaemonPaths) -> bool {
    let fut = async {
        let client = Client::connect(paths).await?;
        client.ping().await?;
        Ok::<_, ClientError>(())
    };
    matches!(tokio::time::timeout(Duration::from_millis(500), fut).await, Ok(Ok(())))
}
