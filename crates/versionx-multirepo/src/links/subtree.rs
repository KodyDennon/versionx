//! Subtree link handler.
//!
//! `git subtree` flattens an upstream repo into the host's history.
//! `sync` verifies the subtree is present; `pull` runs `git subtree
//! pull` from `spec.url` on `spec.track`; `push` pushes host commits
//! back upstream. `check_updates` compares the host's vendored tree
//! to the remote tip — non-trivial for true subtrees, so we simply
//! surface the remote tip and leave equality to a future
//! content-aware compare.

use std::process::Command;

use camino::Utf8Path;

use super::{LinkError, LinkHandler, LinkResult, LinkSpec, LinkStatus};

#[derive(Debug)]
pub struct SubtreeHandler;

impl LinkHandler for SubtreeHandler {
    fn sync(&self, workspace_root: &Utf8Path, spec: &LinkSpec) -> LinkResult<LinkStatus> {
        let subtree_path = workspace_root.join(&spec.path);
        let exists = subtree_path.is_dir();
        let message = if exists {
            format!("subtree present at {}", spec.path)
        } else {
            format!("subtree missing at {}", spec.path)
        };
        Ok(LinkStatus {
            name: spec.name.clone(),
            kind: spec.kind.clone(),
            path: spec.path.clone(),
            local_sha: None,
            upstream_sha: None,
            up_to_date: exists,
            message,
        })
    }

    fn check_updates(&self, _workspace_root: &Utf8Path, spec: &LinkSpec) -> LinkResult<LinkStatus> {
        let upstream = ls_remote_tip(&spec.url, &spec.track).ok();
        Ok(LinkStatus {
            name: spec.name.clone(),
            kind: spec.kind.clone(),
            path: spec.path.clone(),
            local_sha: None,
            upstream_sha: upstream.clone(),
            up_to_date: false, // we don't have cheap equality; surface the tip
            message: upstream
                .as_deref()
                .map(|s| format!("upstream tip {s}"))
                .unwrap_or_else(|| "unable to reach upstream".into()),
        })
    }

    fn update(&self, workspace_root: &Utf8Path, spec: &LinkSpec) -> LinkResult<LinkStatus> {
        run_git(
            workspace_root,
            &[
                "subtree",
                "pull",
                "--prefix",
                spec.path.as_str(),
                &spec.url,
                &spec.track,
                "--squash",
            ],
        )?;
        Ok(LinkStatus {
            name: spec.name.clone(),
            kind: spec.kind.clone(),
            path: spec.path.clone(),
            local_sha: None,
            upstream_sha: None,
            up_to_date: true,
            message: "pulled subtree".into(),
        })
    }

    fn pull(&self, workspace_root: &Utf8Path, spec: &LinkSpec) -> LinkResult<LinkStatus> {
        // `pull` and `update` are the same operation for subtrees.
        self.update(workspace_root, spec)
    }

    fn push(&self, workspace_root: &Utf8Path, spec: &LinkSpec) -> LinkResult<LinkStatus> {
        run_git(
            workspace_root,
            &["subtree", "push", "--prefix", spec.path.as_str(), &spec.url, &spec.track],
        )?;
        Ok(LinkStatus {
            name: spec.name.clone(),
            kind: spec.kind.clone(),
            path: spec.path.clone(),
            local_sha: None,
            upstream_sha: None,
            up_to_date: true,
            message: format!("pushed subtree to {}", spec.url),
        })
    }
}

fn run_git(cwd: &Utf8Path, args: &[&str]) -> LinkResult<()> {
    let out = Command::new("git")
        .arg("-C")
        .arg(cwd.as_str())
        .args(args)
        .output()
        .map_err(|source| LinkError::Io { path: cwd.to_path_buf(), source })?;
    if !out.status.success() {
        return Err(LinkError::Other(format!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim(),
        )));
    }
    Ok(())
}

fn ls_remote_tip(url: &str, ref_name: &str) -> LinkResult<String> {
    let out = Command::new("git")
        .args(["ls-remote", url, ref_name])
        .output()
        .map_err(|source| LinkError::Io { path: camino::Utf8PathBuf::from("."), source })?;
    if !out.status.success() {
        return Err(LinkError::Other(String::from_utf8_lossy(&out.stderr).trim().to_string()));
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    stdout
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().next())
        .map(str::to_string)
        .ok_or_else(|| LinkError::Other(format!("no tip for {url} {ref_name}")))
}
