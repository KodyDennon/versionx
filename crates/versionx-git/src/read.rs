//! Git read operations via `gix`.
//!
//! Everything here is side-effect free — we open the repo read-only,
//! inspect, close. `gix` is materially faster than `git2` for
//! read-heavy paths (log traversal, ref listing) so we route reads
//! through it even though the write path uses `git2`.

use camino::{Utf8Path, Utf8PathBuf};
use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum ReadError {
    #[error("gix error at {path}: {message}")]
    Gix { path: Utf8PathBuf, message: String },
    #[error("{path} is not a git repository")]
    NotARepo { path: Utf8PathBuf },
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub type ReadResult<T> = Result<T, ReadError>;

/// Minimal repo metadata a caller typically wants together.
#[derive(Clone, Debug, Serialize)]
pub struct RepoSummary {
    pub workdir: Utf8PathBuf,
    pub head_sha: String,
    pub head_ref: Option<String>,
    pub dirty: bool,
    pub remotes: Vec<RemoteInfo>,
}

#[derive(Clone, Debug, Serialize)]
pub struct RemoteInfo {
    pub name: String,
    pub url: String,
}

/// Collect everything a caller needs in one pass.
pub fn summarize(repo_path: &Utf8Path) -> ReadResult<RepoSummary> {
    let repo = gix::open(repo_path.as_std_path()).map_err(|e| map(repo_path, e))?;
    let head_id = repo.head_id().map_err(|e| map(repo_path, e))?;
    let head_sha = head_id.to_string();
    let head_ref = repo.head_name().ok().flatten().map(|n| n.as_bstr().to_string());

    let remotes: Vec<RemoteInfo> = repo
        .remote_names()
        .into_iter()
        .filter_map(|name| {
            let url = repo
                .find_remote(name.as_ref())
                .ok()?
                .url(gix::remote::Direction::Fetch)
                .map(|u| u.to_bstring().to_string())?;
            Some(RemoteInfo { name: name.to_string(), url })
        })
        .collect();

    let workdir = repo
        .work_dir()
        .map(|p| {
            Utf8PathBuf::from_path_buf(p.to_path_buf()).unwrap_or_else(|_| repo_path.to_path_buf())
        })
        .unwrap_or_else(|| repo_path.to_path_buf());

    let dirty = is_dirty(repo_path).unwrap_or(true);

    Ok(RepoSummary { workdir, head_sha, head_ref, dirty, remotes })
}

/// Working-tree dirtiness — any tracked modification or untracked file.
/// We go through `git2` here because gix's status API is still maturing
/// and accuracy matters for release safety.
pub fn is_dirty(repo_path: &Utf8Path) -> ReadResult<bool> {
    let repo = git2::Repository::open(repo_path.as_std_path())
        .map_err(|_| ReadError::NotARepo { path: repo_path.to_path_buf() })?;
    let mut opts = git2::StatusOptions::new();
    opts.include_ignored(false).include_untracked(true).recurse_untracked_dirs(true);
    let statuses = repo
        .statuses(Some(&mut opts))
        .map_err(|e| ReadError::Gix { path: repo_path.to_path_buf(), message: e.to_string() })?;
    Ok(statuses.iter().any(|e| !e.status().is_empty()))
}

/// Return commit messages on HEAD, newest first, up to `max`.
pub fn log_messages(repo_path: &Utf8Path, max: usize) -> ReadResult<Vec<LogEntry>> {
    let repo = gix::open(repo_path.as_std_path()).map_err(|e| map(repo_path, e))?;
    let head_id = repo.head_id().map_err(|e| map(repo_path, e))?;
    let mut out = Vec::with_capacity(max);
    let walk = repo
        .rev_walk([head_id])
        .sorting(gix::revision::walk::Sorting::ByCommitTime(
            gix::traverse::commit::simple::CommitTimeOrder::NewestFirst,
        ))
        .all()
        .map_err(|e| map(repo_path, e))?;
    for info in walk.take(max) {
        let info = info.map_err(|e| map(repo_path, e))?;
        let commit = repo.find_commit(info.id).map_err(|e| map(repo_path, e))?;
        let message = commit.message_raw().map_err(|e| map(repo_path, e))?.to_string();
        let time = commit.time().map_err(|e| map(repo_path, e))?;
        out.push(LogEntry { sha: info.id.to_string(), message, timestamp: time.seconds });
    }
    Ok(out)
}

#[derive(Clone, Debug, Serialize)]
pub struct LogEntry {
    pub sha: String,
    pub message: String,
    pub timestamp: i64,
}

/// Last tag pointing at `HEAD` or its ancestors. Returns `None` when no
/// tags are present.
pub fn latest_tag(repo_path: &Utf8Path) -> ReadResult<Option<String>> {
    let repo = git2::Repository::open(repo_path.as_std_path())
        .map_err(|_| ReadError::NotARepo { path: repo_path.to_path_buf() })?;
    let mut opts = git2::DescribeOptions::new();
    opts.describe_tags();
    let formatter = git2::DescribeFormatOptions::new();
    match repo.describe(&opts).and_then(|d| d.format(Some(&formatter))) {
        Ok(name) => Ok(Some(name)),
        Err(_) => Ok(None),
    }
}

fn map(path: &Utf8Path, e: impl std::fmt::Display) -> ReadError {
    ReadError::Gix { path: path.to_path_buf(), message: e.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    #[allow(unused_imports)]
    use std::process::Command;

    fn init_repo(root: &Utf8Path) -> git2::Repository {
        let repo = git2::Repository::init(root.as_std_path()).unwrap();
        let mut cfg = repo.config().unwrap();
        cfg.set_str("user.name", "Tester").unwrap();
        cfg.set_str("user.email", "t@example.com").unwrap();
        repo
    }

    fn commit(root: &Utf8Path, file: &str, body: &str, msg: &str) -> String {
        fs::write(root.join(file), body).unwrap();
        // Use git2 directly to avoid depending on the user's global
        // git identity (which CI may not have set).
        let repo = git2::Repository::open(root.as_std_path()).unwrap();
        let mut index = repo.index().unwrap();
        index.add_all(std::iter::once("."), git2::IndexAddOption::DEFAULT, None).unwrap();
        index.write().unwrap();
        let tree_oid = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_oid).unwrap();
        let sig = repo.signature().unwrap();
        let parents: Vec<git2::Commit> = match repo.head() {
            Ok(h) => vec![h.peel_to_commit().unwrap()],
            Err(_) => Vec::new(),
        };
        let refs: Vec<&git2::Commit> = parents.iter().collect();
        let oid = repo.commit(Some("HEAD"), &sig, &sig, msg, &tree, &refs).unwrap();
        let _ = Command::new("true"); // silence import warning
        oid.to_string()
    }

    #[test]
    fn summarize_fresh_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        init_repo(&root);
        commit(&root, "a.txt", "hi", "init");
        let s = summarize(&root).unwrap();
        assert!(!s.dirty);
        assert_eq!(s.head_sha.len(), 40);
    }

    #[test]
    fn dirty_reported_for_untracked() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        init_repo(&root);
        commit(&root, "a.txt", "hi", "init");
        fs::write(root.join("b.txt"), "new").unwrap();
        assert!(is_dirty(&root).unwrap());
    }

    #[test]
    fn log_returns_newest_first() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        init_repo(&root);
        commit(&root, "a.txt", "1", "first");
        commit(&root, "a.txt", "2", "second");
        let msgs = log_messages(&root, 5).unwrap();
        assert!(msgs.len() >= 2);
        assert!(msgs[0].message.contains("second"));
    }
}
