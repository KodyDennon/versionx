//! Structured errors for `versionx-core`.

use thiserror::Error;

/// Result alias used throughout core.
pub type CoreResult<T> = Result<T, CoreError>;

/// Every fallible operation in `versionx-core` returns one of these variants.
///
/// Deliberately coarse: the CLI / MCP / daemon all render `CoreError`
/// uniformly, so variants match the user-visible failure modes, not the
/// low-level subsystem source.
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

    /// Underlying config load/parse failure.
    #[error(transparent)]
    Config(#[from] versionx_config::ConfigError),

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
