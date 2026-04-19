//! The `PackageManagerAdapter` trait + shared types every adapter uses.
//!
//! Each ecosystem crate (`versionx-adapter-node`, `-python`, `-rust`, …)
//! implements this trait. `versionx-core` holds a registry and picks the
//! right adapter based on [`Ecosystem`] / detected signals.
//!
//! See `docs/spec/03-ecosystem-adapters.md §2`.

#![deny(unsafe_code)]

use std::path::PathBuf;

use async_trait::async_trait;
use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use versionx_events::EventSender;

/// Which ecosystem an adapter covers.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Ecosystem {
    Node,
    Python,
    Rust,
    Go,
    Ruby,
    Jvm,
    Oci,
}

impl Ecosystem {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Node => "node",
            Self::Python => "python",
            Self::Rust => "rust",
            Self::Go => "go",
            Self::Ruby => "ruby",
            Self::Jvm => "jvm",
            Self::Oci => "oci",
        }
    }
}

/// Context handed to every adapter method.
#[derive(Clone)]
pub struct AdapterContext {
    /// Working directory — where the ecosystem's manifest lives.
    pub cwd: Utf8PathBuf,
    /// Path to the runtime's binary directory (so adapters spawn the right
    /// `node` / `python` / `cargo` without relying on the user's PATH).
    /// None means "fall back to PATH" — some adapters (e.g. cargo) may be
    /// fine with that during development.
    pub runtime_bin_dir: Option<Utf8PathBuf>,
    /// Event bus for streaming subprocess output + progress.
    pub events: EventSender,
    /// Additional env vars layered on top of the parent process env.
    pub env: Vec<(String, String)>,
    /// Dry-run mode: no subprocesses, just planning.
    pub dry_run: bool,
}

impl std::fmt::Debug for AdapterContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AdapterContext")
            .field("cwd", &self.cwd)
            .field("runtime_bin_dir", &self.runtime_bin_dir)
            .field("env", &self.env)
            .field("dry_run", &self.dry_run)
            .finish_non_exhaustive()
    }
}

/// Result of [`PackageManagerAdapter::detect`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DetectResult {
    /// Is this adapter applicable at all in `cwd`?
    pub applicable: bool,
    /// Detection reason: `"pnpm-lock.yaml"`, `"packageManager:pnpm@8.15.0"`, etc.
    pub reason: Option<String>,
    /// The package manager id the adapter will use (`"pnpm"`, `"npm"`, `"yarn"`).
    pub package_manager: Option<String>,
    /// Absolute path of the manifest file (`package.json` etc.).
    pub manifest_path: Option<Utf8PathBuf>,
}

/// Plan a single step would take. Adapters compute plans from intents and
/// execute them step-by-step.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlanStep {
    /// Stable id — a hash of the step, used for idempotency.
    pub id: String,
    /// `"install"`, `"prune"`, `"upgrade"`, etc.
    pub action: String,
    /// Human-readable preview (`"pnpm install --frozen-lockfile"`).
    pub command_preview: String,
    /// `true` if running this step will change the lockfile on disk.
    pub affects_lockfile: bool,
}

/// A complete plan for a given intent.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Plan {
    pub steps: Vec<PlanStep>,
    pub summary: String,
    pub warnings: Vec<String>,
}

/// Intent the caller wants the adapter to satisfy.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Intent {
    /// Install to match manifest + lockfile (no new deps).
    Sync,
    /// Add a new dependency.
    Install { spec: String, dev: bool },
    /// Remove a dependency.
    Remove { name: String },
    /// Update existing deps within their declared ranges.
    Upgrade { spec: Option<String> },
    /// Regenerate the lockfile without touching installed files.
    LockOnly,
}

/// Execution outcome of a plan step.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StepOutcome {
    pub step_id: String,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    pub stdout_tail: String,
    pub stderr_tail: String,
}

#[derive(Debug, Error)]
pub enum AdapterError {
    #[error("{tool} is not installed at the expected location: {path}")]
    ToolMissing { tool: String, path: Utf8PathBuf },

    #[error("manifest not found at {path}")]
    ManifestNotFound { path: Utf8PathBuf },

    #[error("parsing manifest at {path}: {message}")]
    ManifestParse { path: Utf8PathBuf, message: String },

    #[error("subprocess `{program}` failed with exit {status}:\n{stderr}")]
    Subprocess { program: String, status: i32, stderr: String },

    #[error("i/o error at {path}: {source}")]
    Io {
        path: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("ambiguous package manager — multiple lockfiles detected: {found:?}")]
    AmbiguousPackageManager { found: Vec<String> },

    #[error("unsupported intent for this adapter: {0}")]
    UnsupportedIntent(String),

    #[error("{0}")]
    Other(String),
}

pub type AdapterResult<T> = Result<T, AdapterError>;

/// Every ecosystem adapter implements this trait.
#[async_trait]
pub trait PackageManagerAdapter: Send + Sync + std::fmt::Debug {
    /// Stable id (`"node"`, `"python"`, `"rust"`).
    fn id(&self) -> &'static str;

    /// Which ecosystem this adapter covers.
    fn ecosystem(&self) -> Ecosystem;

    /// Detect whether this adapter should handle `cwd`. Pure — no side effects.
    async fn detect(&self, ctx: &AdapterContext) -> AdapterResult<DetectResult>;

    /// Compute a plan for `intent`, never executing.
    async fn plan(&self, ctx: &AdapterContext, intent: &Intent) -> AdapterResult<Plan>;

    /// Execute a plan step. Streams events on the bus.
    async fn execute(
        &self,
        ctx: &AdapterContext,
        step: &PlanStep,
        intent: &Intent,
    ) -> AdapterResult<StepOutcome>;
}

/// Locate a binary under `<runtime_bin_dir>/<name>`, falling back to PATH.
#[must_use]
pub fn resolve_binary(runtime_bin_dir: Option<&Utf8Path>, name: &str) -> PathBuf {
    if let Some(dir) = runtime_bin_dir {
        let candidate = dir.join(name);
        if candidate.exists() {
            return candidate.into();
        }
        #[cfg(windows)]
        {
            let with_ext = dir.join(format!("{name}.exe"));
            if with_ext.exists() {
                return with_ext.into();
            }
        }
    }
    PathBuf::from(name)
}

/// Crate version as declared in `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_matches_cargo() {
        assert!(!VERSION.is_empty());
    }

    #[test]
    fn ecosystem_stringifies() {
        assert_eq!(Ecosystem::Node.as_str(), "node");
        assert_eq!(Ecosystem::Python.as_str(), "python");
    }
}
