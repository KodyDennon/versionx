//! Platform-agnostic duplex stream for the IPC socket.
//!
//! On Unix this is a [`tokio::net::UnixStream`]; on Windows it's a named-pipe
//! stream. Both sides speak the same `Framed<_, JsonFrameCodec>` wrapper,
//! so the dispatch code doesn't have to care about the platform.
//!
//! ### Security
//!
//! - Unix: the socket file is created with `mode 0600` after bind. Parent
//!   dir is created with the default umask — it lives under
//!   `$VERSIONX_HOME/run/`, which is user-owned by construction.
//! - Windows: the named pipe is created with `reject_remote_clients = true`,
//!   so only local processes on the same machine can connect.
//!
//! ### Why not `interprocess`?
//!
//! It would do most of this but adds a non-trivial dep for what is ~200
//! lines of platform code. We keep the surface small and tokio-native.

use std::io;

use tokio_util::codec::Framed;

use crate::codec::JsonFrameCodec;
use crate::paths::DaemonPaths;

// -------------- Unix -----------------------------------------------------

#[cfg(unix)]
mod platform {
    use std::os::unix::fs::PermissionsExt;

    use tokio::net::{UnixListener, UnixStream};

    use crate::paths::DaemonPaths;

    pub type DuplexStream = UnixStream;

    #[derive(Debug)]
    pub struct Listener {
        inner: UnixListener,
    }

    impl Listener {
        pub async fn bind(paths: &DaemonPaths) -> std::io::Result<Self> {
            paths.ensure_dirs()?;
            // Remove any stale socket from a crashed previous run.
            let _ = std::fs::remove_file(paths.socket.as_std_path());
            let inner = UnixListener::bind(paths.socket.as_std_path())?;
            let perms = std::fs::Permissions::from_mode(0o600);
            std::fs::set_permissions(paths.socket.as_std_path(), perms)?;
            Ok(Self { inner })
        }

        pub async fn accept(&self) -> std::io::Result<DuplexStream> {
            let (s, _) = self.inner.accept().await?;
            Ok(s)
        }
    }

    pub async fn connect(paths: &DaemonPaths) -> std::io::Result<DuplexStream> {
        UnixStream::connect(paths.socket.as_std_path()).await
    }
}

// -------------- Windows --------------------------------------------------

#[cfg(windows)]
mod platform {
    use pin_project_lite::pin_project;
    use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
    use tokio::net::windows::named_pipe::{
        ClientOptions, NamedPipeClient, NamedPipeServer, ServerOptions,
    };

    use crate::paths::DaemonPaths;

    pin_project! {
        /// Either end of a Windows named-pipe connection. Pin-projected so
        /// the AsyncRead/AsyncWrite impls stay `unsafe`-free.
        #[project = DuplexProj]
        pub enum DuplexStream {
            Server { #[pin] inner: NamedPipeServer },
            Client { #[pin] inner: NamedPipeClient },
        }
    }

    impl AsyncRead for DuplexStream {
        fn poll_read(
            self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buf: &mut ReadBuf<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            match self.project() {
                DuplexProj::Server { inner } => inner.poll_read(cx, buf),
                DuplexProj::Client { inner } => inner.poll_read(cx, buf),
            }
        }
    }

    impl AsyncWrite for DuplexStream {
        fn poll_write(
            self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buf: &[u8],
        ) -> std::task::Poll<std::io::Result<usize>> {
            match self.project() {
                DuplexProj::Server { inner } => inner.poll_write(cx, buf),
                DuplexProj::Client { inner } => inner.poll_write(cx, buf),
            }
        }

        fn poll_flush(
            self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            match self.project() {
                DuplexProj::Server { inner } => inner.poll_flush(cx),
                DuplexProj::Client { inner } => inner.poll_flush(cx),
            }
        }

        fn poll_shutdown(
            self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            match self.project() {
                DuplexProj::Server { inner } => inner.poll_shutdown(cx),
                DuplexProj::Client { inner } => inner.poll_shutdown(cx),
            }
        }
    }

    #[derive(Debug)]
    pub struct Listener {
        pipe_name: String,
        server: parking_lot::Mutex<Option<NamedPipeServer>>,
    }

    impl Listener {
        pub async fn bind(paths: &DaemonPaths) -> std::io::Result<Self> {
            paths.ensure_dirs()?;
            let pipe_name = paths.windows_pipe_name();
            let server = ServerOptions::new()
                .first_pipe_instance(true)
                .reject_remote_clients(true)
                .create(&pipe_name)?;
            // Marker file so tooling / humans can see the pipe path.
            let _ = std::fs::write(paths.socket.as_std_path(), pipe_name.as_bytes());
            Ok(Self { pipe_name, server: parking_lot::Mutex::new(Some(server)) })
        }

        pub async fn accept(&self) -> std::io::Result<DuplexStream> {
            let server = self.server.lock().take().ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "named pipe listener already consumed",
                )
            })?;
            server.connect().await?;
            let next = ServerOptions::new().reject_remote_clients(true).create(&self.pipe_name)?;
            *self.server.lock() = Some(next);
            Ok(DuplexStream::Server { inner: server })
        }
    }

    pub async fn connect(paths: &DaemonPaths) -> std::io::Result<DuplexStream> {
        let name = paths.windows_pipe_name();
        let client = ClientOptions::new().open(&name)?;
        Ok(DuplexStream::Client { inner: client })
    }
}

pub use platform::{DuplexStream, Listener, connect};

/// Wrap a raw duplex in the framed codec.
pub fn framed(stream: DuplexStream) -> Framed<DuplexStream, JsonFrameCodec> {
    Framed::new(stream, JsonFrameCodec::new())
}

/// Quick liveness probe. Returns `Ok(true)` only if a connection succeeds
/// within `timeout`.
pub async fn probe(paths: &DaemonPaths, timeout: std::time::Duration) -> io::Result<bool> {
    match tokio::time::timeout(timeout, connect(paths)).await {
        Ok(Ok(_)) => Ok(true),
        Ok(Err(_)) | Err(_) => Ok(false),
    }
}
