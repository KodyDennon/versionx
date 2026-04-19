//! Schema migrations. Applied on open by [`crate::store::open`].
//!
//! New migrations go at the end of [`MIGRATIONS`]. Never edit an existing one
//! after it's shipped — write a new migration that alters.

use rusqlite_migration::{M, Migrations};

/// Ordered list of migrations. Index + 1 = `schema_migrations.version`.
pub(crate) fn migrations() -> Migrations<'static> {
    Migrations::new(vec![
        M::up(V1_INITIAL),
        M::up(V2_POLICY_EVALUATIONS),
        M::up(V3_CHANGELOG_VOICE),
    ])
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

// 0.5 — durable history of policy evaluations so `policy stats` can
// answer "what fired this week" without re-running every policy
// across every commit.
const V2_POLICY_EVALUATIONS: &str = r"
CREATE TABLE policy_evaluations (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    repo_id         INTEGER REFERENCES repos(id) ON DELETE SET NULL,
    evaluated_at    TEXT    NOT NULL,
    trigger         TEXT,
    policy_name     TEXT    NOT NULL,
    severity        TEXT    NOT NULL,
    waived          INTEGER NOT NULL DEFAULT 0,
    finding_json    TEXT    NOT NULL,
    plan_id         TEXT,
    git_sha         TEXT
);
CREATE INDEX idx_policy_eval_repo_time
    ON policy_evaluations(repo_id, evaluated_at DESC);
CREATE INDEX idx_policy_eval_policy
    ON policy_evaluations(policy_name);
";

// 0.6 — per-component changelog voice/style memory so the
// AI-as-client changelog generator can match the project's
// established cadence (terse vs narrative, emoji vs plain, etc.).
const V3_CHANGELOG_VOICE: &str = r"
CREATE TABLE changelog_voice (
    repo_id         INTEGER NOT NULL REFERENCES repos(id) ON DELETE CASCADE,
    component_id    TEXT    NOT NULL,
    voice           TEXT    NOT NULL,
    samples_json    TEXT,
    updated_at      TEXT    NOT NULL,
    PRIMARY KEY (repo_id, component_id)
);
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_validate() {
        migrations().validate().unwrap();
    }
}
