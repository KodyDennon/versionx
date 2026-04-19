//! `refs/versionx/history` — append-only git-backed run history.
//!
//! Every versionx run (release apply, fleet operation, bump, …) can
//! append a JSON-line commit under this private ref. The ref is *not*
//! fast-forwardable to a branch — it's a straight linear history used
//! for state-DB recovery.
//!
//! Design: each run is one commit whose tree contains a single file,
//! `run.json`, with the serialized event. Commits are chained so
//! `git log refs/versionx/history` gives a complete timeline even if
//! the local state DB is wiped.

use camino::Utf8Path;
use chrono::Utc;
use git2::{ObjectType, Repository};
use serde::{Deserialize, Serialize};

use crate::write::WriteError;

pub const HISTORY_REF: &str = "refs/versionx/history";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HistoryEvent {
    pub at: chrono::DateTime<Utc>,
    pub kind: String,
    pub summary: String,
    #[serde(default)]
    pub details: serde_json::Value,
}

impl HistoryEvent {
    pub fn new(kind: impl Into<String>, summary: impl Into<String>) -> Self {
        Self {
            at: Utc::now(),
            kind: kind.into(),
            summary: summary.into(),
            details: serde_json::Value::Null,
        }
    }

    #[must_use]
    pub fn with_details(mut self, details: serde_json::Value) -> Self {
        self.details = details;
        self
    }
}

/// Append `event` as a new commit under `refs/versionx/history`. The
/// commit's tree contains a single `run.json` file — we deliberately
/// don't care about tree shape, readers just `git cat-file blob`.
pub fn append(repo_path: &Utf8Path, event: &HistoryEvent) -> Result<String, WriteError> {
    let repo = crate::write::open(repo_path)?;
    let signature = {
        let mut sig = repo.signature().unwrap_or_else(|_| {
            git2::Signature::now("versionx", "versionx@localhost").expect("fallback sig")
        });
        sig = git2::Signature::new(
            sig.name().unwrap_or("versionx"),
            sig.email().unwrap_or("versionx@localhost"),
            &sig.when(),
        )
        .unwrap_or(sig);
        sig.to_owned()
    };

    // Build a tree with a single `run.json` blob.
    let json = serde_json::to_string_pretty(event).unwrap_or_else(|_| "{}".into());
    let blob_oid = repo.blob(json.as_bytes())?;
    let mut builder = repo.treebuilder(None)?;
    builder.insert("run.json", blob_oid, 0o100_644)?;
    let tree_oid = builder.write()?;
    let tree = repo.find_tree(tree_oid)?;

    // Parent = prior history tip, if any.
    let parent = repo.find_reference(HISTORY_REF).ok().and_then(|r| r.peel_to_commit().ok());
    let parent_refs: Vec<&git2::Commit> = parent.iter().collect();

    let oid = repo.commit(
        Some(HISTORY_REF),
        &signature,
        &signature,
        &format!("{}: {}", event.kind, event.summary),
        &tree,
        &parent_refs,
    )?;
    Ok(oid.to_string())
}

/// Read every history entry newest-first, up to `max`.
pub fn list(repo_path: &Utf8Path, max: usize) -> Result<Vec<HistoryEvent>, WriteError> {
    let repo = crate::write::open(repo_path)?;
    let Some(head) = repo.find_reference(HISTORY_REF).ok().and_then(|r| r.peel_to_commit().ok())
    else {
        return Ok(Vec::new());
    };

    let mut out = Vec::with_capacity(max);
    let mut current = Some(head);
    while let Some(commit) = current.take() {
        if out.len() >= max {
            break;
        }
        if let Some(event) = read_event_from_commit(&repo, &commit) {
            out.push(event);
        }
        current = commit.parent(0).ok();
    }
    Ok(out)
}

fn read_event_from_commit(repo: &Repository, commit: &git2::Commit) -> Option<HistoryEvent> {
    let tree = commit.tree().ok()?;
    let entry = tree.get_name("run.json")?;
    let obj = entry.to_object(repo).ok()?;
    let blob = obj.peel(ObjectType::Blob).ok()?;
    let blob = blob.as_blob()?;
    serde_json::from_slice(blob.content()).ok()
}

/// Discard `refs/versionx/history`. Only used by `versionx state
/// repair` after the user explicitly confirms — we don't want to
/// strand audit data on accident.
pub fn purge(repo_path: &Utf8Path) -> Result<(), WriteError> {
    let repo = crate::write::open(repo_path)?;
    if let Ok(mut r) = repo.find_reference(HISTORY_REF) {
        r.delete()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::Utf8PathBuf;

    fn fresh_repo() -> (tempfile::TempDir, Utf8PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        let repo = Repository::init(root.as_std_path()).unwrap();
        let mut cfg = repo.config().unwrap();
        cfg.set_str("user.name", "Tester").unwrap();
        cfg.set_str("user.email", "t@example.com").unwrap();
        (tmp, root)
    }

    #[test]
    fn append_creates_ref_with_commit() {
        let (_g, root) = fresh_repo();
        let event = HistoryEvent::new("release.apply", "demo-core v1.2.4");
        let sha = append(&root, &event).unwrap();
        assert_eq!(sha.len(), 40);
        let entries = list(&root, 10).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].summary, "demo-core v1.2.4");
    }

    #[test]
    fn append_chains_parents() {
        let (_g, root) = fresh_repo();
        append(&root, &HistoryEvent::new("a", "first")).unwrap();
        append(&root, &HistoryEvent::new("b", "second")).unwrap();
        let entries = list(&root, 10).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].summary, "second");
        assert_eq!(entries[1].summary, "first");
    }

    #[test]
    fn purge_removes_history() {
        let (_g, root) = fresh_repo();
        append(&root, &HistoryEvent::new("x", "y")).unwrap();
        purge(&root).unwrap();
        assert!(list(&root, 10).unwrap().is_empty());
    }
}
