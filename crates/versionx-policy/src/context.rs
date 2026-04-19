//! Evaluation context — what the rules see.
//!
//! Rules consume a [`PolicyContext`] snapshot rather than reaching into
//! live state. That keeps evaluation deterministic (identical input ⇒
//! identical findings) and lets us ship a test-only builder without
//! spinning up a real workspace.

use std::collections::BTreeMap;

use camino::Utf8PathBuf;
use indexmap::IndexMap;

use crate::schema::Trigger;

/// One component as seen by the policy engine.
#[derive(Clone, Debug)]
pub struct ContextComponent {
    pub id: String,
    pub kind: String,
    pub root: Utf8PathBuf,
    pub version: Option<String>,
    /// Manifest dependency declarations, keyed by dep name → spec
    /// string. Used by `dependency_version` / `dependency_presence`.
    pub dependencies: BTreeMap<String, String>,
    /// Free-form component tags pulled from config (used by the `tag`
    /// scope selector).
    pub tags: Vec<String>,
}

/// One runtime pin the policy can see (`runtime_version` rule).
#[derive(Clone, Debug)]
pub struct ContextRuntime {
    pub name: String,
    pub version: String,
}

/// Commit metadata (used by `commit_format`).
#[derive(Clone, Debug)]
pub struct ContextCommit {
    pub sha: String,
    pub message: String,
}

/// One external-repo link (`link_freshness`).
#[derive(Clone, Debug)]
pub struct ContextLink {
    pub name: String,
    /// How many days since the link was last updated.
    pub age_days: Option<i64>,
}

/// The full snapshot handed to evaluators.
#[derive(Clone, Debug, Default)]
pub struct PolicyContext {
    /// Which evaluation phase we're in.
    pub trigger: Option<Trigger>,
    /// Root of the workspace (for path-scoped rules).
    pub workspace_root: Utf8PathBuf,
    pub components: IndexMap<String, ContextComponent>,
    pub runtimes: IndexMap<String, ContextRuntime>,
    /// Commits in play. For `release_propose` this is commits since the
    /// last release tag; for `sync` it's empty.
    pub commits: Vec<ContextCommit>,
    pub links: IndexMap<String, ContextLink>,
    /// Whether the versionx.lock file matches what native lockfiles
    /// produce — fed in by the `versionx verify` step. `None` means the
    /// engine doesn't yet know.
    pub lockfile_integrity_ok: Option<bool>,
    /// Components with registered sigstore attestations keyed by
    /// component id. Empty map means "no provenance recorded".
    pub provenance: IndexMap<String, String>,
    /// Named advisories the resolver pulled in (CVE ids, GHSAs).
    /// Key: advisory id; value: affected package spec.
    pub advisories: IndexMap<String, String>,
}

impl PolicyContext {
    /// Start fresh with just a workspace root; callers mutate the other
    /// fields in before handing to the engine.
    #[must_use]
    pub fn new(workspace_root: Utf8PathBuf) -> Self {
        Self { workspace_root, ..Self::default() }
    }
}
