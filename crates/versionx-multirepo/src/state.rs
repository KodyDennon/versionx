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
    #[error("state DB error: {0}")]
    State(#[from] versionx_state::StateError),
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

/// Outcome of a `repair` that also rebuilt the on-disk state DB.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RepairReport {
    /// Path of the rebuilt sqlite file.
    pub state_db_path: String,
    /// Number of `release.apply` events replayed into `runs`.
    pub release_runs: usize,
    /// Number of `state.backup` events seen.
    pub backups: usize,
    /// Total events scanned.
    pub events_scanned: usize,
}

/// Rebuild the on-disk `state.db` from the history ref. Idempotent —
/// calling twice produces the same DB content modulo timestamps.
///
/// Strategy:
///   1. Open / create the state DB at `db_path`.
///   2. Upsert a row in `repos` for `workspace_root` so foreign keys
///      attach.
///   3. Walk `refs/versionx/history` newest-first; for each known
///      event kind insert a corresponding row.
///
/// Skips unknown event kinds — repair must not fail on a future
/// schema's events written by a newer Versionx.
pub fn repair_state_db(
    repo: &Utf8Path,
    workspace_root: &Utf8Path,
    db_path: &Utf8Path,
    max_events: usize,
) -> StateResult<RepairReport> {
    use chrono::DateTime;
    use versionx_state::RunOutcome;

    let events = versionx_git::history::list(repo, max_events)?;
    let state = versionx_state::open(db_path).map_err(StateError::State)?;
    let repo_row = state.upsert_repo(workspace_root, None).map_err(StateError::State)?;

    let mut release_runs = 0_usize;
    let mut backups = 0_usize;
    for event in &events {
        match event.kind.as_str() {
            "release.apply" => {
                let plan_id = event.details.get("plan_id").and_then(|v| v.as_str());
                let started_at: DateTime<chrono::Utc> = event.at;
                state
                    .replay_run(
                        Some(repo_row.id),
                        "release apply",
                        started_at,
                        RunOutcome::Success,
                        plan_id,
                        None,
                    )
                    .map_err(StateError::State)?;
                release_runs += 1;
            }
            "state.backup" => {
                backups += 1;
            }
            _ => {}
        }
    }

    Ok(RepairReport {
        state_db_path: db_path.to_string(),
        release_runs,
        backups,
        events_scanned: events.len(),
    })
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

    #[test]
    fn repair_state_db_replays_release_runs() {
        let (_g, root) = init_repo();
        record_release_apply(&root, "blake3:first", "f".repeat(40).as_str(), vec![]).unwrap();
        record_release_apply(&root, "blake3:second", "s".repeat(40).as_str(), vec![]).unwrap();
        let db_path = root.join(".versionx/state.db");
        let report = repair_state_db(&root, &root, &db_path, 100).unwrap();
        assert_eq!(report.release_runs, 2);
        assert_eq!(report.events_scanned, 2);

        let state = versionx_state::open(&db_path).unwrap();
        let runs = state.recent_runs(10).unwrap();
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].command, "release apply");
    }
}
