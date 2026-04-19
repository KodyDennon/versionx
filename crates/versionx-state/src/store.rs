//! The `State` handle: opens the DB, runs migrations, exposes typed CRUD.

use std::fs;
use std::str::FromStr;

use camino::{Utf8Path, Utf8PathBuf};
use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use rusqlite::{Connection, OptionalExtension, params};

use crate::error::{StateError, StateResult};
use crate::migrations::migrations;
use crate::model::{InstalledRuntime, Repo, Run, RunOutcome};

/// Opaque handle to the `SQLite` state database.
///
/// Internally wraps a [`rusqlite::Connection`] behind a `Mutex` — we serialise
/// writes to avoid `SQLite`'s WAL contention surface while still letting `Clone`d
/// handles share a connection. For 0.1.0 one connection per process is enough;
/// a multi-connection pool is only needed once the daemon exists.
pub struct State {
    conn: Mutex<Connection>,
    path: Utf8PathBuf,
}

impl std::fmt::Debug for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("State").field("path", &self.path).finish_non_exhaustive()
    }
}

/// Open (or create) a state DB at `path`. Applies pending migrations.
pub fn open(path: impl AsRef<Utf8Path>) -> StateResult<State> {
    let path = path.as_ref().to_path_buf();
    if let Some(parent) = path.parent()
        && !parent.as_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    let mut conn = Connection::open(&path)
        .map_err(|source| StateError::Open { path: path.clone(), source })?;
    apply_pragmas(&conn)?;
    migrations().to_latest(&mut conn)?;
    Ok(State { conn: Mutex::new(conn), path })
}

/// Open an ephemeral in-memory DB for tests.
pub fn open_in_memory() -> StateResult<State> {
    let mut conn = Connection::open_in_memory()?;
    apply_pragmas(&conn)?;
    migrations().to_latest(&mut conn)?;
    Ok(State { conn: Mutex::new(conn), path: Utf8PathBuf::from(":memory:") })
}

fn apply_pragmas(conn: &Connection) -> StateResult<()> {
    // WAL + NORMAL gives good durability for a desktop cache at minimal perf cost.
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "busy_timeout", 5000_i64)?;
    Ok(())
}

impl State {
    /// Where the DB file lives (`":memory:"` for in-memory tests).
    #[must_use]
    pub fn path(&self) -> &Utf8Path {
        &self.path
    }

    // ------------------------------------------------------------------ repos

    /// Upsert a repo by absolute path. Returns the resulting [`Repo`].
    pub fn upsert_repo(&self, path: &Utf8Path, name: Option<&str>) -> StateResult<Repo> {
        let conn = self.conn.lock();
        let now = Utc::now().to_rfc3339();

        conn.execute(
            "INSERT INTO repos (path, name, first_seen)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(path) DO UPDATE SET name = COALESCE(excluded.name, repos.name)",
            params![path.as_str(), name, now],
        )?;

        fetch_repo_by_path(&conn, path)?
            .ok_or_else(|| StateError::NotFound { kind: "repo", id: path.to_string() })
    }

    /// Look up a repo by absolute path.
    pub fn repo_by_path(&self, path: &Utf8Path) -> StateResult<Option<Repo>> {
        let conn = self.conn.lock();
        fetch_repo_by_path(&conn, path)
    }

    /// Set the `last_synced` + `config_hash` on a repo.
    pub fn mark_repo_synced(&self, repo_id: i64, config_hash: &str) -> StateResult<()> {
        let conn = self.conn.lock();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE repos SET last_synced = ?1, config_hash = ?2 WHERE id = ?3",
            params![now, config_hash, repo_id],
        )?;
        Ok(())
    }

    // ---------------------------------------------------------------- runtimes

    /// Record a completed runtime install. If an entry for
    /// `(tool, version, source)` already exists its `install_path` + `sha256`
    /// are refreshed; `installed_at` is preserved.
    pub fn record_runtime(
        &self,
        tool: &str,
        version: &str,
        source: &str,
        install_path: &Utf8Path,
        sha256: Option<&str>,
    ) -> StateResult<InstalledRuntime> {
        let conn = self.conn.lock();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO runtimes_installed
                 (tool, version, source, install_path, sha256, installed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(tool, version, source) DO UPDATE SET
                 install_path = excluded.install_path,
                 sha256 = COALESCE(excluded.sha256, runtimes_installed.sha256)",
            params![tool, version, source, install_path.as_str(), sha256, now],
        )?;

        fetch_runtime(&conn, tool, version, source)?.ok_or_else(|| StateError::NotFound {
            kind: "runtime",
            id: format!("{tool}@{version}"),
        })
    }

    /// Look up an installation by its composite natural key.
    pub fn runtime(
        &self,
        tool: &str,
        version: &str,
        source: &str,
    ) -> StateResult<Option<InstalledRuntime>> {
        let conn = self.conn.lock();
        fetch_runtime(&conn, tool, version, source)
    }

    /// List every installed runtime ordered by tool then version.
    pub fn list_runtimes(&self) -> StateResult<Vec<InstalledRuntime>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, tool, version, source, install_path, sha256, installed_at, last_used
             FROM runtimes_installed
             ORDER BY tool, version",
        )?;
        let rows =
            stmt.query_map([], installed_runtime_from_row)?.collect::<Result<Vec<_>, _>>()?;
        rows.into_iter().collect()
    }

    /// Update `last_used` for a runtime (called by the shim, once implemented).
    pub fn touch_runtime(&self, runtime_id: i64) -> StateResult<()> {
        let conn = self.conn.lock();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE runtimes_installed SET last_used = ?1 WHERE id = ?2",
            params![now, runtime_id],
        )?;
        Ok(())
    }

    /// Pin `tool` on `repo_id` to a specific installed runtime.
    pub fn set_repo_runtime(&self, repo_id: i64, tool: &str, runtime_id: i64) -> StateResult<()> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO repo_runtimes (repo_id, tool, runtime_id)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(repo_id, tool) DO UPDATE SET runtime_id = excluded.runtime_id",
            params![repo_id, tool, runtime_id],
        )?;
        Ok(())
    }

    // -------------------------------------------------------------------- runs

    /// Start a new run. Returns the row id.
    pub fn start_run(&self, repo_id: Option<i64>, command: &str) -> StateResult<i64> {
        let conn = self.conn.lock();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO runs (repo_id, command, started_at, versionx_version)
             VALUES (?1, ?2, ?3, ?4)",
            params![repo_id, command, now, env!("CARGO_PKG_VERSION")],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Complete a run with an outcome + exit code.
    pub fn finish_run(
        &self,
        run_id: i64,
        outcome: RunOutcome,
        exit_code: Option<i32>,
    ) -> StateResult<()> {
        let conn = self.conn.lock();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE runs SET ended_at = ?1, outcome = ?2, exit_code = ?3 WHERE id = ?4",
            params![now, outcome.as_str(), exit_code, run_id],
        )?;
        Ok(())
    }

    /// Return the most recent `limit` runs, newest first.
    pub fn recent_runs(&self, limit: u32) -> StateResult<Vec<Run>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, repo_id, command, started_at, ended_at, outcome, exit_code,
                    plan_id, versionx_version, agent_id
             FROM runs
             ORDER BY started_at DESC
             LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit], run_from_row)?.collect::<Result<Vec<_>, _>>()?;
        rows.into_iter().collect()
    }
}

// --- private helpers -------------------------------------------------------

fn fetch_repo_by_path(conn: &Connection, path: &Utf8Path) -> StateResult<Option<Repo>> {
    conn.query_row(
        "SELECT id, path, name, remote_url, github_id, first_seen, last_synced, config_hash
         FROM repos WHERE path = ?1",
        params![path.as_str()],
        repo_from_row,
    )
    .optional()?
    .transpose()
}

fn fetch_runtime(
    conn: &Connection,
    tool: &str,
    version: &str,
    source: &str,
) -> StateResult<Option<InstalledRuntime>> {
    conn.query_row(
        "SELECT id, tool, version, source, install_path, sha256, installed_at, last_used
         FROM runtimes_installed
         WHERE tool = ?1 AND version = ?2 AND source = ?3",
        params![tool, version, source],
        installed_runtime_from_row,
    )
    .optional()?
    .transpose()
}

fn repo_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Result<Repo, StateError>> {
    let path_s: String = row.get(1)?;
    let first_seen_s: String = row.get(5)?;
    let last_synced_s: Option<String> = row.get(6)?;

    let repo = (|| -> Result<_, StateError> {
        Ok(Repo {
            id: row.get(0)?,
            path: Utf8PathBuf::from(path_s),
            name: row.get(2)?,
            remote_url: row.get(3)?,
            github_id: row.get(4)?,
            first_seen: parse_dt(&first_seen_s, "first_seen")?,
            last_synced: last_synced_s
                .as_deref()
                .map(|s| parse_dt(s, "last_synced"))
                .transpose()?,
            config_hash: row.get(7)?,
        })
    })();
    Ok(repo)
}

fn installed_runtime_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<Result<InstalledRuntime, StateError>> {
    let install_path_s: String = row.get(4)?;
    let installed_at_s: String = row.get(6)?;
    let last_used_s: Option<String> = row.get(7)?;

    let runtime = (|| -> Result<_, StateError> {
        Ok(InstalledRuntime {
            id: row.get(0)?,
            tool: row.get(1)?,
            version: row.get(2)?,
            source: row.get(3)?,
            install_path: Utf8PathBuf::from(install_path_s),
            sha256: row.get(5)?,
            installed_at: parse_dt(&installed_at_s, "installed_at")?,
            last_used: last_used_s.as_deref().map(|s| parse_dt(s, "last_used")).transpose()?,
        })
    })();
    Ok(runtime)
}

fn run_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Result<Run, StateError>> {
    let started_s: String = row.get(3)?;
    let ended_s: Option<String> = row.get(4)?;
    let outcome_s: Option<String> = row.get(5)?;

    let run = (|| -> Result<_, StateError> {
        Ok(Run {
            id: row.get(0)?,
            repo_id: row.get(1)?,
            command: row.get(2)?,
            started_at: parse_dt(&started_s, "started_at")?,
            ended_at: ended_s.as_deref().map(|s| parse_dt(s, "ended_at")).transpose()?,
            outcome: outcome_s.as_deref().and_then(RunOutcome::parse),
            exit_code: row.get(6)?,
            plan_id: row.get(7)?,
            versionx_version: row.get(8)?,
            agent_id: row.get(9)?,
        })
    })();
    Ok(run)
}

fn parse_dt(s: &str, column: &str) -> StateResult<DateTime<Utc>> {
    DateTime::<Utc>::from_str(s)
        .or_else(|_| DateTime::parse_from_rfc3339(s).map(|dt| dt.with_timezone(&Utc)))
        .map_err(|e| StateError::Deserialize { column: column.to_string(), message: e.to_string() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_in_memory_works() {
        let state = open_in_memory().unwrap();
        assert_eq!(state.path().as_str(), ":memory:");
    }

    #[test]
    fn upsert_repo_then_lookup() {
        let state = open_in_memory().unwrap();
        let p = Utf8Path::new("/repo");
        let r1 = state.upsert_repo(p, Some("demo")).unwrap();
        assert_eq!(r1.path, p);
        assert_eq!(r1.name.as_deref(), Some("demo"));

        // Upserting again should keep id + first_seen stable.
        let r2 = state.upsert_repo(p, Some("demo")).unwrap();
        assert_eq!(r1.id, r2.id);
        assert_eq!(r1.first_seen, r2.first_seen);
    }

    #[test]
    fn mark_repo_synced_updates_fields() {
        let state = open_in_memory().unwrap();
        let r = state.upsert_repo(Utf8Path::new("/repo"), None).unwrap();
        state.mark_repo_synced(r.id, "blake3:abc").unwrap();
        let reloaded = state.repo_by_path(Utf8Path::new("/repo")).unwrap().unwrap();
        assert_eq!(reloaded.config_hash.as_deref(), Some("blake3:abc"));
        assert!(reloaded.last_synced.is_some());
    }

    #[test]
    fn record_runtime_upserts_by_key() {
        let state = open_in_memory().unwrap();
        let first = state
            .record_runtime(
                "node",
                "20.11.1",
                "nodejs.org",
                Utf8Path::new("/rt/node/20.11.1"),
                Some("deadbeef"),
            )
            .unwrap();
        let again = state
            .record_runtime(
                "node",
                "20.11.1",
                "nodejs.org",
                Utf8Path::new("/rt/node/20.11.1-moved"),
                Some("deadbeef"),
            )
            .unwrap();
        assert_eq!(first.id, again.id);
        assert_eq!(again.install_path.as_str(), "/rt/node/20.11.1-moved");
    }

    #[test]
    fn list_runtimes_is_ordered() {
        let state = open_in_memory().unwrap();
        state.record_runtime("python", "3.12.2", "pbs", Utf8Path::new("/a"), None).unwrap();
        state.record_runtime("node", "20.11.1", "nodejs.org", Utf8Path::new("/b"), None).unwrap();
        state.record_runtime("node", "18.19.0", "nodejs.org", Utf8Path::new("/c"), None).unwrap();
        let all = state.list_runtimes().unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].tool, "node");
        assert_eq!(all[0].version, "18.19.0");
        assert_eq!(all[1].version, "20.11.1");
        assert_eq!(all[2].tool, "python");
    }

    #[test]
    fn repo_runtime_pin_roundtrip() {
        let state = open_in_memory().unwrap();
        let repo = state.upsert_repo(Utf8Path::new("/repo"), None).unwrap();
        let rt =
            state.record_runtime("node", "20", "nodejs.org", Utf8Path::new("/rt"), None).unwrap();
        state.set_repo_runtime(repo.id, "node", rt.id).unwrap();

        // Reassigning to a different runtime should update, not duplicate.
        let rt2 =
            state.record_runtime("node", "22", "nodejs.org", Utf8Path::new("/rt2"), None).unwrap();
        state.set_repo_runtime(repo.id, "node", rt2.id).unwrap();
    }

    #[test]
    fn run_lifecycle() {
        let state = open_in_memory().unwrap();
        let run_id = state.start_run(None, "sync").unwrap();
        state.finish_run(run_id, RunOutcome::Success, Some(0)).unwrap();
        let recent = state.recent_runs(5).unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].command, "sync");
        assert_eq!(recent[0].outcome, Some(RunOutcome::Success));
        assert_eq!(recent[0].exit_code, Some(0));
    }

    #[test]
    fn open_on_disk_persists() {
        let tmp = tempfile::tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(tmp.path().join("state.db")).unwrap();
        {
            let s = open(&path).unwrap();
            s.upsert_repo(Utf8Path::new("/persisted"), Some("demo")).unwrap();
        }
        let s = open(&path).unwrap();
        assert!(s.repo_by_path(Utf8Path::new("/persisted")).unwrap().is_some());
    }
}
