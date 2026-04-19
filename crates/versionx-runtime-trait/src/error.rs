//! Errors surfaced by runtime installers.

use camino::Utf8PathBuf;
use thiserror::Error;

pub type InstallerResult<T> = Result<T, InstallerError>;

#[derive(Debug, Error)]
pub enum InstallerError {
    #[error("network error fetching {url}: {source}")]
    Network {
        url: String,
        #[source]
        source: reqwest::Error,
    },

    #[error("http {status} fetching {url}")]
    Http { url: String, status: u16 },

    #[error("this platform is not supported for {tool}: {platform}")]
    UnsupportedPlatform { tool: &'static str, platform: String },

    #[error("checksum mismatch for {url}: expected {expected}, got {actual}")]
    ChecksumMismatch { url: String, expected: String, actual: String },

    #[error("i/o error at {path}: {source}")]
    Io {
        path: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("extracting archive at {path}: {message}")]
    Extract { path: Utf8PathBuf, message: String },

    #[error("no version matches `{spec}` for {tool}")]
    UnresolvableVersion { tool: &'static str, spec: String },

    #[error("required external tool `{tool}` is not installed ({hint})")]
    MissingExternalTool { tool: String, hint: String },

    #[error("subprocess `{program}` failed with exit {status}: {stderr}")]
    Subprocess { program: String, status: i32, stderr: String },

    #[error("{0}")]
    Other(String),
}
