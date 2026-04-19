//! `ref` link handler — records an upstream ref SHA without cloning.
//!
//! Useful for lightweight "we depend on this external thing but don't
//! need a working copy" pins. `check_updates` uses `git ls-remote` to
//! fetch the current upstream SHA cheaply; `update` records the new
//! SHA into `.versionx/links/<name>.json` (the tiny persistence we
//! allow here) so subsequent checks can detect drift.

use std::fs;
use std::process::Command;

use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};

use super::{LinkError, LinkHandler, LinkResult, LinkSpec, LinkStatus};

#[derive(Serialize, Deserialize)]
struct RefRecord {
    sha: String,
    track: String,
}

#[derive(Debug)]
pub struct RefHandler;

impl LinkHandler for RefHandler {
    fn sync(&self, workspace_root: &Utf8Path, spec: &LinkSpec) -> LinkResult<LinkStatus> {
        // `sync` is a no-op for ref links. We just report whatever
        // the record says.
        let record = load_record(workspace_root, &spec.name);
        let local = record.as_ref().map(|r| r.sha.clone());
        Ok(LinkStatus {
            name: spec.name.clone(),
            kind: spec.kind.clone(),
            path: spec.path.clone(),
            local_sha: local.clone(),
            upstream_sha: None,
            up_to_date: true,
            message: match &local {
                Some(sha) => format!("pinned at {sha}"),
                None => "no pin recorded".into(),
            },
        })
    }

    fn check_updates(&self, workspace_root: &Utf8Path, spec: &LinkSpec) -> LinkResult<LinkStatus> {
        let upstream = ls_remote_tip(&spec.url, &spec.track).ok();
        let local = load_record(workspace_root, &spec.name).map(|r| r.sha);
        let up_to_date = matches!((&local, &upstream), (Some(l), Some(u)) if l == u);
        Ok(LinkStatus {
            name: spec.name.clone(),
            kind: spec.kind.clone(),
            path: spec.path.clone(),
            local_sha: local,
            upstream_sha: upstream.clone(),
            up_to_date,
            message: upstream
                .as_deref()
                .map(|s| format!("upstream tip {s}"))
                .unwrap_or_else(|| "unreachable upstream".into()),
        })
    }

    fn update(&self, workspace_root: &Utf8Path, spec: &LinkSpec) -> LinkResult<LinkStatus> {
        let upstream = ls_remote_tip(&spec.url, &spec.track)?;
        save_record(
            workspace_root,
            &spec.name,
            &RefRecord { sha: upstream.clone(), track: spec.track.clone() },
        )?;
        Ok(LinkStatus {
            name: spec.name.clone(),
            kind: spec.kind.clone(),
            path: spec.path.clone(),
            local_sha: Some(upstream),
            upstream_sha: None,
            up_to_date: true,
            message: format!("pinned to tip of {}", spec.track),
        })
    }

    fn pull(&self, _workspace_root: &Utf8Path, spec: &LinkSpec) -> LinkResult<LinkStatus> {
        Ok(LinkStatus {
            name: spec.name.clone(),
            kind: spec.kind.clone(),
            path: spec.path.clone(),
            local_sha: None,
            upstream_sha: None,
            up_to_date: true,
            message: "ref links don't pull — there's no working copy".into(),
        })
    }

    fn push(&self, _workspace_root: &Utf8Path, spec: &LinkSpec) -> LinkResult<LinkStatus> {
        Ok(LinkStatus {
            name: spec.name.clone(),
            kind: spec.kind.clone(),
            path: spec.path.clone(),
            local_sha: None,
            upstream_sha: None,
            up_to_date: true,
            message: "ref links don't push".into(),
        })
    }
}

fn record_path(workspace_root: &Utf8Path, name: &str) -> Utf8PathBuf {
    workspace_root.join(format!(".versionx/links/{name}.json"))
}

fn load_record(workspace_root: &Utf8Path, name: &str) -> Option<RefRecord> {
    let p = record_path(workspace_root, name);
    let raw = fs::read_to_string(p.as_std_path()).ok()?;
    serde_json::from_str(&raw).ok()
}

fn save_record(workspace_root: &Utf8Path, name: &str, record: &RefRecord) -> LinkResult<()> {
    let p = record_path(workspace_root, name);
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent.as_std_path())
            .map_err(|source| LinkError::Io { path: parent.to_path_buf(), source })?;
    }
    let body = serde_json::to_string_pretty(record)
        .map_err(|e| LinkError::Other(format!("serialize: {e}")))?;
    fs::write(p.as_std_path(), body).map_err(|source| LinkError::Io { path: p.clone(), source })
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
        .ok_or_else(|| LinkError::Other(format!("no output for {url} {ref_name}")))
}
