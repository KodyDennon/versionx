//! Schema migrations. Applied on open by [`crate::store::open`].
//!
//! New migrations go at the end of [`MIGRATIONS`]. Never edit an existing one
//! after it's shipped — write a new migration that alters.

use rusqlite_migration::{M, Migrations};

/// Ordered list of migrations. Index + 1 = `schema_migrations.version`.
pub(crate) fn migrations() -> Migrations<'static> {
    Migrations::new(vec![M::up(V1_INITIAL)])
}

const V1_INITIAL: &str = r"
CREATE TABLE repos (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    path         TEXT    NOT NULL UNIQUE,
    name         TEXT,
    remote_url   TEXT,
    github_id    TEXT,
    first_seen   TEXT    NOT NULL,
    last_synced  TEXT,
    config_hash  TEXT
);
CREATE INDEX idx_repos_github ON repos(github_id);

CREATE TABLE runtimes_installed (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    tool          TEXT    NOT NULL,
    version       TEXT    NOT NULL,
    source        TEXT    NOT NULL,
    install_path  TEXT    NOT NULL,
    sha256        TEXT,
    installed_at  TEXT    NOT NULL,
    last_used     TEXT,
    UNIQUE(tool, version, source)
);

CREATE TABLE repo_runtimes (
    repo_id     INTEGER NOT NULL REFERENCES repos(id) ON DELETE CASCADE,
    tool        TEXT    NOT NULL,
    runtime_id  INTEGER NOT NULL REFERENCES runtimes_installed(id),
    PRIMARY KEY (repo_id, tool)
);

CREATE TABLE runs (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    repo_id           INTEGER REFERENCES repos(id) ON DELETE SET NULL,
    command           TEXT    NOT NULL,
    started_at        TEXT    NOT NULL,
    ended_at          TEXT,
    outcome           TEXT,
    exit_code         INTEGER,
    plan_id           TEXT,
    plan_json         TEXT,
    events_zstd       BLOB,
    versionx_version  TEXT    NOT NULL,
    agent_id          TEXT
);
CREATE INDEX idx_runs_repo ON runs(repo_id, started_at DESC);
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_validate() {
        migrations().validate().unwrap();
    }
}
