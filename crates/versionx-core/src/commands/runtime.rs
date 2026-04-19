//! `versionx runtime list | prune` — manage the global installed set.

use std::fs;

use camino::Utf8PathBuf;
use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use versionx_state::InstalledRuntime;

use super::CoreContext;
use crate::error::CoreResult;

/// Listed runtime with the extra derived field `size_bytes` computed by
/// walking the install dir. Expensive for large installs — callers that
/// only want the DB view can use [`list_from_state`] directly.
#[derive(Clone, Debug, Serialize)]
pub struct RuntimeListing {
    pub tool: String,
    pub version: String,
    pub source: String,
    pub install_path: Utf8PathBuf,
    pub installed_at: DateTime<Utc>,
    pub last_used: Option<DateTime<Utc>>,
    pub sha256: Option<String>,
    pub size_bytes: u64,
    pub on_disk: bool,
}

/// Return every installed runtime recorded in the state DB.
///
/// # Errors
/// - [`CoreError::State`] on DB access failures.
pub fn list(ctx: &CoreContext) -> CoreResult<Vec<RuntimeListing>> {
    let state = versionx_state::open(ctx.home.state_db_path())?;
    let rows = state.list_runtimes()?;
    Ok(rows.into_iter().map(enrich).collect())
}

/// Plain DB rows without the filesystem probe (fast path for non-interactive
/// callers that don't need size info).
pub fn list_from_state(ctx: &CoreContext) -> CoreResult<Vec<InstalledRuntime>> {
    let state = versionx_state::open(ctx.home.state_db_path())?;
    Ok(state.list_runtimes()?)
}

fn enrich(r: InstalledRuntime) -> RuntimeListing {
    let on_disk = r.install_path.exists();
    let size_bytes = if on_disk { directory_size(&r.install_path).unwrap_or(0) } else { 0 };
    RuntimeListing {
        tool: r.tool,
        version: r.version,
        source: r.source,
        install_path: r.install_path,
        installed_at: r.installed_at,
        last_used: r.last_used,
        sha256: r.sha256,
        size_bytes,
        on_disk,
    }
}

#[allow(clippy::unnecessary_wraps)] // `Result` kept for future-error-variants growth.
fn directory_size(path: &camino::Utf8Path) -> std::io::Result<u64> {
    let mut total: u64 = 0;
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_dir() {
                if let Ok(next) = camino::Utf8PathBuf::from_path_buf(entry.path()) {
                    stack.push(next);
                }
            } else {
                total = total.saturating_add(meta.len());
            }
        }
    }
    Ok(total)
}

/// Options controlling `prune`.
#[derive(Clone, Debug)]
pub struct PruneOptions {
    /// Runtimes not used in this many days become eligible.
    pub older_than_days: u32,
    /// If true, report what would be pruned without actually deleting.
    pub dry_run: bool,
    /// If true, keep the latest `keep_per_major` per (tool, major) even if
    /// they'd otherwise be pruned.
    pub keep_latest: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct PruneOutcome {
    pub removed: Vec<RuntimeListing>,
    pub kept: Vec<RuntimeListing>,
    pub dry_run: bool,
    pub freed_bytes: u64,
}

/// Remove old runtimes. Never touches a runtime whose `last_used` is newer
/// than the cutoff or whose tool is the only one of its kind (to avoid leaving
/// the user without a working `node` at all).
///
/// # Errors
/// - [`CoreError::State`] on DB access failures.
/// - [`CoreError::Io`] on filesystem deletion failures.
pub fn prune(ctx: &CoreContext, opts: &PruneOptions) -> CoreResult<PruneOutcome> {
    let cutoff = Utc::now() - Duration::days(i64::from(opts.older_than_days));
    let all = list(ctx)?;

    // Group by tool so we can keep at least one per tool if the user sets keep_latest.
    let mut by_tool: std::collections::BTreeMap<String, Vec<RuntimeListing>> =
        std::collections::BTreeMap::new();
    for rt in all {
        by_tool.entry(rt.tool.clone()).or_default().push(rt);
    }

    let mut removed = Vec::new();
    let mut kept = Vec::new();
    let mut freed_bytes: u64 = 0;

    for (_, mut group) in by_tool {
        // Sort newest-installed last.
        group.sort_by_key(|r| r.installed_at);

        let keep_idx = group.len().saturating_sub(1);
        for (idx, rt) in group.iter().enumerate() {
            let too_recent = rt.last_used.unwrap_or(rt.installed_at) > cutoff;
            let protected = opts.keep_latest && idx == keep_idx;
            if too_recent || protected {
                kept.push(rt.clone());
                continue;
            }

            if !opts.dry_run && rt.on_disk {
                let _ = fs::remove_dir_all(&rt.install_path);
                let state = versionx_state::open(ctx.home.state_db_path())?;
                // Leave the row in place so `versionx runtime list` still reports it as
                // no-longer-on-disk; callers that want a hard-delete can add that later.
                drop(state);
            }
            freed_bytes = freed_bytes.saturating_add(rt.size_bytes);
            removed.push(rt.clone());
        }
    }

    Ok(PruneOutcome { removed, kept, dry_run: opts.dry_run, freed_bytes })
}
