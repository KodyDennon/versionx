//! The server half of `versiond`.
//!
//! Responsibilities:
//!   - Accept connections on the platform transport.
//!   - Per connection: read frames, dispatch methods, write responses /
//!     streamed notifications.
//!   - Own the workspace discovery + status cache and invalidate it on
//!     filesystem events from [`crate::watcher`].
//!   - Enforce an idle timeout (graceful exit when nobody's talked to us
//!     for `idle_timeout`).
//!
//! ### Concurrency model
//!
//! One tokio task per connection. All connections share a `Arc<State>` so
//! cache reads are fast + lock-free after a warm-up. Workspace discovery /
//! status / bump computations happen inside the per-connection task; they
//! do not block the accept loop.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use camino::Utf8PathBuf;
use futures::{SinkExt, StreamExt};
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use tokio::sync::{Notify, broadcast, mpsc};
use tokio::task::JoinHandle;
use tokio_util::codec::Framed;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::codec::JsonFrameCodec;
use crate::paths::DaemonPaths;
use crate::pidfile::PidFile;
use crate::protocol::{
    ErrorObject, Message, Notification, Request, Response, methods, notifications,
};
use crate::transport::{DuplexStream, Listener, framed};
use crate::watcher::{self, CacheInvalidator, WatcherHandle};

/// Runtime configuration for the daemon.
#[derive(Clone, Debug)]
pub struct ServerConfig {
    pub paths: DaemonPaths,
    /// Shut down after this much wall-clock time without any request.
    /// `None` disables the timeout (useful for systemd/launchd-managed
    /// instances).
    pub idle_timeout: Option<Duration>,
    /// Broadcast channel capacity for notifications. Slow subscribers
    /// lag-drop beyond this.
    pub notification_capacity: usize,
}

impl ServerConfig {
    #[must_use]
    pub const fn new(paths: DaemonPaths) -> Self {
        Self { paths, idle_timeout: Some(Duration::from_mins(30)), notification_capacity: 256 }
    }
}

/// Shared state across connections.
struct State {
    /// Cached workspace snapshots, behind a type that the fs watcher
    /// can invalidate without needing access to the full `State`.
    snapshots: Arc<SnapshotCache>,
    /// Notification fanout. Each subscriber holds a [`broadcast::Receiver`]
    /// and filters by channel name on their side (kept simple — filtering
    /// on send side means multiple senders per channel type).
    notify_tx: broadcast::Sender<Notification>,
    /// Resets to `now` on every incoming request. The idle watchdog reads
    /// this to decide whether to exit.
    last_activity: Mutex<Instant>,
    /// Flag tripped by `server.shutdown` RPC or SIGINT; acceptor loop
    /// watches it. Arc so per-connection tasks can clone + wait on it.
    shutdown: Arc<Notify>,
    /// Server start time (for `server.info`).
    started_at: Instant,
    /// The fs watcher. We keep it on `State` so `handle_workspace` can
    /// register newly-discovered roots dynamically.
    watcher: WatcherHandle,
}

/// Workspace snapshot cache shared between the request path (which
/// reads + populates) and the fs watcher (which evicts).
///
/// We keep this as its own type so the watcher can invalidate without
/// pulling in the rest of `State`.
struct SnapshotCache {
    inner: RwLock<HashMap<Utf8PathBuf, CachedSnapshot>>,
}

impl SnapshotCache {
    fn new() -> Self {
        Self { inner: RwLock::new(HashMap::new()) }
    }
}

impl CacheInvalidator for SnapshotCache {
    fn invalidate(&self, changed_paths: &[std::path::PathBuf]) {
        if changed_paths.is_empty() {
            return;
        }
        let mut cache = self.inner.write();
        // Drop any cached entry whose root is an ancestor of (or equal
        // to) any changed path. We err on the side of evicting too
        // much — a recompute is cheaper than a stale cache hit.
        cache.retain(|root, _| {
            let root_std = root.as_std_path();
            !changed_paths.iter().any(|p| p.starts_with(root_std))
        });
    }
}

struct CachedSnapshot {
    /// The pre-serialized result of the last `workspace.list` call. Small
    /// (< 200KB) for realistic repos — keeping it cached avoids repeating
    /// manifest parsing on every request.
    workspace_list: serde_json::Value,
    workspace_status: serde_json::Value,
    workspace_graph: serde_json::Value,
}

/// High-level entry point. Takes over the current task until shutdown.
///
/// # Errors
/// Propagates IO/bind errors at startup. Once the accept loop is running,
/// per-connection errors are logged and swallowed so one bad client can't
/// take down the daemon.
pub async fn run(config: ServerConfig) -> anyhow::Result<()> {
    // 1. Acquire the pid + lock file first — fail fast if another daemon
    //    is already alive.
    let _pidfile = PidFile::acquire(&config.paths.lock_file, &config.paths.pid_file)?;

    // 2. Bind the listener.
    let listener = Listener::bind(&config.paths).await?;
    info!(socket = %config.paths.socket, "versiond listening");

    // 3. Set up shared state + fanout channel.
    let (notify_tx, _) = broadcast::channel(config.notification_capacity);
    let snapshots = Arc::new(SnapshotCache::new());

    // 4. Start the fs watcher *before* building State so we can hand
    //    its handle to State for dynamic root registration. The watcher
    //    holds an Arc to `snapshots` so it can evict on file changes.
    let watcher_handle =
        watcher::spawn(notify_tx.clone(), snapshots.clone() as Arc<dyn CacheInvalidator>)?;

    let state = Arc::new(State {
        snapshots,
        notify_tx,
        last_activity: Mutex::new(Instant::now()),
        shutdown: Arc::new(Notify::new()),
        started_at: Instant::now(),
        watcher: watcher_handle,
    });

    // 5. Start the idle watchdog.
    let watchdog_handle = spawn_idle_watchdog(state.clone(), config.idle_timeout);

    // 6. Accept loop — exits when shutdown is signaled.
    let shutdown_notify = state.shutdown.clone();
    let accept_shutdown = async {
        shutdown_notify.notified().await;
    };
    tokio::pin!(accept_shutdown);

    loop {
        tokio::select! {
            () = &mut accept_shutdown => {
                info!("shutdown signal received, draining connections");
                break;
            }
            res = listener.accept() => {
                match res {
                    Ok(stream) => {
                        let framed = framed(stream);
                        let state = state.clone();
                        tokio::spawn(async move {
                            if let Err(e) = handle_connection(framed, state).await {
                                warn!("connection closed with error: {e}");
                            }
                        });
                    }
                    Err(err) => {
                        error!("accept failed: {err}");
                        // Keep accepting — a single accept failure shouldn't
                        // kill the server.
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                }
            }
        }
    }

    // Let any in-flight clients see the shutdown notification.
    let _ = state
        .notify_tx
        .send(Notification::new(notifications::SHUTTING_DOWN, serde_json::json!({})));

    drop(listener);
    watchdog_handle.abort();
    // The watcher is owned by `state`; dropping the last Arc to State
    // (when this function returns + per-connection tasks finish) shuts
    // it down.
    info!("versiond shutdown complete");
    Ok(())
}

async fn handle_connection(
    mut stream: Framed<DuplexStream, JsonFrameCodec>,
    state: Arc<State>,
) -> anyhow::Result<()> {
    let conn_id = Uuid::now_v7();
    debug!(%conn_id, "client connected");

    // Per-connection subscription state.
    let subscriptions = Arc::new(Mutex::new(Subscriptions::default()));

    // Fanout task — receives notifications from the global broadcast
    // channel and forwards matching ones to this specific client.
    let mut notify_rx = state.notify_tx.subscribe();
    let (to_client_tx, mut to_client_rx) = mpsc::channel::<Message>(64);
    let fanout_subs = subscriptions.clone();
    let fanout_tx = to_client_tx.clone();
    let fanout = tokio::spawn(async move {
        while let Ok(n) = notify_rx.recv().await {
            let wanted = fanout_subs.lock().matches(&n.method);
            if wanted {
                let _ = fanout_tx.send(Message::Notification(n)).await;
            }
        }
    });

    loop {
        tokio::select! {
            // Outbound: messages produced by the dispatch branch or the fanout task.
            Some(out) = to_client_rx.recv() => {
                if let Err(e) = stream.send(out).await {
                    debug!(%conn_id, "client write failed: {e}");
                    break;
                }
            }
            // Inbound: next frame from the client.
            frame = stream.next() => {
                let Some(frame) = frame else { break };
                match frame {
                    Ok(Message::Request(req)) => {
                        *state.last_activity.lock() = Instant::now();
                        let resp = dispatch_request(&state, subscriptions.clone(), req).await;
                        if let Err(e) = stream.send(Message::Response(resp)).await {
                            debug!(%conn_id, "response write failed: {e}");
                            break;
                        }
                    }
                    Ok(Message::Notification(n)) => {
                        // Clients don't really send notifications today,
                        // but don't break if they do — just drop them.
                        debug!(%conn_id, method = %n.method, "ignoring client notification");
                    }
                    Ok(Message::Response(_)) => {
                        // Ditto — no outstanding requests from us (yet).
                        debug!(%conn_id, "ignoring unexpected response");
                    }
                    Err(e) => {
                        warn!(%conn_id, "codec error: {e}; closing connection");
                        let _ = stream
                            .send(Message::Response(Response::error(
                                "malformed",
                                ErrorObject::new(ErrorObject::PARSE_ERROR, e.to_string()),
                            )))
                            .await;
                        break;
                    }
                }
            }
        }
    }

    fanout.abort();
    debug!(%conn_id, "client disconnected");
    Ok(())
}

// -------- Dispatch -------------------------------------------------------

async fn dispatch_request(
    state: &Arc<State>,
    subscriptions: Arc<Mutex<Subscriptions>>,
    req: Request,
) -> Response {
    let id = req.id.clone();
    match handle_method(state, subscriptions, req).await {
        Ok(v) => Response::success(id, v),
        Err(e) => Response::error(id, e),
    }
}

async fn handle_method(
    state: &Arc<State>,
    subscriptions: Arc<Mutex<Subscriptions>>,
    req: Request,
) -> Result<serde_json::Value, ErrorObject> {
    match req.method.as_str() {
        methods::PING => Ok(serde_json::json!({"pong": true})),
        methods::SERVER_INFO => Ok(serde_json::to_value(server_info(state)).unwrap_or_default()),
        methods::SHUTDOWN => {
            info!("shutdown requested via RPC");
            state.shutdown.notify_waiters();
            Ok(serde_json::json!({"ok": true}))
        }
        methods::WORKSPACE_LIST => handle_workspace(state, req.params, WorkspaceOp::List),
        methods::WORKSPACE_STATUS => handle_workspace(state, req.params, WorkspaceOp::Status),
        methods::WORKSPACE_GRAPH => handle_workspace(state, req.params, WorkspaceOp::Graph),
        methods::BUMP_PROPOSE => handle_bump(state, req.params),
        methods::SUBSCRIBE => {
            let params: SubscribeParams = parse_params(req.params)?;
            subscriptions.lock().add(params.channels);
            Ok(serde_json::json!({"subscribed": true}))
        }
        methods::UNSUBSCRIBE => {
            let params: SubscribeParams = parse_params(req.params)?;
            subscriptions.lock().remove(&params.channels);
            Ok(serde_json::json!({"unsubscribed": true}))
        }
        other => Err(ErrorObject::new(
            ErrorObject::METHOD_NOT_FOUND,
            format!("method not found: {other}"),
        )),
    }
}

/// Trampoline so the dispatch match stays tidy. The `?` operator falls
/// out of a `fn` with a custom `Try` impl via `From`, so we translate
/// param-parse failures here.
fn parse_params<T: for<'de> Deserialize<'de>>(params: serde_json::Value) -> Result<T, ErrorObject> {
    serde_json::from_value(params)
        .map_err(|e| ErrorObject::new(ErrorObject::INVALID_PARAMS, format!("invalid params: {e}")))
}

#[derive(Deserialize)]
struct WorkspaceParams {
    root: Utf8PathBuf,
}

#[derive(Deserialize)]
struct BumpParams {
    root: Utf8PathBuf,
    #[serde(default)]
    last_hashes: indexmap::IndexMap<String, String>,
}

#[derive(Deserialize)]
struct SubscribeParams {
    channels: Vec<String>,
}

#[derive(Serialize)]
struct ServerInfo {
    version: &'static str,
    pid: u32,
    uptime_seconds: u64,
}

fn server_info(state: &Arc<State>) -> ServerInfo {
    ServerInfo {
        version: env!("CARGO_PKG_VERSION"),
        pid: std::process::id(),
        uptime_seconds: state.started_at.elapsed().as_secs(),
    }
}

enum WorkspaceOp {
    List,
    Status,
    Graph,
}

fn handle_workspace(
    state: &Arc<State>,
    params: serde_json::Value,
    op: WorkspaceOp,
) -> Result<serde_json::Value, ErrorObject> {
    let p: WorkspaceParams = parse_params(params)?;
    let root = normalize_root(&p.root)?;

    // Fast path: warm cache, no fs work.
    if let Some(cached) = state.snapshots.inner.read().get(&root) {
        return Ok(match op {
            WorkspaceOp::List => cached.workspace_list.clone(),
            WorkspaceOp::Status => cached.workspace_status.clone(),
            WorkspaceOp::Graph => cached.workspace_graph.clone(),
        });
    }

    // Slow path: compute + cache. We do discovery outside the lock to
    // keep contention low. We also register this root with the fs
    // watcher so future edits invalidate the cache we're about to fill.
    if let Err(e) = state.watcher.watch(root.as_std_path()) {
        // Non-fatal — caching still works, we just lose tightness.
        warn!(?root, "watcher.watch failed: {e}");
    }
    let computed = compute_snapshot(&root)?;
    state.snapshots.inner.write().insert(root.clone(), computed);

    let cache = state.snapshots.inner.read();
    let c = cache
        .get(&root)
        .ok_or_else(|| ErrorObject::new(ErrorObject::INTERNAL_ERROR, "cache vanished"))?;
    Ok(match op {
        WorkspaceOp::List => c.workspace_list.clone(),
        WorkspaceOp::Status => c.workspace_status.clone(),
        WorkspaceOp::Graph => c.workspace_graph.clone(),
    })
}

fn compute_snapshot(root: &Utf8PathBuf) -> Result<CachedSnapshot, ErrorObject> {
    use versionx_workspace::{ComponentGraph, discovery, hash};

    let ws = discovery::discover(root).map_err(|e| {
        ErrorObject::new(ErrorObject::WORKSPACE_FAILED, format!("discovery failed: {e}"))
    })?;
    let graph = ComponentGraph::build(&ws).map_err(|e| {
        ErrorObject::new(ErrorObject::WORKSPACE_FAILED, format!("graph build failed: {e}"))
    })?;

    // Build JSON payloads that match the shapes the core command
    // implementations return. We mirror the field names so clients can
    // deserialize either way.
    let list_entries: Vec<_> = ws
        .components
        .values()
        .map(|c| {
            serde_json::json!({
                "id": c.id.to_string(),
                "display_name": c.display_name,
                "kind": c.kind.as_str(),
                "root": c.root.to_string(),
                "version": c.version.as_ref().map(ToString::to_string),
                "source": match &c.source {
                    versionx_workspace::ComponentSource::Manifest { manifest_path } =>
                        format!("manifest:{manifest_path}"),
                    versionx_workspace::ComponentSource::Declared => "declared".into(),
                },
                "depends_on": c.depends_on.iter().map(ToString::to_string).collect::<Vec<_>>(),
            })
        })
        .collect();

    let mut status_entries = Vec::with_capacity(ws.components.len());
    for component in ws.components.values() {
        let current = hash::hash_component(&component.root, &component.inputs).map_err(|e| {
            ErrorObject::new(ErrorObject::WORKSPACE_FAILED, format!("hash failed: {e}"))
        })?;
        let cascade: Vec<_> =
            graph.transitive_dependents(&component.id).into_iter().map(|c| c.to_string()).collect();
        status_entries.push(serde_json::json!({
            "id": component.id.to_string(),
            "kind": component.kind.as_str(),
            "version": component.version.as_ref().map(ToString::to_string),
            "current_hash": current,
            "last_hash": serde_json::Value::Null,
            "dirty": true,
            "cascade": cascade,
        }));
    }

    let edges: Vec<_> = ws
        .components
        .values()
        .flat_map(|c| {
            c.depends_on.iter().map(
                move |dep| serde_json::json!({"from": c.id.to_string(), "to": dep.to_string()}),
            )
        })
        .collect();

    let list = serde_json::json!({
        "workspace_root": ws.root.to_string(),
        "components": list_entries,
    });
    let status = serde_json::json!({
        "workspace_root": ws.root.to_string(),
        "components": status_entries,
        "any_dirty": !ws.components.is_empty(),
    });
    let graph_json = serde_json::json!({
        "workspace_root": ws.root.to_string(),
        "nodes": ws.components.keys().map(ToString::to_string).collect::<Vec<_>>(),
        "edges": edges,
        "topo_order": graph.topo_order().into_iter().map(|c| c.to_string()).collect::<Vec<_>>(),
    });

    Ok(CachedSnapshot {
        workspace_list: list,
        workspace_status: status,
        workspace_graph: graph_json,
    })
}

fn handle_bump(
    state: &Arc<State>,
    params: serde_json::Value,
) -> Result<serde_json::Value, ErrorObject> {
    let p: BumpParams = parse_params(params)?;
    let root = normalize_root(&p.root)?;

    // We don't currently cache the bump plan because it depends on the
    // caller-supplied `last_hashes`. Recompute each time — it's cheap
    // (hashing only, no network).
    use versionx_workspace::{ComponentGraph, discovery, hash};
    let ws = discovery::discover(&root).map_err(|e| {
        ErrorObject::new(ErrorObject::WORKSPACE_FAILED, format!("discovery failed: {e}"))
    })?;
    let graph = ComponentGraph::build(&ws).map_err(|e| {
        ErrorObject::new(ErrorObject::WORKSPACE_FAILED, format!("graph build failed: {e}"))
    })?;

    let mut dirty: Vec<String> = Vec::new();
    for c in ws.components.values() {
        let current = hash::hash_component(&c.root, &c.inputs).map_err(|e| {
            ErrorObject::new(ErrorObject::WORKSPACE_FAILED, format!("hash failed: {e}"))
        })?;
        let prior = p.last_hashes.get(c.id.as_str());
        if prior.map(String::as_str) != Some(current.as_str()) {
            dirty.push(c.id.to_string());
        }
    }

    // Small payload so we don't bother with a cache miss emitting a
    // notification. Idempotent and cheap.
    let _ = state; // reserved for future caching
    let _ = graph; // reserved for cascade preview — the client does the full
    // bump math itself for now.
    Ok(serde_json::json!({
        "workspace_root": ws.root.to_string(),
        "dirty_components": dirty,
    }))
}

fn normalize_root(p: &Utf8PathBuf) -> Result<Utf8PathBuf, ErrorObject> {
    let canonical = std::fs::canonicalize(p.as_std_path()).map_err(|e| {
        ErrorObject::new(ErrorObject::INVALID_PARAMS, format!("cannot canonicalize {p}: {e}"))
    })?;
    Utf8PathBuf::from_path_buf(canonical).map_err(|bad| {
        ErrorObject::new(
            ErrorObject::INVALID_PARAMS,
            format!("root is not valid UTF-8: {}", bad.to_string_lossy()),
        )
    })
}

// -------- Subscriptions --------------------------------------------------

#[derive(Default)]
struct Subscriptions {
    channels: std::collections::BTreeSet<String>,
    wildcard: bool,
}

impl Subscriptions {
    fn add(&mut self, channels: Vec<String>) {
        for c in channels {
            if c == "*" {
                self.wildcard = true;
            } else {
                self.channels.insert(c);
            }
        }
    }
    fn remove(&mut self, channels: &[String]) {
        for c in channels {
            if c == "*" {
                self.wildcard = false;
            } else {
                self.channels.remove(c);
            }
        }
    }
    fn matches(&self, method: &str) -> bool {
        self.wildcard || self.channels.contains(method)
    }
}

// -------- Idle watchdog --------------------------------------------------

fn spawn_idle_watchdog(state: Arc<State>, timeout: Option<Duration>) -> JoinHandle<()> {
    tokio::spawn(async move {
        let Some(timeout) = timeout else { return };
        // Check every minute — we don't need second-level precision on a
        // 30-minute idle timer.
        // Check once per minute — good-enough resolution for a 30-min watchdog.
        let mut interval = tokio::time::interval(Duration::from_mins(1));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            let idle = state.last_activity.lock().elapsed();
            if idle >= timeout {
                info!(?idle, "idle timeout reached, shutting down");
                state.shutdown.notify_waiters();
                return;
            }
        }
    })
}
