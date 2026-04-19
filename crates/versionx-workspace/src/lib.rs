//! Component discovery, dependency graph, and content hashing — the
//! Versionx workspace core.
//!
//! A *workspace* is a set of [`Component`]s with a directed dependency
//! [`Graph`] between them. Components come from two sources:
//!
//! 1. **Auto-discovery**: walks the workspace root looking for known
//!    manifest files (`package.json`, `Cargo.toml`, `pyproject.toml`, …).
//!    Each manifest becomes one component.
//! 2. **Explicit `[[components]]` in versionx.toml**: lets users declare
//!    components that don't have a native manifest — e.g. a shared
//!    `.proto` file, a SQL schema, a bash scripts dir, a docs site.
//!
//! Dependencies are stitched together from native package-manager
//! declarations (Cargo's `[workspace].members`, pnpm's `workspace:*`,
//! uv's `[tool.uv.workspace]`) + explicit `depends_on = [...]` entries
//! in the `[[components]]` table for cross-language links.
//!
//! Change detection uses BLAKE3 content hashing over each component's
//! `inputs` glob (defaults to every file under the component's path,
//! minus `.gitignore` + a small hard-coded blocklist). The hash is
//! stored in the lockfile; a mismatch means the component is "dirty".

#![deny(unsafe_code)]

pub mod discovery;
pub mod error;
pub mod graph;
pub mod hash;
pub mod model;

pub use error::{WorkspaceError, WorkspaceResult};
pub use graph::{ComponentGraph, cascade_from};
pub use model::{
    Component, ComponentId, ComponentKind, ComponentRef, ComponentSource, VersionedComponent,
    Workspace,
};

/// Crate version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_nonempty() {
        assert!(!VERSION.is_empty());
    }
}
