//! Link handlers — one module per kind.
//!
//! A "link" is how a workspace pulls in code from another repository:
//!   - [`submodule`]: classic `.gitmodules` — each link is a pinned
//!     commit in a separate repo the host checks out.
//!   - [`subtree`]: vendored via `git subtree` — the upstream history
//!     is merged into the host's history.
//!   - [`virtual`]: a local directory that points at a sibling
//!     checkout (symlink or config pointer). No git-level coupling.
//!   - [`ref`]: a git-ref-only reference — we record the upstream ref
//!     hash so `versionx links check-updates` can spot drift without
//!     cloning.
//!
//! Every kind implements [`LinkHandler`] — a small sync trait with
//! `sync`, `check_updates`, `update`, `pull`, `push`. The CLI dispatch
//! picks the implementation from [`LinkSpec::kind`].

pub mod r#ref;
pub mod submodule;
pub mod subtree;
pub mod r#virtual;

use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkKind {
    Submodule,
    Subtree,
    Virtual,
    Ref,
}

/// One entry in `[[links]]` (in either `versionx.toml` or
/// `versionx-fleet.toml`). Mirrors the shape used by
/// `versionx-config::schema::LinkConfig` but lives here so multirepo
/// doesn't depend on the full config type.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LinkSpec {
    pub name: String,
    pub kind: LinkKind,
    pub url: String,
    /// Where the link is checked out / mounted relative to the
    /// workspace root.
    pub path: Utf8PathBuf,
    /// The git ref to track on the upstream side.
    pub track: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct LinkStatus {
    pub name: String,
    pub kind: LinkKind,
    pub path: Utf8PathBuf,
    pub local_sha: Option<String>,
    pub upstream_sha: Option<String>,
    pub up_to_date: bool,
    pub message: String,
}

#[derive(Debug, thiserror::Error)]
pub enum LinkError {
    #[error("git error: {0}")]
    Git(#[from] versionx_git::WriteError),
    #[error("read error: {0}")]
    Read(#[from] versionx_git::ReadError),
    #[error("io error at {path}: {source}")]
    Io {
        path: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{0}")]
    Other(String),
}

pub type LinkResult<T> = Result<T, LinkError>;

/// Synchronous trait — we explicitly stay non-async. All implementations
/// shell out to git (via `versionx_git`) which is blocking IO that's
/// cleaner without async ceremony.
pub trait LinkHandler {
    /// Bring the link into the expected state at `workspace_root`.
    /// Idempotent.
    fn sync(&self, workspace_root: &Utf8Path, spec: &LinkSpec) -> LinkResult<LinkStatus>;

    /// Report whether the upstream has moved relative to what's pinned.
    fn check_updates(&self, workspace_root: &Utf8Path, spec: &LinkSpec) -> LinkResult<LinkStatus>;

    /// Update the local copy to the latest upstream commit on `track`.
    fn update(&self, workspace_root: &Utf8Path, spec: &LinkSpec) -> LinkResult<LinkStatus>;

    /// Pull bi-directional changes where that makes sense (subtree +
    /// virtual). No-op for submodule / ref.
    fn pull(&self, workspace_root: &Utf8Path, spec: &LinkSpec) -> LinkResult<LinkStatus>;

    /// Push local commits upstream (subtree only). No-op for
    /// submodule / virtual / ref.
    fn push(&self, workspace_root: &Utf8Path, spec: &LinkSpec) -> LinkResult<LinkStatus>;
}

/// Dispatch to the right handler.
pub fn handler_for(kind: &LinkKind) -> Box<dyn LinkHandler> {
    match kind {
        LinkKind::Submodule => Box::new(submodule::SubmoduleHandler),
        LinkKind::Subtree => Box::new(subtree::SubtreeHandler),
        LinkKind::Virtual => Box::new(r#virtual::VirtualHandler),
        LinkKind::Ref => Box::new(r#ref::RefHandler),
    }
}

/// Convenience: run `sync` on every spec and collect the results.
/// Fail-fast — the first error aborts.
pub fn sync_all(root: &Utf8Path, specs: &[LinkSpec]) -> LinkResult<Vec<LinkStatus>> {
    let mut out = Vec::with_capacity(specs.len());
    for spec in specs {
        out.push(handler_for(&spec.kind).sync(root, spec)?);
    }
    Ok(out)
}

/// Convenience: check every spec for updates.
pub fn check_updates_all(root: &Utf8Path, specs: &[LinkSpec]) -> LinkResult<Vec<LinkStatus>> {
    let mut out = Vec::with_capacity(specs.len());
    for spec in specs {
        out.push(handler_for(&spec.kind).check_updates(root, spec)?);
    }
    Ok(out)
}
