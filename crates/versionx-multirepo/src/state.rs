//! `versionx state backup|restore|repair` — state-DB recovery via
//! git-backed `refs/versionx/history`.
//!
//! Philosophy: the `state.db` sqlite file is a hot cache, not the
//! source of truth. When it's lost, stale, or corrupted, we
//! reconstruct by replaying the history events the release pipeline
//! wrote into `refs/versionx/history`.
//!
//! `backup` — writes a snapshot event (`state.backup`) with a manifest
//! of the relevant state-DB tables. Caller separately copies the db
//! file; this hook just timestamps the backup.
//!
//! `restore` — walks the history ref, finds the last `state.backup`,
//! and surfaces its manifest so the caller can restore accordingly.
//!
//! `repair` — read-only; walks every history entry, rebuilds a
//! consolidated view + returns it. The shell of the recovery pipeline;
//! actual sqlite writes live in `versionx-state` (deferred to 0.8).

use camino::Utf8Path;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub use versionx_git::history::HISTORY_REF;
use versionx_git::history::HistoryEvent;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BackupManifest {
    pub at: DateTime<Utc>,
    pub workspace_root: String,
    /// Free-form tag so multiple backups can be distinguished.
    pub label: Option<String>,
    /// Name of the sqlite file the caller should copy separately.
    pub state_db_path: String,
}

#[derive(Debug, thiserror::Error)]
pub enum StateError {
    #[error("git error: {0}")]
    Git(#[from] versionx_git::WriteError),
    #[error("no backup found in history")]
    NoBackup,
}

pub type StateResult<T> = Result<T, StateError>;

/// Append a `state.backup` event to the history ref.
pub fn backup(repo: &Utf8Path, manifest: BackupManifest) -> StateResult<String> {
    let details = serde_json::to_value(&manifest).unwrap_or(serde_json::Value::Null);
    let event = HistoryEvent::new(
        "state.backup",
        manifest.label.clone().unwrap_or_else(|| "snapshot".into()),
    )
    .with_details(details);
    Ok(versionx_git::history::append(repo, &event)?)
}

/// Pull the most recent `state.backup` manifest from the history ref.
pub fn restore(repo: &Utf8Path) -> StateResult<BackupManifest> {
    let events = versionx_git::history::list(repo, 1000)?;
    let backup = events.iter().find(|e| e.kind == "state.backup").ok_or(StateError::NoBackup)?;
    serde_json::from_value(backup.details.clone()).map_err(|e| {
        StateError::Git(versionx_git::WriteError::Git(git2::Error::from_str(&format!(
            "manifest parse: {e}"
        ))))
    })
}

/// Walk every history entry newest-first and return them for the
/// caller to inspect / replay.
pub fn repair(repo: &Utf8Path, max: usize) -> StateResult<Vec<HistoryEvent>> {
    Ok(versionx_git::history::list(repo, max)?)
}

/// Convenience helper: record a release apply into the history ref.
/// Callers pass the plan id + commit sha for audit.
pub fn record_release_apply(
    repo: &Utf8Path,
    plan_id: &str,
    commit_sha: &str,
    bumps: Vec<serde_json::Value>,
) -> StateResult<String> {
    let event = HistoryEvent::new("release.apply", format!("plan {plan_id}")).with_details(
        serde_json::json!({
            "plan_id": plan_id,
            "commit": commit_sha,
            "bumps": bumps,
        }),
    );
    Ok(versionx_git::history::append(repo, &event)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::Utf8PathBuf;

    fn init_repo() -> (tempfile::TempDir, Utf8PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        let repo = git2::Repository::init(root.as_std_path()).unwrap();
        let mut cfg = repo.config().unwrap();
        cfg.set_str("user.name", "Tester").unwrap();
        cfg.set_str("user.email", "t@e.com").unwrap();
        (tmp, root)
    }

    #[test]
    fn backup_and_restore_round_trip() {
        let (_g, root) = init_repo();
        let manifest = BackupManifest {
            at: Utc::now(),
            workspace_root: root.to_string(),
            label: Some("demo".into()),
            state_db_path: root.join(".versionx/state.db").to_string(),
        };
        backup(&root, manifest.clone()).unwrap();
        let found = restore(&root).unwrap();
        assert_eq!(found.label.as_deref(), Some("demo"));
    }

    #[test]
    fn repair_returns_events_newest_first() {
        let (_g, root) = init_repo();
        record_release_apply(&root, "blake3:first", "f".repeat(40).as_str(), vec![]).unwrap();
        record_release_apply(&root, "blake3:second", "s".repeat(40).as_str(), vec![]).unwrap();
        let events = repair(&root, 10).unwrap();
        assert_eq!(events.len(), 2);
        assert!(events[0].summary.contains("second"));
    }
}
