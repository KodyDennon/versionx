//! Structured errors for `versionx-core`.

use thiserror::Error;

/// Result alias used throughout core.
pub type CoreResult<T> = Result<T, CoreError>;

/// Every fallible operation in `versionx-core` returns one of these variants.
#[derive(Debug, Error)]
pub enum CoreError {
    /// The user tried to run a mutating command without a config present
    /// and without opting into zero-config detection.
    #[error("no versionx.toml found at {path} (hint: run `versionx init` first)")]
    NoConfig { path: String },

    /// A `versionx.toml` already exists and `--force` was not passed.
    #[error("versionx.toml already exists at {path} (pass --force to overwrite)")]
    ConfigAlreadyExists { path: String },

    /// No ecosystem signals detected — nothing to synthesize.
    #[error(
        "no ecosystems detected at {path}. Create a package.json, Cargo.toml, pyproject.toml, \
         or .tool-versions file first, or run `versionx init --template <kind>`."
    )]
    NoEcosystemsDetected { path: String },

    /// User asked for a runtime we don't have an installer for.
    #[error("no installer for runtime `{0}` (try `node`, `python`, `rust`)")]
    UnknownRuntime(String),

    /// User didn't pin a runtime this command needs.
    #[error("no pinned version for `{tool}` — set `[runtimes] {tool} = \"...\"` in versionx.toml")]
    RuntimeNotPinned { tool: String },

    /// Underlying config load/parse failure.
    #[error(transparent)]
    Config(#[from] versionx_config::ConfigError),

    /// Underlying runtime install failure.
    #[error(transparent)]
    Installer(#[from] versionx_runtime_trait::InstallerError),

    /// State DB failure.
    #[error(transparent)]
    State(#[from] versionx_state::StateError),

    /// Lockfile failure.
    #[error(transparent)]
    Lockfile(#[from] versionx_lockfile::LockfileError),

    /// Path resolution failure.
    #[error(transparent)]
    Paths(#[from] crate::paths::PathDetectionError),

    /// I/O failure with path context.
    #[error("i/o error at {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },

    /// Serialization failure writing back the config (`toml_edit`).
    #[error("failed to serialize versionx.toml: {0}")]
    Serialize(String),
}
