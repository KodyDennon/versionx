//! Canonical filesystem layout — XDG-compliant, with `$VERSIONX_HOME` override.
//!
//! See `docs/spec/01-architecture-overview.md §6` for the layout table.

use camino::{Utf8Path, Utf8PathBuf};
use directories::BaseDirs;

/// Resolved per-user filesystem paths.
#[derive(Clone, Debug)]
pub struct VersionxHome {
    pub data: Utf8PathBuf,
    pub cache: Utf8PathBuf,
    pub config: Utf8PathBuf,
    pub state: Utf8PathBuf,
    pub runtime: Utf8PathBuf,
}

impl VersionxHome {
    /// Discover the user's paths. Honors `VERSIONX_HOME` (single-dir layout
    /// for users who prefer the old `~/.vers`-style layout) before falling
    /// back to XDG.
    ///
    /// # Errors
    /// Returns an error if no home directory is discoverable or paths aren't UTF-8.
    pub fn detect() -> Result<Self, PathDetectionError> {
        if let Ok(explicit) = std::env::var("VERSIONX_HOME") {
            // Single-dir layout: `runtimes/`, `cache/`, `state.db`, `shims/`,
            // `global.toml` all live directly under `$VERSIONX_HOME`. This
            // matches the shim's simpler resolution logic and matches the
            // `~/.vers`-era convention for users who prefer one dir.
            let base = Utf8PathBuf::from(explicit);
            return Ok(Self {
                data: base.clone(),
                cache: base.join("cache"),
                config: base.clone(),
                state: base.clone(),
                runtime: base,
            });
        }

        let dirs = BaseDirs::new().ok_or(PathDetectionError::NoHomeDir)?;

        let data = pathbuf_from(dirs.data_dir())?.join("versionx");
        let cache = pathbuf_from(dirs.cache_dir())?.join("versionx");
        let config = pathbuf_from(dirs.config_dir())?.join("versionx");
        let state =
            pathbuf_from(dirs.state_dir().unwrap_or_else(|| dirs.data_dir()))?.join("versionx");
        let runtime =
            pathbuf_from(dirs.runtime_dir().unwrap_or_else(|| dirs.cache_dir()))?.join("versionx");

        Ok(Self { data, cache, config, state, runtime })
    }

    /// Directory holding installed toolchains (`<data>/runtimes/`).
    #[must_use]
    pub fn runtimes_dir(&self) -> Utf8PathBuf {
        self.data.join("runtimes")
    }

    /// Directory holding generated shims (`<data>/shims/`).
    #[must_use]
    pub fn shims_dir(&self) -> Utf8PathBuf {
        self.data.join("shims")
    }

    /// Cache root for downloads + index snapshots (`<cache>/cache/`).
    #[must_use]
    pub fn cache_dir(&self) -> &Utf8Path {
        &self.cache
    }

    /// The `SQLite` state DB path (`<data>/state.db`).
    #[must_use]
    pub fn state_db_path(&self) -> Utf8PathBuf {
        self.data.join("state.db")
    }

    /// User-global default config (`<config>/global.toml`).
    #[must_use]
    pub fn global_config(&self) -> Utf8PathBuf {
        self.config.join("global.toml")
    }

    /// Create every directory this layout expects. Idempotent.
    /// First-run bootstrap calls this so subsequent commands don't
    /// need to re-check.
    pub fn ensure_dirs(&self) -> std::io::Result<()> {
        for dir in [
            &self.data,
            &self.cache,
            &self.config,
            &self.state,
            &self.runtime,
            &self.runtimes_dir(),
            &self.shims_dir(),
        ] {
            std::fs::create_dir_all(dir.as_std_path())?;
        }
        Ok(())
    }
}

fn pathbuf_from(p: &std::path::Path) -> Result<Utf8PathBuf, PathDetectionError> {
    Utf8PathBuf::from_path_buf(p.to_path_buf())
        .map_err(|p| PathDetectionError::NotUtf8(p.display().to_string()))
}

#[derive(Debug, thiserror::Error)]
pub enum PathDetectionError {
    #[error("no home directory found")]
    NoHomeDir,
    #[error("path is not valid UTF-8: {0}")]
    NotUtf8(String),
}
