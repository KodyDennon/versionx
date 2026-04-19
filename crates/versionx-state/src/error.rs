//! Structured errors for `versionx-state`.

use camino::Utf8PathBuf;
use thiserror::Error;

pub type StateResult<T> = Result<T, StateError>;

#[derive(Debug, Error)]
pub enum StateError {
    #[error("opening state DB at {path}: {source}")]
    Open {
        path: Utf8PathBuf,
        #[source]
        source: rusqlite::Error,
    },

    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("migration error: {0}")]
    Migration(#[from] rusqlite_migration::Error),

    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),

    #[error("deserializing row column `{column}`: {message}")]
    Deserialize { column: String, message: String },

    #[error("record not found: {kind} `{id}`")]
    NotFound { kind: &'static str, id: String },
}
