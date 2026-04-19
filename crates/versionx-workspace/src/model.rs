//! Core types: [`Component`], [`Workspace`], identifiers, versions.

use std::collections::BTreeSet;

use camino::Utf8PathBuf;
use semver::Version;
use serde::{Deserialize, Serialize};

/// Stable identifier for a component within a workspace.
///
/// Derived from the component's native name when available
/// (`@acme/ui` → `"@acme/ui"`; `serde` → `"serde"`; `my-app` → `"my-app"`).
/// For explicit `[[components]]` entries the user-supplied `name` wins.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ComponentId(pub String);

impl ComponentId {
    #[must_use]
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ComponentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Where the component came from — an auto-discovered manifest or an
/// explicit `[[components]]` entry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComponentSource {
    /// Auto-discovered via a known manifest file (`package.json`, `Cargo.toml`, ...).
    Manifest { manifest_path: Utf8PathBuf },
    /// Declared explicitly in `versionx.toml`'s `[[components]]` array.
    Declared,
}

/// What *kind* of component this is. Used to pick an adapter + default
/// command mappings. Extensible.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComponentKind {
    Node,
    Python,
    Rust,
    Go,
    Ruby,
    Jvm,
    Oci,
    /// "Other" covers proto files, SQL schemas, shell-script libraries,
    /// docs sites, anything not backed by a known PM. Release behavior
    /// is governed entirely by `versionx.toml` for these.
    Other {
        label: String,
    },
}

impl ComponentKind {
    /// Canonical string form used in lockfiles + state DB.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Node => "node",
            Self::Python => "python",
            Self::Rust => "rust",
            Self::Go => "go",
            Self::Ruby => "ruby",
            Self::Jvm => "jvm",
            Self::Oci => "oci",
            Self::Other { label } => label,
        }
    }
}

/// A single component in the workspace.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Component {
    /// Unique id within the workspace.
    pub id: ComponentId,
    /// Human-readable display name (e.g. `"@acme/ui"`).
    pub display_name: String,
    /// Absolute path to the component's root directory.
    pub root: Utf8PathBuf,
    /// What kind of component (drives which adapter is picked).
    pub kind: ComponentKind,
    /// Where this component was discovered.
    pub source: ComponentSource,
    /// Current version as read from the native manifest (or the
    /// `[[components]].version` override). `None` when the component's
    /// manifest lacks a version field (Cargo workspace roots, Python
    /// projects with no `version = ...`, etc.).
    pub version: Option<Version>,
    /// Input globs used when hashing this component. Defaults to
    /// `["**/*"]` under [`root`](Self::root), minus a small blocklist of
    /// noise directories (`target/`, `node_modules/`, `__pycache__/`,
    /// `.venv/`, `dist/`, `build/`).
    pub inputs: Vec<String>,
    /// Component ids this component depends on.
    pub depends_on: BTreeSet<ComponentId>,
}

/// A resolved workspace — every discovered component + the dep graph.
#[derive(Clone, Debug)]
pub struct Workspace {
    /// Root directory of the workspace (the repo root, or whatever path
    /// the caller passed to [`discover`](crate::discovery::discover)).
    pub root: Utf8PathBuf,
    /// Components in deterministic insertion order (matches how they
    /// were discovered — manifests first, walked top-down; then explicit
    /// `[[components]]` entries in the order they appear in
    /// `versionx.toml`).
    pub components: indexmap::IndexMap<ComponentId, Component>,
}

impl Workspace {
    #[must_use]
    pub fn get(&self, id: &ComponentId) -> Option<&Component> {
        self.components.get(id)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.components.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.components.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&ComponentId, &Component)> {
        self.components.iter()
    }
}

/// Lightweight reference used by cross-component plans.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComponentRef {
    pub id: ComponentId,
    pub root: Utf8PathBuf,
}

impl From<&Component> for ComponentRef {
    fn from(c: &Component) -> Self {
        Self { id: c.id.clone(), root: c.root.clone() }
    }
}

/// A component paired with its last-released content hash. Used by the
/// change-detection + bump-proposal pipeline.
#[derive(Clone, Debug)]
pub struct VersionedComponent {
    pub component: Component,
    /// BLAKE3 hash of the component's inputs at last release. `None` when
    /// the component has never been released.
    pub last_hash: Option<String>,
    /// Hash right now. Always present — recomputed on demand.
    pub current_hash: String,
}

impl VersionedComponent {
    /// Whether the component has been modified since its last released hash.
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        self.last_hash.as_deref() != Some(self.current_hash.as_str())
    }
}
