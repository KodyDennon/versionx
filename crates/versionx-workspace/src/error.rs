//! Errors returned by the workspace crate.

use camino::Utf8PathBuf;
use thiserror::Error;

pub type WorkspaceResult<T> = Result<T, WorkspaceError>;

#[derive(Debug, Error)]
pub enum WorkspaceError {
    #[error("workspace root does not exist: {path}")]
    RootMissing { path: Utf8PathBuf },

    #[error("i/o error at {path}: {source}")]
    Io {
        path: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("reading manifest at {path}: {message}")]
    ManifestParse { path: Utf8PathBuf, message: String },

    #[error("invalid versionx.toml component entry `{id}`: {message}")]
    InvalidComponent { id: String, message: String },

    #[error("component `{id}` references unknown dependency `{dep}`")]
    UnknownDep { id: String, dep: String },

    #[error("dependency cycle detected: {path:?}")]
    Cycle { path: Vec<String> },

    #[error("underlying config error: {0}")]
    Config(#[from] versionx_config::ConfigError),
}
