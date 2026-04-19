//! `versionx.lock` serializer + deserializer.
//!
//! The lockfile is a meta-lockfile (`docs/spec/02-config-and-state-model.md §3`):
//! it records resolved runtime versions, hashes of each ecosystem's native
//! lockfile, and a content hash of the effective merged config. It does
//! **not** replace native lockfiles like `pnpm-lock.yaml` or `uv.lock`.
//!
//! Hash policy: BLAKE3 for internal / fast keys, SHA-256 for anything that
//! crosses into supply-chain tools (SBOMs, sigstore).

#![deny(unsafe_code)]

use std::fs;

use camino::{Utf8Path, Utf8PathBuf};
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Maximum lockfile schema version this binary writes + fully validates.
pub const SUPPORTED_SCHEMA_VERSION: &str = "1";

/// A complete `versionx.lock` document.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lockfile {
    pub schema_version: String,
    pub generated_at: DateTime<Utc>,
    pub versionx_version: String,
    pub config_hash: String,

    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub runtimes: IndexMap<String, LockedRuntime>,

    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub ecosystems: IndexMap<String, LockedEcosystem>,

    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub links: IndexMap<String, LockedLink>,

    /// Per-component release baselines. Populated by `versionx release apply`
    /// after a successful release so subsequent `versionx bump` calls can
    /// compare against the last-released hash + version instead of treating
    /// every component as dirty.
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub components: IndexMap<String, LockedComponent>,
}

/// One resolved runtime (`[runtimes.node]` in the lockfile).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LockedRuntime {
    pub version: String,
    pub source: String,
    /// SHA-256 of the downloaded installer/tarball. Present when the runtime
    /// was installed with checksum verification. Absent for trait-wrapped
    /// runtimes like rustup where we don't own the download.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    /// Absolute path on this machine — informational only; never relied on
    /// for verification (machines differ).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub install_path: Option<Utf8PathBuf>,
}

/// One resolved ecosystem (`[ecosystems.node]`).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LockedEcosystem {
    pub package_manager: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_lockfile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_lockfile_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_at: Option<DateTime<Utc>>,
}

/// One resolved link (`[links.shared-ui]`).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LockedLink {
    #[serde(rename = "type")]
    pub kind: String,
    pub commit: String,
    pub resolved_url: String,
}

/// Per-component release baseline (`[components.<id>]`).
///
/// Updated only by `versionx release apply`. `content_hash` captures the
/// BLAKE3 of the component at release time — `versionx bump` diffs it
/// against the current hash to decide dirtiness.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LockedComponent {
    /// Component version at release time (e.g. `"1.2.3"`).
    pub version: String,
    /// BLAKE3 content hash captured at release time, prefixed `"blake3:"`.
    pub content_hash: String,
    /// Timestamp the release was applied. Informational only.
    pub released_at: DateTime<Utc>,
    /// Git tag name that was created for this release, when applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    /// Commit SHA that landed the release. Useful for debugging + audit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
}

#[derive(Debug, Error)]
pub enum LockfileError {
    #[error("lockfile i/o error at {path}: {source}")]
    Io {
        path: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("parsing lockfile at {path}: {source}")]
    Parse {
        path: Utf8PathBuf,
        #[source]
        source: toml::de::Error,
    },
    #[error("writing lockfile at {path}: {source}")]
    Serialize {
        path: Utf8PathBuf,
        #[source]
        source: toml::ser::Error,
    },
    #[error(
        "lockfile at {path} has schema_version={found}, this versionx binary \
         supports up to {supported}"
    )]
    SchemaTooNew { path: Utf8PathBuf, found: String, supported: String },
}

pub type LockfileResult<T> = Result<T, LockfileError>;

impl Lockfile {
    /// Build a fresh lockfile with the current timestamp + versionx version.
    #[must_use]
    pub fn new(config_hash: impl Into<String>) -> Self {
        Self {
            schema_version: SUPPORTED_SCHEMA_VERSION.into(),
            generated_at: Utc::now(),
            versionx_version: env!("CARGO_PKG_VERSION").into(),
            config_hash: config_hash.into(),
            runtimes: IndexMap::new(),
            ecosystems: IndexMap::new(),
            links: IndexMap::new(),
            components: IndexMap::new(),
        }
    }

    /// Read + validate a lockfile from disk.
    pub fn load(path: impl AsRef<Utf8Path>) -> LockfileResult<Self> {
        let path = path.as_ref().to_path_buf();
        let raw = fs::read_to_string(&path)
            .map_err(|source| LockfileError::Io { path: path.clone(), source })?;
        Self::from_str_at(&raw, &path)
    }

    /// Parse from a string, with the original path for error context.
    pub fn from_str_at(source: &str, path: &Utf8Path) -> LockfileResult<Self> {
        let lock: Self = toml::from_str(source)
            .map_err(|source| LockfileError::Parse { path: path.to_path_buf(), source })?;
        if lock.schema_version.as_str() > SUPPORTED_SCHEMA_VERSION {
            return Err(LockfileError::SchemaTooNew {
                path: path.to_path_buf(),
                found: lock.schema_version,
                supported: SUPPORTED_SCHEMA_VERSION.into(),
            });
        }
        Ok(lock)
    }

    /// Render to a TOML string with a do-not-edit header.
    pub fn to_toml_string(&self) -> LockfileResult<String> {
        let body = toml::to_string_pretty(self).map_err(|source| LockfileError::Serialize {
            path: Utf8PathBuf::from("<memory>"),
            source,
        })?;
        Ok(format!(
            "# DO NOT EDIT — managed by `versionx sync`\n# See docs/spec/02-config-and-state-model.md §3\n\n{body}"
        ))
    }

    /// Write atomically: serialize to a sibling `.tmp` and rename over the
    /// target so concurrent readers never see a half-written file.
    pub fn save(&self, path: impl AsRef<Utf8Path>) -> LockfileResult<()> {
        let path = path.as_ref().to_path_buf();
        let body = self.to_toml_string()?;
        let tmp = path.with_extension("lock.tmp");
        fs::write(&tmp, body).map_err(|source| LockfileError::Io { path: tmp.clone(), source })?;
        fs::rename(&tmp, &path)
            .map_err(|source| LockfileError::Io { path: path.clone(), source })?;
        Ok(())
    }
}

/// BLAKE3 content hash prefixed with `"blake3:"`.
#[must_use]
pub fn blake3_hex(bytes: &[u8]) -> String {
    format!("blake3:{}", blake3::hash(bytes).to_hex())
}

/// SHA-256 content hash, bare hex (no prefix — matches common checksum file format).
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    let digest = sha2::Sha256::digest(bytes);
    hex::encode(digest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_preserves_fields() {
        let mut lock = Lockfile::new("blake3:abcd");
        lock.runtimes.insert(
            "node".into(),
            LockedRuntime {
                version: "20.11.1".into(),
                source: "nodejs.org".into(),
                sha256: Some("deadbeef".into()),
                install_path: None,
            },
        );
        let rendered = lock.to_toml_string().unwrap();
        assert!(rendered.starts_with("# DO NOT EDIT"));
        let parsed = Lockfile::from_str_at(&rendered, Utf8Path::new("<test>")).unwrap();
        assert_eq!(parsed.config_hash, "blake3:abcd");
        assert_eq!(parsed.runtimes["node"].version, "20.11.1");
    }

    #[test]
    fn save_is_atomic() {
        let tmp = tempfile::tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(tmp.path().join("versionx.lock")).unwrap();
        let lock = Lockfile::new("blake3:x");
        lock.save(&path).unwrap();
        let reread = Lockfile::load(&path).unwrap();
        assert_eq!(reread.config_hash, "blake3:x");

        // No leftover .tmp.
        let tmp_file = path.with_extension("lock.tmp");
        assert!(!tmp_file.exists());
    }

    #[test]
    fn rejects_future_schema_version() {
        let src = r#"
            schema_version = "999"
            generated_at = "2026-04-18T00:00:00Z"
            versionx_version = "0.1.0"
            config_hash = "blake3:x"
        "#;
        let err = Lockfile::from_str_at(src, Utf8Path::new("<test>")).unwrap_err();
        assert!(matches!(err, LockfileError::SchemaTooNew { .. }));
    }

    #[test]
    fn unknown_top_level_field_errors() {
        let src = r#"
            schema_version = "1"
            generated_at = "2026-04-18T00:00:00Z"
            versionx_version = "0.1.0"
            config_hash = "blake3:x"
            unknown_key = "whoops"
        "#;
        let err = Lockfile::from_str_at(src, Utf8Path::new("<test>")).unwrap_err();
        assert!(matches!(err, LockfileError::Parse { .. }));
    }

    #[test]
    fn blake3_prefixed() {
        assert!(blake3_hex(b"hello").starts_with("blake3:"));
    }

    #[test]
    fn sha256_64_hex_chars() {
        assert_eq!(sha256_hex(b"").len(), 64);
    }

    #[test]
    fn components_round_trip() {
        let mut lock = Lockfile::new("blake3:x");
        lock.components.insert(
            "core".into(),
            LockedComponent {
                version: "1.2.3".into(),
                content_hash: "blake3:abc".into(),
                released_at: Utc::now(),
                tag: Some("v1.2.3".into()),
                commit: Some("deadbeef".into()),
            },
        );
        let rendered = lock.to_toml_string().unwrap();
        let reread = Lockfile::from_str_at(&rendered, Utf8Path::new("<test>")).unwrap();
        let c = reread.components.get("core").unwrap();
        assert_eq!(c.version, "1.2.3");
        assert_eq!(c.content_hash, "blake3:abc");
        assert_eq!(c.tag.as_deref(), Some("v1.2.3"));
    }
}
