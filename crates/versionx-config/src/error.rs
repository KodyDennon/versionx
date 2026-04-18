//! Structured errors for the config crate.

use std::io;

use camino::Utf8PathBuf;
use thiserror::Error;

/// Result alias for the crate.
pub type ConfigResult<T> = Result<T, ConfigError>;

/// Any failure while loading, validating, or detecting a Versionx config.
///
/// Every variant carries enough context for the CLI to produce an actionable
/// error (the `suggestion` lives with the caller — we just provide the kind).
#[derive(Debug, Error)]
pub enum ConfigError {
    /// The requested path does not exist.
    #[error("config file not found at {path}")]
    NotFound { path: Utf8PathBuf },

    /// I/O failure reading or writing a config file.
    #[error("i/o error accessing {path}: {source}")]
    Io {
        path: Utf8PathBuf,
        #[source]
        source: io::Error,
    },

    /// Non-UTF-8 path encountered at a system boundary.
    #[error("path is not valid UTF-8: {path}")]
    PathNotUtf8 { path: String },

    /// The file parsed as TOML but violated the schema.
    #[error("invalid versionx.toml at {path}: {message}")]
    Invalid { path: Utf8PathBuf, message: String },

    /// TOML parse failure. Wraps the underlying toml error.
    #[error("failed to parse versionx.toml at {path}:\n{source}")]
    TomlParse {
        path: Utf8PathBuf,
        #[source]
        source: toml::de::Error,
    },

    /// Env-var interpolation referenced a missing variable (and no default).
    #[error("missing environment variable `{var}` referenced in {path}")]
    MissingEnv { var: String, path: Utf8PathBuf },

    /// Schema version bump required.
    #[error(
        "versionx.toml at {path} uses schema_version {found}, but this versionx binary \
         understands up to {supported}. Run `versionx migrate` to upgrade."
    )]
    SchemaTooNew { path: Utf8PathBuf, found: String, supported: String },
}
