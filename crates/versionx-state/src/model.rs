//! Row-shaped types used across `versionx-state`.
//!
//! Kept separate from SQL-touching code so callers can construct and pattern-match
//! without touching `rusqlite` directly.

use camino::Utf8PathBuf;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A repo Versionx has ever touched on this machine.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Repo {
    pub id: i64,
    pub path: Utf8PathBuf,
    pub name: Option<String>,
    pub remote_url: Option<String>,
    pub github_id: Option<String>,
    pub first_seen: DateTime<Utc>,
    pub last_synced: Option<DateTime<Utc>>,
    pub config_hash: Option<String>,
}

/// A single toolchain installation under `$XDG_DATA_HOME/versionx/runtimes/`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InstalledRuntime {
    pub id: i64,
    /// `"node"`, `"python"`, `"rust"`, `"pnpm"`, ...
    pub tool: String,
    /// Exact resolved version (`"20.11.1"`, `"3.12.2"`).
    pub version: String,
    /// Source label (`"nodejs.org"`, `"python-build-standalone"`, `"rustup"`).
    pub source: String,
    /// Absolute install directory.
    pub install_path: Utf8PathBuf,
    /// Verified SHA-256 of the downloaded archive, if available.
    pub sha256: Option<String>,
    pub installed_at: DateTime<Utc>,
    pub last_used: Option<DateTime<Utc>>,
}

/// Recorded outcome of a user-facing command (`sync`, `install`, `release`, ...).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Run {
    pub id: i64,
    pub repo_id: Option<i64>,
    pub command: String,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub outcome: Option<RunOutcome>,
    pub exit_code: Option<i32>,
    pub plan_id: Option<String>,
    pub versionx_version: String,
    pub agent_id: Option<String>,
}

/// Coarse result for `runs.outcome`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunOutcome {
    Success,
    Failure,
    Cancelled,
}

impl RunOutcome {
    /// Render as the string form stored in `SQLite`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Failure => "failure",
            Self::Cancelled => "cancelled",
        }
    }

    /// Parse back from a `SQLite` TEXT column value.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "success" => Some(Self::Success),
            "failure" => Some(Self::Failure),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }
}
