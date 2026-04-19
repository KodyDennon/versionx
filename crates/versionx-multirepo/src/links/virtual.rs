//! Virtual link handler.
//!
//! A "virtual" link is a local directory reference — often a symlink
//! — to a sibling checkout. No git coupling at all; we just verify
//! the target exists + report whether it's a live git repo we can
//! introspect.
//!
//! Useful for monorepo-of-repos setups where every member is
//! cloned independently and the fleet file provides the cross-cuts.

use camino::Utf8Path;

use super::{LinkHandler, LinkResult, LinkSpec, LinkStatus};

#[derive(Debug)]
pub struct VirtualHandler;

impl LinkHandler for VirtualHandler {
    fn sync(&self, workspace_root: &Utf8Path, spec: &LinkSpec) -> LinkResult<LinkStatus> {
        let full = workspace_root.join(&spec.path);
        let present = full.exists();
        let is_git = present && versionx_git::read::summarize(&full).map(|_| true).unwrap_or(false);
        Ok(LinkStatus {
            name: spec.name.clone(),
            kind: spec.kind.clone(),
            path: spec.path.clone(),
            local_sha: if is_git {
                versionx_git::read::summarize(&full).ok().map(|s| s.head_sha)
            } else {
                None
            },
            upstream_sha: None,
            up_to_date: present,
            message: if !present {
                format!("missing at {}", spec.path)
            } else if is_git {
                format!("present ({})", spec.path)
            } else {
                format!("present (not a git repo) ({})", spec.path)
            },
        })
    }

    fn check_updates(&self, workspace_root: &Utf8Path, spec: &LinkSpec) -> LinkResult<LinkStatus> {
        // For virtual links "update" is ambiguous — we just report
        // the current state, same as sync.
        self.sync(workspace_root, spec)
    }

    fn update(&self, workspace_root: &Utf8Path, spec: &LinkSpec) -> LinkResult<LinkStatus> {
        // No-op; virtual links don't have a remote concept.
        self.sync(workspace_root, spec)
    }

    fn pull(&self, workspace_root: &Utf8Path, spec: &LinkSpec) -> LinkResult<LinkStatus> {
        self.sync(workspace_root, spec)
    }

    fn push(&self, workspace_root: &Utf8Path, spec: &LinkSpec) -> LinkResult<LinkStatus> {
        self.sync(workspace_root, spec)
    }
}
