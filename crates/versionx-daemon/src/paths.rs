//! Where the daemon lives on disk.
//!
//! Layout under `$VERSIONX_HOME` (or `~/.versionx` by default):
//!
//! ```text
//! <home>/
//!   run/
//!     versiond.sock  — UDS (Unix)
//!     versiond.pipe  — placeholder marker (Windows)
//!     versiond.pid   — numeric PID, single-writer
//!     versiond.lock  — flock held by the running daemon
//!   logs/
//!     versiond.log   — rolling structured log output
//! ```
//!
//! Windows uses `\\.\pipe\versiond_<user>` for the actual pipe and keeps the
//! `versiond.pipe` file just as a discoverable marker for tooling.

use camino::{Utf8Path, Utf8PathBuf};

#[derive(Clone, Debug)]
pub struct DaemonPaths {
    pub home: Utf8PathBuf,
    pub run_dir: Utf8PathBuf,
    pub log_dir: Utf8PathBuf,
    pub socket: Utf8PathBuf,
    pub pid_file: Utf8PathBuf,
    pub lock_file: Utf8PathBuf,
    pub log_file: Utf8PathBuf,
}

impl DaemonPaths {
    /// Resolve paths for the given `VERSIONX_HOME`. No filesystem work — call
    /// [`Self::ensure_dirs`] before writing anything.
    pub fn under(home: impl AsRef<Utf8Path>) -> Self {
        let home = home.as_ref().to_path_buf();
        let run_dir = home.join("run");
        let log_dir = home.join("logs");
        Self {
            socket: run_dir.join(socket_leaf()),
            pid_file: run_dir.join("versiond.pid"),
            lock_file: run_dir.join("versiond.lock"),
            log_file: log_dir.join("versiond.log"),
            run_dir,
            log_dir,
            home,
        }
    }

    /// Pick up paths from the process environment the same way the rest of
    /// versionx does (`VERSIONX_HOME` → `~/.versionx`).
    ///
    /// # Errors
    /// Returns `None` if the home directory can't be determined.
    pub fn from_env() -> Option<Self> {
        let home = if let Ok(h) = std::env::var("VERSIONX_HOME") {
            Utf8PathBuf::from(h)
        } else {
            let base = directories::BaseDirs::new()?;
            Utf8PathBuf::from_path_buf(base.home_dir().join(".versionx")).ok()?
        };
        Some(Self::under(home))
    }

    pub fn ensure_dirs(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.run_dir)?;
        std::fs::create_dir_all(&self.log_dir)?;
        Ok(())
    }

    /// Windows named-pipe path. Includes a short hash of the home dir
    /// so two daemons with different `VERSIONX_HOME`s (e.g. tests run
    /// in parallel under separate tempdirs) don't collide on the same
    /// pipe — `first_pipe_instance(true)` would otherwise reject the
    /// second binder with "Access is denied".
    ///
    /// Format: `\\.\pipe\versiond_<user>_<8hex>`.
    #[cfg(windows)]
    pub fn windows_pipe_name(&self) -> String {
        let user = std::env::var("USERNAME").unwrap_or_else(|_| "default".into());
        let hash = blake3::hash(self.home.as_str().as_bytes());
        let short = &hash.to_hex().to_string()[..8];
        format!(r"\\.\pipe\versiond_{user}_{short}")
    }
}

#[cfg(unix)]
const fn socket_leaf() -> &'static str {
    "versiond.sock"
}

#[cfg(windows)]
const fn socket_leaf() -> &'static str {
    "versiond.pipe"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_rooted_under_home() {
        let p = DaemonPaths::under("/tmp/vxhome");
        assert!(p.socket.starts_with("/tmp/vxhome/run"));
        assert!(p.pid_file.starts_with("/tmp/vxhome/run"));
        assert!(p.log_file.starts_with("/tmp/vxhome/logs"));
    }
}
