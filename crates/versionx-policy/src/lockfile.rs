//! `versionx.policy.lock` — pin inherited policy sources to a content
//! SHA so downstream repos can't silently diverge.
//!
//! Shape:
//! ```toml
//! schema_version = "1"
//! generated_at = 2026-04-18T12:00:00Z
//!
//! [[source]]
//! path = ".versionx/policies/main.toml"
//! blake3 = "blake3:abcd…"
//! sealed = ["no-ancient-node"]
//!
//! [[source]]
//! path = ".versionx/policies/vendored/security.toml"
//! blake3 = "blake3:efgh…"
//! sealed = ["no-blocked-cves", "lockfile-integrity"]
//! ```
//!
//! `versionx policy update` refreshes the hashes after verifying the
//! upstream content still satisfies the sealed invariants.

use std::fs;

use camino::{Utf8Path, Utf8PathBuf};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const SUPPORTED_SCHEMA_VERSION: &str = "1";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyLockfile {
    #[serde(default = "default_schema_version")]
    pub schema_version: String,
    pub generated_at: DateTime<Utc>,
    #[serde(default, rename = "source", skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<LockedSource>,
}

fn default_schema_version() -> String {
    SUPPORTED_SCHEMA_VERSION.into()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LockedSource {
    /// Relative path (or URL) of the policy source.
    pub path: String,
    /// Content hash at lock time, `"blake3:<hex>"`.
    pub blake3: String,
    /// Policy names that are sealed from this source. The engine
    /// refuses to load a policy file that tries to disable one of
    /// these names.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sealed: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum LockfileError {
    #[error("io error at {path}: {source}")]
    Io {
        path: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("parse error at {path}: {source}")]
    Parse {
        path: Utf8PathBuf,
        #[source]
        source: toml::de::Error,
    },
    #[error("serialize error: {0}")]
    Ser(#[from] toml::ser::Error),
    #[error("sealed policy `{name}` disabled by {by}")]
    SealedDisabled { name: String, by: String },
}

impl PolicyLockfile {
    #[must_use]
    pub fn new() -> Self {
        Self {
            schema_version: SUPPORTED_SCHEMA_VERSION.into(),
            generated_at: Utc::now(),
            sources: Vec::new(),
        }
    }

    pub fn load(path: &Utf8Path) -> Result<Self, LockfileError> {
        let raw = fs::read_to_string(path.as_std_path())
            .map_err(|source| LockfileError::Io { path: path.to_path_buf(), source })?;
        toml::from_str(&raw)
            .map_err(|source| LockfileError::Parse { path: path.to_path_buf(), source })
    }

    pub fn save(&self, path: &Utf8Path) -> Result<(), LockfileError> {
        let body = toml::to_string_pretty(self)?;
        let tmp = path.with_extension("lock.tmp");
        fs::write(tmp.as_std_path(), body)
            .map_err(|source| LockfileError::Io { path: tmp.clone(), source })?;
        fs::rename(tmp.as_std_path(), path.as_std_path())
            .map_err(|source| LockfileError::Io { path: path.to_path_buf(), source })?;
        Ok(())
    }

    /// Enforce that a freshly-loaded policy source does not attempt to
    /// disable any names that a sibling `sealed` list protects.
    pub fn enforce_seals(
        &self,
        current_path: &str,
        current_disabled: &[String],
    ) -> Result<(), LockfileError> {
        for source in &self.sources {
            if source.path == current_path {
                continue;
            }
            for sealed_name in &source.sealed {
                if current_disabled.iter().any(|d| d == sealed_name) {
                    return Err(LockfileError::SealedDisabled {
                        name: sealed_name.clone(),
                        by: current_path.to_string(),
                    });
                }
            }
        }
        Ok(())
    }
}

impl Default for PolicyLockfile {
    fn default() -> Self {
        Self::new()
    }
}

/// Compute the canonical hash of a policy source (any TOML file on
/// disk). `"blake3:"`-prefixed hex.
#[must_use]
pub fn hash_source(path: &Utf8Path) -> std::io::Result<String> {
    let bytes = std::fs::read(path.as_std_path())?;
    Ok(format!("blake3:{}", blake3::hash(&bytes).to_hex()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_with_sources() {
        let mut lf = PolicyLockfile::new();
        lf.sources.push(LockedSource {
            path: ".versionx/policies/main.toml".into(),
            blake3: "blake3:abc".into(),
            sealed: vec!["no-ancient-node".into()],
        });
        let tmp = tempfile::tempdir().unwrap();
        let p = Utf8PathBuf::from_path_buf(tmp.path().join("policy.lock")).unwrap();
        lf.save(&p).unwrap();
        let back = PolicyLockfile::load(&p).unwrap();
        assert_eq!(back.sources.len(), 1);
        assert_eq!(back.sources[0].blake3, "blake3:abc");
    }

    #[test]
    fn sealed_disable_is_refused() {
        let mut lf = PolicyLockfile::new();
        lf.sources.push(LockedSource {
            path: "org/policies.toml".into(),
            blake3: "blake3:upstream".into(),
            sealed: vec!["no-ancient-node".into()],
        });
        let err = lf.enforce_seals("local.toml", &["no-ancient-node".into()]).unwrap_err();
        assert!(matches!(err, LockfileError::SealedDisabled { .. }));
    }

    #[test]
    fn hash_source_is_stable() {
        let tmp = tempfile::tempdir().unwrap();
        let p = Utf8PathBuf::from_path_buf(tmp.path().join("x.toml")).unwrap();
        std::fs::write(p.as_std_path(), "a = 1\n").unwrap();
        let h1 = hash_source(&p).unwrap();
        let h2 = hash_source(&p).unwrap();
        assert_eq!(h1, h2);
        assert!(h1.starts_with("blake3:"));
    }
}
