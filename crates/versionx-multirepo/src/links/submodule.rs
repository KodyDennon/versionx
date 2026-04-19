//! Submodule link handler.
//!
//! Submodules are the classic git mechanism. We treat them as
//! read-only from the host's perspective: `sync` runs
//! `git submodule update --init --recursive`, `check_updates` diffs
//! the pinned commit against the remote's `track` ref, `update`
//! fast-forwards + re-pins, `pull`/`push` are no-ops.

use std::process::Command;

use camino::Utf8Path;

use super::{LinkError, LinkHandler, LinkResult, LinkSpec, LinkStatus};

#[derive(Debug)]
pub struct SubmoduleHandler;

impl LinkHandler for SubmoduleHandler {
    fn sync(&self, workspace_root: &Utf8Path, spec: &LinkSpec) -> LinkResult<LinkStatus> {
        run_git(
            workspace_root,
            &["submodule", "update", "--init", "--recursive", spec.path.as_str()],
        )?;
        // After init, read the submodule's HEAD.
        let sub_path = workspace_root.join(&spec.path);
        let local_sha = versionx_git::read::summarize(&sub_path).ok().map(|s| s.head_sha);
        Ok(LinkStatus {
            name: spec.name.clone(),
            kind: spec.kind.clone(),
            path: spec.path.clone(),
            local_sha: local_sha.clone(),
            upstream_sha: None,
            up_to_date: true,
            message: format!("synced {}", spec.name),
        })
    }

    fn check_updates(&self, workspace_root: &Utf8Path, spec: &LinkSpec) -> LinkResult<LinkStatus> {
        let sub_path = workspace_root.join(&spec.path);
        let local_sha = versionx_git::read::summarize(&sub_path).ok().map(|s| s.head_sha);
        let upstream_sha = remote_tip(&spec.url, &spec.track).ok();
        let up_to_date = matches!((&local_sha, &upstream_sha), (Some(l), Some(u)) if l == u);
        Ok(LinkStatus {
            name: spec.name.clone(),
            kind: spec.kind.clone(),
            path: spec.path.clone(),
            local_sha,
            upstream_sha,
            up_to_date,
            message: if up_to_date { "up to date".into() } else { "behind upstream".into() },
        })
    }

    fn update(&self, workspace_root: &Utf8Path, spec: &LinkSpec) -> LinkResult<LinkStatus> {
        let sub_path = workspace_root.join(&spec.path);
        // Fetch + check out the track ref in the submodule.
        run_git(&sub_path, &["fetch", "origin", &spec.track])?;
        run_git(&sub_path, &["checkout", &spec.track])?;
        // Record the new pin in the parent repo.
        run_git(workspace_root, &["add", spec.path.as_str()])?;
        let local_sha = versionx_git::read::summarize(&sub_path).ok().map(|s| s.head_sha);
        Ok(LinkStatus {
            name: spec.name.clone(),
            kind: spec.kind.clone(),
            path: spec.path.clone(),
            local_sha,
            upstream_sha: None,
            up_to_date: true,
            message: format!("updated to {}", spec.track),
        })
    }

    fn pull(&self, _workspace_root: &Utf8Path, spec: &LinkSpec) -> LinkResult<LinkStatus> {
        // Submodules are pinned to a specific commit — `pull` isn't
        // a meaningful op. Surface that clearly.
        Ok(noop_status(spec, "pull is a no-op for submodules"))
    }

    fn push(&self, _workspace_root: &Utf8Path, spec: &LinkSpec) -> LinkResult<LinkStatus> {
        Ok(noop_status(spec, "push is a no-op for submodules"))
    }
}

fn noop_status(spec: &LinkSpec, message: &str) -> LinkStatus {
    LinkStatus {
        name: spec.name.clone(),
        kind: spec.kind.clone(),
        path: spec.path.clone(),
        local_sha: None,
        upstream_sha: None,
        up_to_date: true,
        message: message.to_string(),
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

/// Ask the remote for the SHA of `ref_name` via `git ls-remote`. Works
/// without a clone.
fn remote_tip(url: &str, ref_name: &str) -> LinkResult<String> {
    let out = Command::new("git")
        .args(["ls-remote", url, ref_name])
        .output()
        .map_err(|source| LinkError::Io { path: camino::Utf8PathBuf::from("."), source })?;
    if !out.status.success() {
        return Err(LinkError::Other(format!(
            "ls-remote {url} {ref_name}: {}",
            String::from_utf8_lossy(&out.stderr).trim(),
        )));
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let sha =
        stdout.lines().next().and_then(|l| l.split_whitespace().next()).ok_or_else(|| {
            LinkError::Other(format!("no output from ls-remote {url} {ref_name}"))
        })?;
    Ok(sha.to_string())
}
