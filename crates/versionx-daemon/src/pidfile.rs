//! Single-instance enforcement via a PID file + advisory file lock.
//!
//! On startup the daemon:
//!   1. Opens the lock file at `<run>/versiond.lock` and `flock(LOCK_EX | LOCK_NB)`.
//!   2. Writes its numeric PID to `<run>/versiond.pid` (0o600).
//!
//! If (1) fails, another daemon is alive and we exit with a friendly
//! `AlreadyRunning` error. The lock is released automatically when the
//! process dies (kernel cleans it up), so no graceful-shutdown hook is
//! required for correctness — we still clean the pid file on normal exit
//! to avoid confusing `versionx daemon status`.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};

use camino::{Utf8Path, Utf8PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum PidFileError {
    #[error(
        "another versiond appears to be running (pid {pid}). Use `versionx daemon stop` or remove {path:?}."
    )]
    AlreadyRunning { pid: u32, path: Utf8PathBuf },
    #[error("io error: {0}")]
    Io(#[from] io::Error),
}

#[derive(Debug)]
pub struct PidFile {
    // The lock File needs to stay open for the duration of the process —
    // dropping closes the fd and releases the flock.
    _lock: File,
    pid_path: Utf8PathBuf,
}

impl PidFile {
    /// Acquire the lock + write our PID. Returns [`PidFileError::AlreadyRunning`]
    /// if a live daemon is already holding the lock.
    pub fn acquire(lock_path: &Utf8Path, pid_path: &Utf8Path) -> Result<Self, PidFileError> {
        // Make sure the parent directory is set up.
        if let Some(parent) = lock_path.parent() {
            fs::create_dir_all(parent.as_std_path())?;
        }

        // Open (or create) the lock file.
        let lock = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(lock_path.as_std_path())?;

        acquire_os_lock(&lock).map_err(|e| {
            if e.kind() == io::ErrorKind::WouldBlock {
                let pid = read_pid(pid_path).unwrap_or(0);
                PidFileError::AlreadyRunning { pid, path: pid_path.to_path_buf() }
            } else {
                PidFileError::Io(e)
            }
        })?;

        // Safe to write the pid now — we hold the lock.
        let mut pid_file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(pid_path.as_std_path())?;
        writeln!(pid_file, "{}", std::process::id())?;
        set_secure_perms(pid_path)?;

        Ok(Self { _lock: lock, pid_path: pid_path.to_path_buf() })
    }

    /// Remove the pid file. The lock will release on drop anyway, but
    /// cleaning the pid file prevents a stale-looking "daemon running"
    /// report from `versionx daemon status`.
    pub fn release(self) {
        let _ = fs::remove_file(self.pid_path.as_std_path());
    }
}

impl Drop for PidFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(self.pid_path.as_std_path());
    }
}

/// Read the current pid stored at `pid_path`, if any. Returns `None` if
/// missing or malformed.
pub fn read_pid(pid_path: &Utf8Path) -> Option<u32> {
    let raw = fs::read_to_string(pid_path.as_std_path()).ok()?;
    raw.trim().parse().ok()
}

/// Best-effort check that `pid` is alive. On Unix we signal 0; on Windows
/// we simply trust that if the pid file exists the daemon is *probably*
/// alive — callers should double-check by probing the socket (which is
/// the only thing we actually care about).
#[cfg(unix)]
#[allow(unsafe_code)] // Narrow FFI: kill(pid, 0) has no side effects.
pub fn is_alive(pid: u32) -> bool {
    unsafe extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    #[allow(clippy::cast_possible_wrap)] // pids fit in i32 on all supported platforms.
    let res = unsafe { kill(pid as i32, 0) };
    // 0 → process exists and we have permission to signal it.
    // EPERM (1) → exists but we can't signal. ESRCH → gone.
    res == 0 || std::io::Error::last_os_error().raw_os_error() == Some(1)
}

#[cfg(not(unix))]
pub fn is_alive(pid: u32) -> bool {
    // On non-Unix we don't have a cheap liveness syscall without pulling
    // in a dep, so we only treat pid=0 as "not alive" and leave the rest
    // to the socket probe.
    pid != 0
}

// --------- Platform-specific helpers -------------------------------------

#[cfg(unix)]
#[allow(unsafe_code)] // Narrow FFI: flock(2) on an owned fd.
fn acquire_os_lock(file: &File) -> io::Result<()> {
    use std::os::unix::io::AsRawFd;
    unsafe extern "C" {
        fn flock(fd: std::os::unix::io::RawFd, operation: i32) -> i32;
    }
    let fd = file.as_raw_fd();
    let rc = unsafe { flock(fd, 2 | 4) }; // LOCK_EX | LOCK_NB
    if rc == 0 { Ok(()) } else { Err(io::Error::last_os_error()) }
}

#[cfg(not(unix))]
fn acquire_os_lock(_file: &File) -> io::Result<()> {
    // On non-Unix, rely on the single-writer named-pipe listener in
    // `transport` for mutual exclusion. `first_pipe_instance = true`
    // returns an error if another server already owns the pipe.
    Ok(())
}

#[cfg(unix)]
fn set_secure_perms(path: &Utf8Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let perms = fs::Permissions::from_mode(0o600);
    fs::set_permissions(path.as_std_path(), perms)
}

#[cfg(not(unix))]
fn set_secure_perms(_path: &Utf8Path) -> io::Result<()> {
    // Windows perms inherit from the parent ACL, which is user-scoped in
    // `$VERSIONX_HOME`. No-op.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acquire_and_release() {
        let tmp = tempfile::tempdir().unwrap();
        let root = camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        let lock = root.join("versiond.lock");
        let pid = root.join("versiond.pid");
        let pf = PidFile::acquire(&lock, &pid).unwrap();
        assert!(pid.is_file());
        pf.release();
        assert!(!pid.is_file());
    }

    // Windows uses the named-pipe `first_pipe_instance = true` flag in
    // `transport` for mutual exclusion (see `acquire_os_lock` above —
    // it's a no-op on Windows). The pidfile-level test only covers
    // the Unix flock path.
    #[cfg(unix)]
    #[test]
    fn second_acquire_blocks() {
        let tmp = tempfile::tempdir().unwrap();
        let root = camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        let lock = root.join("versiond.lock");
        let pid = root.join("versiond.pid");
        let _pf = PidFile::acquire(&lock, &pid).unwrap();
        let err = PidFile::acquire(&lock, &pid).unwrap_err();
        assert!(matches!(err, PidFileError::AlreadyRunning { .. }));
    }
}
