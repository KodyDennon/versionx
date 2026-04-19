//! `versionx-fleet.toml` — the ops-repo pattern.
//!
//! A fleet file lives in a dedicated ops repo (or alongside `versionx.toml`
//! in a monolithic monorepo) and groups member repositories into named
//! sets. Ops like `versionx fleet release propose --set customer-portal`
//! operate on every member in the named set in parallel.
//!
//! ```toml
//! schema_version = "1"
//!
//! [[member]]
//! name = "frontend"
//! path = "./repos/frontend"
//! remote = "git@github.com:acme/frontend.git"
//!
//! [[member]]
//! name = "api"
//! path = "./repos/api"
//!
//! [[set]]
//! name = "customer-portal"
//! members = ["frontend", "api", "shared"]
//! release_mode = "coordinated"      # "independent" | "gated" | "coordinated"
//! ```
//!
//! See `docs/spec/06-multi-repo-and-monorepo.md`.

use std::fs;

use camino::{Utf8Path, Utf8PathBuf};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

pub const SUPPORTED_SCHEMA_VERSION: &str = "1";
pub const DEFAULT_FLEET_FILENAME: &str = "versionx-fleet.toml";

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FleetConfig {
    #[serde(default = "default_schema_version")]
    pub schema_version: String,
    #[serde(default, rename = "member", skip_serializing_if = "Vec::is_empty")]
    pub members: Vec<Member>,
    #[serde(default, rename = "set", skip_serializing_if = "Vec::is_empty")]
    pub sets: Vec<ReleaseSet>,
}

fn default_schema_version() -> String {
    SUPPORTED_SCHEMA_VERSION.into()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Member {
    /// Stable identifier used by `[[set]].members`.
    pub name: String,
    /// Repo-relative path (relative to the fleet file).
    pub path: Utf8PathBuf,
    /// Remote URL — optional when the member is already checked out at
    /// `path`. When missing, `versionx fleet sync` won't clone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote: Option<String>,
    /// Branch to track for pulls. Defaults to `"main"`.
    #[serde(default = "default_branch")]
    pub branch: String,
    /// Optional tags for `versionx fleet query`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

fn default_branch() -> String {
    "main".into()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseSet {
    /// Set name — `versionx fleet release --set <name>`.
    pub name: String,
    /// Member names. Each must resolve to a declared `[[member]]`.
    pub members: Vec<String>,
    /// Release orchestration mode. One of:
    ///   - `independent`: parallel, no atomicity.
    ///   - `gated`: topological order; stop on first failure.
    ///   - `coordinated`: dry-run all → tag all → apply in topo order
    ///     with rollback on failure.
    #[serde(default = "default_release_mode")]
    pub release_mode: String,
    /// Optional dependency graph between members. When present the
    /// saga uses this order; otherwise the order from `members` is
    /// honored verbatim.
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub depends_on: IndexMap<String, Vec<String>>,
}

fn default_release_mode() -> String {
    "coordinated".into()
}

#[derive(Debug, thiserror::Error)]
pub enum FleetError {
    #[error("fleet config not found at {path}")]
    NotFound { path: Utf8PathBuf },
    #[error("io error at {path}: {source}")]
    Io {
        path: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("parse error at {path}: {message}")]
    Parse { path: Utf8PathBuf, message: String },
    #[error("duplicate member name: {name}")]
    DuplicateMember { name: String },
    #[error("duplicate set name: {name}")]
    DuplicateSet { name: String },
    #[error("set `{set}` references unknown member `{member}`")]
    UnknownMember { set: String, member: String },
}

pub type FleetResult<T> = Result<T, FleetError>;

impl FleetConfig {
    /// Locate the fleet file. Walks up from `start_dir` looking for
    /// `versionx-fleet.toml`. Returns the path on first hit; errors
    /// with `NotFound` if none is found.
    pub fn discover(start_dir: &Utf8Path) -> FleetResult<Utf8PathBuf> {
        let mut cursor = start_dir.to_path_buf();
        loop {
            let candidate = cursor.join(DEFAULT_FLEET_FILENAME);
            if candidate.is_file() {
                return Ok(candidate);
            }
            let Some(parent) = cursor.parent() else {
                return Err(FleetError::NotFound { path: start_dir.to_path_buf() });
            };
            if parent == cursor {
                return Err(FleetError::NotFound { path: start_dir.to_path_buf() });
            }
            cursor = parent.to_path_buf();
        }
    }

    /// Load + validate from disk.
    pub fn load(path: &Utf8Path) -> FleetResult<Self> {
        let raw = fs::read_to_string(path.as_std_path())
            .map_err(|source| FleetError::Io { path: path.to_path_buf(), source })?;
        Self::from_toml(&raw, path)
    }

    /// Parse from a TOML string with path context for errors.
    pub fn from_toml(source: &str, path: &Utf8Path) -> FleetResult<Self> {
        let cfg: Self = toml::from_str(source)
            .map_err(|e| FleetError::Parse { path: path.to_path_buf(), message: e.to_string() })?;
        cfg.validate()?;
        Ok(cfg)
    }

    /// Serialize. Formatting isn't round-trip perfect — use
    /// `toml_edit` if user formatting preservation matters.
    pub fn to_toml(&self) -> FleetResult<String> {
        toml::to_string_pretty(self).map_err(|e| FleetError::Parse {
            path: Utf8PathBuf::from("<memory>"),
            message: e.to_string(),
        })
    }

    /// Look up a set by name.
    pub fn set(&self, name: &str) -> Option<&ReleaseSet> {
        self.sets.iter().find(|s| s.name == name)
    }

    /// Look up a member by name.
    pub fn member(&self, name: &str) -> Option<&Member> {
        self.members.iter().find(|m| m.name == name)
    }

    fn validate(&self) -> FleetResult<()> {
        let mut names = std::collections::BTreeSet::new();
        for m in &self.members {
            if !names.insert(&m.name) {
                return Err(FleetError::DuplicateMember { name: m.name.clone() });
            }
        }
        let mut set_names = std::collections::BTreeSet::new();
        for s in &self.sets {
            if !set_names.insert(&s.name) {
                return Err(FleetError::DuplicateSet { name: s.name.clone() });
            }
            for m in &s.members {
                if !names.contains(m) {
                    return Err(FleetError::UnknownMember {
                        set: s.name.clone(),
                        member: m.clone(),
                    });
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_minimal_fleet() {
        let src = r#"
            schema_version = "1"

            [[member]]
            name = "frontend"
            path = "./repos/frontend"
            remote = "git@github.com:acme/frontend.git"
            branch = "main"

            [[member]]
            name = "api"
            path = "./repos/api"

            [[set]]
            name = "customer-portal"
            members = ["frontend", "api"]
            release_mode = "coordinated"
        "#;
        let cfg = FleetConfig::from_toml(src, Utf8Path::new("<t>")).unwrap();
        assert_eq!(cfg.members.len(), 2);
        assert_eq!(cfg.set("customer-portal").unwrap().members.len(), 2);
    }

    #[test]
    fn rejects_unknown_member_in_set() {
        let src = r#"
            [[member]]
            name = "a"
            path = "./a"
            [[set]]
            name = "s"
            members = ["a", "b"]
        "#;
        let err = FleetConfig::from_toml(src, Utf8Path::new("<t>")).unwrap_err();
        assert!(matches!(err, FleetError::UnknownMember { .. }));
    }

    #[test]
    fn discover_walks_up() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        std::fs::write(root.join("versionx-fleet.toml"), "schema_version = \"1\"\n").unwrap();
        let child = root.join("deep/nested");
        std::fs::create_dir_all(child.as_std_path()).unwrap();
        let found = FleetConfig::discover(&child).unwrap();
        assert_eq!(found, root.join("versionx-fleet.toml"));
    }
}
