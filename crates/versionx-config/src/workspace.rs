//! Workspace root detection.
//!
//! Rule (from `docs/spec/02-config-and-state-model.md §2.5`):
//! 1. Walk up from the starting directory looking for a `versionx.toml`
//!    whose `[versionx] workspace = true`. If found, that's the root.
//! 2. Otherwise, walk up for any `versionx.toml`. Nearest wins.
//! 3. Otherwise, use the git root (`git rev-parse --show-toplevel` equivalent
//!    — we look for a `.git` directory or file).
//! 4. Otherwise, use the starting directory with a warning.

use camino::{Utf8Path, Utf8PathBuf};

use crate::error::{ConfigError, ConfigResult};
use crate::schema::VersionxConfig;

/// The discovered workspace root + how we found it.
#[derive(Clone, Debug)]
pub struct WorkspaceRoot {
    /// Absolute path to the root directory.
    pub path: Utf8PathBuf,
    /// How the root was determined.
    pub reason: DiscoveryReason,
}

/// Why a particular directory was chosen as the workspace root.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DiscoveryReason {
    /// Found a `versionx.toml` with `workspace = true`.
    DeclaredWorkspace,
    /// Found the nearest `versionx.toml`.
    NearestConfig,
    /// Fell back to the git repo root.
    GitRoot,
    /// Fell back to the starting directory.
    Fallback,
}

/// Reader abstraction so tests can exercise detection without touching the
/// real filesystem. Methods are pure, no I/O allowed in test impls.
pub trait FsReader {
    /// True if `path` is a directory that exists.
    fn is_dir(&self, path: &Utf8Path) -> bool;
    /// True if `path` is a file that exists.
    fn is_file(&self, path: &Utf8Path) -> bool;
    /// Read a file as UTF-8. `Err(NotFound)` if missing.
    fn read_to_string(&self, path: &Utf8Path) -> std::io::Result<String>;
}

/// Default reader: the real filesystem.
#[derive(Debug, Default)]
pub struct RealFs;

impl FsReader for RealFs {
    fn is_dir(&self, path: &Utf8Path) -> bool {
        path.is_dir()
    }
    fn is_file(&self, path: &Utf8Path) -> bool {
        path.is_file()
    }
    fn read_to_string(&self, path: &Utf8Path) -> std::io::Result<String> {
        std::fs::read_to_string(path)
    }
}

/// Detect the workspace root starting at `start` (typically `cwd`).
pub fn detect_workspace_root(start: &Utf8Path) -> ConfigResult<WorkspaceRoot> {
    detect_workspace_root_with(&RealFs, start)
}

/// Detect with an injectable filesystem reader.
pub fn detect_workspace_root_with(
    fs: &dyn FsReader,
    start: &Utf8Path,
) -> ConfigResult<WorkspaceRoot> {
    if !fs.is_dir(start) {
        return Err(ConfigError::Invalid {
            path: start.to_path_buf(),
            message: format!("workspace root start `{start}` is not a directory"),
        });
    }

    // Pass 1: look for a declared workspace root (versionx.toml with workspace = true).
    let mut cursor: Option<&Utf8Path> = Some(start);
    let mut nearest_config_dir: Option<Utf8PathBuf> = None;
    while let Some(dir) = cursor {
        let candidate = dir.join("versionx.toml");
        if fs.is_file(&candidate) {
            if nearest_config_dir.is_none() {
                nearest_config_dir = Some(dir.to_path_buf());
            }
            // Read just enough to answer the "is it workspace = true" question.
            if let Ok(contents) = fs.read_to_string(&candidate)
                && let Ok(parsed) = toml::from_str::<VersionxConfig>(&contents)
                && parsed.versionx.workspace
            {
                return Ok(WorkspaceRoot {
                    path: dir.to_path_buf(),
                    reason: DiscoveryReason::DeclaredWorkspace,
                });
            }
        }
        cursor = dir.parent();
    }

    // Pass 2: nearest config wins.
    if let Some(dir) = nearest_config_dir {
        return Ok(WorkspaceRoot { path: dir, reason: DiscoveryReason::NearestConfig });
    }

    // Pass 3: git root.
    if let Some(git_root) = find_git_root(fs, start) {
        return Ok(WorkspaceRoot { path: git_root, reason: DiscoveryReason::GitRoot });
    }

    // Pass 4: fallback to the starting directory.
    Ok(WorkspaceRoot { path: start.to_path_buf(), reason: DiscoveryReason::Fallback })
}

fn find_git_root(fs: &dyn FsReader, start: &Utf8Path) -> Option<Utf8PathBuf> {
    let mut cursor: Option<&Utf8Path> = Some(start);
    while let Some(dir) = cursor {
        let git = dir.join(".git");
        // .git is typically a directory, but in worktrees / submodules it
        // can be a file pointing elsewhere. Either counts as a git root.
        if fs.is_dir(&git) || fs.is_file(&git) {
            return Some(dir.to_path_buf());
        }
        cursor = dir.parent();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// In-memory fake FS for deterministic tests.
    struct FakeFs {
        dirs: HashMap<Utf8PathBuf, ()>,
        files: HashMap<Utf8PathBuf, String>,
    }

    impl FakeFs {
        fn new() -> Self {
            Self { dirs: HashMap::new(), files: HashMap::new() }
        }
        fn mkdir(&mut self, p: &str) -> &mut Self {
            self.dirs.insert(Utf8PathBuf::from(p), ());
            self
        }
        fn write(&mut self, p: &str, contents: &str) -> &mut Self {
            self.files.insert(Utf8PathBuf::from(p), contents.into());
            // Also ensure parent dirs exist.
            let mut cur = Utf8PathBuf::from(p);
            while let Some(parent) = cur.parent() {
                self.dirs.insert(parent.to_path_buf(), ());
                cur = parent.to_path_buf();
            }
            self
        }
    }

    impl FsReader for FakeFs {
        fn is_dir(&self, path: &Utf8Path) -> bool {
            self.dirs.contains_key(path)
        }
        fn is_file(&self, path: &Utf8Path) -> bool {
            self.files.contains_key(path)
        }
        fn read_to_string(&self, path: &Utf8Path) -> std::io::Result<String> {
            self.files
                .get(path)
                .cloned()
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "nope"))
        }
    }

    #[test]
    fn declared_workspace_wins_over_nearest() {
        let mut fs = FakeFs::new();
        fs.mkdir("/a/b/c")
            .write("/a/versionx.toml", "[versionx]\nworkspace = true\n")
            .write("/a/b/versionx.toml", "[runtimes]\nnode = \"20\"\n");
        let root = detect_workspace_root_with(&fs, Utf8Path::new("/a/b/c")).unwrap();
        assert_eq!(root.path, Utf8PathBuf::from("/a"));
        assert_eq!(root.reason, DiscoveryReason::DeclaredWorkspace);
    }

    #[test]
    fn nearest_config_wins_when_no_declared_workspace() {
        let mut fs = FakeFs::new();
        fs.mkdir("/a/b/c")
            .write("/a/versionx.toml", "[runtimes]\n")
            .write("/a/b/versionx.toml", "[runtimes]\n");
        let root = detect_workspace_root_with(&fs, Utf8Path::new("/a/b/c")).unwrap();
        assert_eq!(root.path, Utf8PathBuf::from("/a/b"));
        assert_eq!(root.reason, DiscoveryReason::NearestConfig);
    }

    #[test]
    fn falls_back_to_git_root() {
        let mut fs = FakeFs::new();
        fs.mkdir("/repo/src/deep").write("/repo/.git/HEAD", "ref: refs/heads/main\n");
        let root = detect_workspace_root_with(&fs, Utf8Path::new("/repo/src/deep")).unwrap();
        assert_eq!(root.path, Utf8PathBuf::from("/repo"));
        assert_eq!(root.reason, DiscoveryReason::GitRoot);
    }

    #[test]
    fn falls_back_to_cwd_when_nothing_else() {
        let mut fs = FakeFs::new();
        fs.mkdir("/tmp/empty");
        let root = detect_workspace_root_with(&fs, Utf8Path::new("/tmp/empty")).unwrap();
        assert_eq!(root.path, Utf8PathBuf::from("/tmp/empty"));
        assert_eq!(root.reason, DiscoveryReason::Fallback);
    }

    #[test]
    fn erroring_on_non_directory_start() {
        let fs = FakeFs::new();
        let err = detect_workspace_root_with(&fs, Utf8Path::new("/does/not/exist")).unwrap_err();
        assert!(matches!(err, ConfigError::Invalid { .. }));
    }
}
