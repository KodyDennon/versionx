//! Build a [`ReleasePlan`] from the current workspace state.
//!
//! Input:
//!   - Workspace root (for discovery + lockfile).
//!   - Strategy (`"pr-title"` | `"conventional"` | `"manual"`).
//!   - Optional commit messages / PR title (for level inference).
//!   - Optional `[[release.groups]]` config for lockstep bundles.
//!
//! Output: an unapproved [`ReleasePlan`] with:
//!   - `pre_requisite_hash` capturing the current lockfile state.
//!   - A bump per dirty component (direct change) + per transitive
//!     dependent (cascade), escalated through any lockstep groups.
//!   - Severity inferred from commit messages (Conventional strategy)
//!     or PR title (PR-title strategy), defaulting to Patch when no
//!     signal is available.
//!
//! This module is pure logic — no filesystem writes. Call
//! [`ReleasePlan::save`] after `propose` to persist to disk.

use camino::{Utf8Path, Utf8PathBuf};
use chrono::Duration;
use indexmap::IndexMap;
use versionx_workspace::{ComponentGraph, ComponentId, Workspace, discovery, hash};

use crate::conventional::{BumpLevel, aggregate_commits, parse_pr_title};
use crate::plan::{BumpReason, PlannedBump, ReleasePlan, lockfile_hash};

/// A release-group membership (trimmed copy of the config type — keeps
/// this crate decoupled from `versionx-config`'s full schema).
#[derive(Clone, Debug)]
pub struct ReleaseGroup {
    pub name: String,
    pub members: Vec<String>,
    /// `"lockstep"` (all members get the same bump level) or
    /// `"independent"` (members share a plan but keep independent levels).
    pub mode: String,
}

/// Knobs for [`propose`].
#[derive(Clone, Debug, Default)]
pub struct ProposeInput {
    /// `"pr-title"` | `"conventional"` | `"manual"`. Drives how we infer
    /// the per-component bump level from commit/PR metadata.
    pub strategy: String,
    /// Commit messages since the last release. Used for the Conventional
    /// Commits strategy.
    pub commit_messages: Vec<String>,
    /// PR title. Used for the PR-title strategy.
    pub pr_title: Option<String>,
    /// Lockstep / independent groupings.
    pub groups: Vec<ReleaseGroup>,
    /// Plan TTL. Defaults to 24h (the spec default) when None.
    pub ttl: Option<Duration>,
}

#[derive(Debug, thiserror::Error)]
pub enum ProposeError {
    #[error("workspace discovery failed at {path}: {message}")]
    Discovery { path: Utf8PathBuf, message: String },
    #[error("graph build failed: {0}")]
    Graph(String),
    #[error("content hashing failed at {path}: {message}")]
    Hash { path: Utf8PathBuf, message: String },
}

pub type ProposeResult<T> = Result<T, ProposeError>;

/// Build a plan. Caller is expected to read `last_hashes` out of the
/// lockfile (via `versionx-core::commands::workspace::load_last_hashes`)
/// so this module stays decoupled from lockfile I/O.
pub fn propose(
    workspace_root: &Utf8Path,
    last_hashes: &IndexMap<String, String>,
    input: &ProposeInput,
) -> ProposeResult<ReleasePlan> {
    let ws = discovery::discover(workspace_root).map_err(|e| ProposeError::Discovery {
        path: workspace_root.to_path_buf(),
        message: e.to_string(),
    })?;
    let graph = ComponentGraph::build(&ws).map_err(|e| ProposeError::Graph(e.to_string()))?;

    let baseline_level = infer_baseline_level(input);
    let direct = detect_direct_changes(&ws, last_hashes, baseline_level)?;
    let cascade = cascade_through_graph(&graph, &direct);
    let (direct, cascade) = apply_group_lockstep(input.groups.as_slice(), direct, cascade);

    let mut bumps = materialize(&ws, &direct, &cascade);
    // Stable output: topo order (leaves first).
    let topo: Vec<String> = graph.topo_order().into_iter().map(|c| c.to_string()).collect();
    bumps.sort_by_key(|b| topo.iter().position(|s| s == &b.id).unwrap_or(usize::MAX));

    let ttl = input.ttl.unwrap_or_else(|| Duration::hours(24));
    let plan = ReleasePlan::new(
        ws.root.clone(),
        lockfile_hash(&ws.root),
        pick_strategy(&input.strategy),
        bumps,
        ttl,
    );
    Ok(plan)
}

// --------- pipeline stages (each unit-tested independently) -------------

/// Resolve the caller's strategy string to a canonical form. Unknown
/// values fall back to `"manual"` so the plan still records something
/// meaningful without crashing.
fn pick_strategy(raw: &str) -> &'static str {
    match raw {
        "pr-title" => "pr-title",
        "conventional" => "conventional",
        "changesets" => "changesets",
        _ => "manual",
    }
}

/// Infer the baseline level for every dirty component based on commit
/// metadata. Returns [`BumpLevel::Patch`] when no signal is available,
/// which is the conservative default the spec calls for.
fn infer_baseline_level(input: &ProposeInput) -> BumpLevel {
    match input.strategy.as_str() {
        "conventional" => {
            let msgs: Vec<&str> = input.commit_messages.iter().map(String::as_str).collect();
            aggregate_commits(&msgs)
        }
        "pr-title" => input.pr_title.as_deref().map(parse_pr_title).unwrap_or(BumpLevel::Patch),
        _ => BumpLevel::Patch,
    }
}

/// Hash every component and compare to `last_hashes`. Dirty components
/// get the `baseline_level` as their proposed bump.
fn detect_direct_changes(
    ws: &Workspace,
    last_hashes: &IndexMap<String, String>,
    baseline_level: BumpLevel,
) -> ProposeResult<IndexMap<ComponentId, BumpLevel>> {
    let mut out = IndexMap::new();
    for component in ws.components.values() {
        let current = hash::hash_component(&component.root, &component.inputs).map_err(|e| {
            ProposeError::Hash { path: component.root.clone(), message: e.to_string() }
        })?;
        let prior = last_hashes.get(component.id.as_str());
        if prior.map(String::as_str) != Some(current.as_str()) {
            out.insert(component.id.clone(), baseline_level);
        }
    }
    Ok(out)
}

/// Walk transitive dependents. Cascaded bumps default to Patch —
/// changed-upstream shouldn't force majors downstream unless the group
/// config asks for it.
fn cascade_through_graph(
    graph: &ComponentGraph,
    direct: &IndexMap<ComponentId, BumpLevel>,
) -> IndexMap<ComponentId, (BumpLevel, Vec<String>)> {
    let mut out: IndexMap<ComponentId, (BumpLevel, Vec<String>)> = IndexMap::new();
    for changed in direct.keys() {
        for dependent in graph.transitive_dependents(changed) {
            if direct.contains_key(&dependent) {
                continue;
            }
            out.entry(dependent)
                .and_modify(|(_, sources)| sources.push(changed.to_string()))
                .or_insert((BumpLevel::Patch, vec![changed.to_string()]));
        }
    }
    out
}

/// Escalate members of a `lockstep` group to the highest level any
/// member hit.
fn apply_group_lockstep(
    groups: &[ReleaseGroup],
    mut direct: IndexMap<ComponentId, BumpLevel>,
    mut cascade: IndexMap<ComponentId, (BumpLevel, Vec<String>)>,
) -> (IndexMap<ComponentId, BumpLevel>, IndexMap<ComponentId, (BumpLevel, Vec<String>)>) {
    for group in groups {
        if group.mode != "lockstep" {
            continue;
        }
        let mut group_level: Option<BumpLevel> = None;
        let mut trigger: Option<String> = None;
        for member in &group.members {
            let id = ComponentId::new(member);
            if let Some(level) = direct.get(&id) {
                group_level = Some(group_level.map_or(*level, |g| g.max(*level)));
                if trigger.is_none() {
                    trigger = Some(member.clone());
                }
            }
            if let Some((level, _)) = cascade.get(&id) {
                group_level = Some(group_level.map_or(*level, |g| g.max(*level)));
                if trigger.is_none() {
                    trigger = Some(member.clone());
                }
            }
        }
        let Some(level) = group_level else { continue };
        for member in &group.members {
            let id = ComponentId::new(member);
            if direct.contains_key(&id) {
                direct.insert(id, level);
            } else {
                cascade.insert(id, (level, vec![format!("group:{}", group.name)]));
            }
        }
    }
    (direct, cascade)
}

/// Turn the internal maps into the PlannedBump wire shape.
fn materialize(
    ws: &Workspace,
    direct: &IndexMap<ComponentId, BumpLevel>,
    cascade: &IndexMap<ComponentId, (BumpLevel, Vec<String>)>,
) -> Vec<PlannedBump> {
    let mut out = Vec::with_capacity(direct.len() + cascade.len());
    for (id, level) in direct {
        if let Some(c) = ws.components.get(id) {
            out.push(build(c, *level, BumpReason::DirectChange));
        }
    }
    for (id, (level, sources)) in cascade {
        if let Some(c) = ws.components.get(id) {
            out.push(build(c, *level, BumpReason::Cascaded { from: sources.clone() }));
        }
    }
    out
}

fn build(c: &versionx_workspace::Component, level: BumpLevel, reason: BumpReason) -> PlannedBump {
    let to_version = match &c.version {
        // First-ever release lands at 0.1.0 regardless of computed level.
        None => semver::Version::new(0, 1, 0),
        Some(v) => level.apply(v),
    };
    PlannedBump {
        id: c.id.to_string(),
        kind: c.kind.as_str().to_string(),
        from: c.version.as_ref().map(ToString::to_string),
        to: to_version.to_string(),
        level,
        reason,
        changelog: String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_crate(dir: &Utf8Path, name: &str, version: &str, deps: &[(&str, &str)]) {
        fs::create_dir_all(dir.as_std_path()).unwrap();
        let mut body = format!("[package]\nname = \"{name}\"\nversion = \"{version}\"\n");
        if !deps.is_empty() {
            body.push_str("[dependencies]\n");
            for (d, p) in deps {
                body.push_str(&format!("{d} = {{ path = \"{p}\" }}\n"));
            }
        }
        fs::write(dir.join("Cargo.toml"), body).unwrap();
        fs::write(dir.join("lib.rs"), "// x\n").unwrap();
    }

    #[test]
    fn conventional_feat_drives_minor_bumps() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        write_crate(&root.join("core"), "core", "1.2.3", &[]);

        let plan = propose(
            &root,
            &IndexMap::new(),
            &ProposeInput {
                strategy: "conventional".into(),
                commit_messages: vec!["feat: add thing".into(), "fix: typo".into()],
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(plan.bumps.len(), 1);
        assert_eq!(plan.bumps[0].level, BumpLevel::Minor);
        assert_eq!(plan.bumps[0].to, "1.3.0");
        assert_eq!(plan.strategy, "conventional");
    }

    #[test]
    fn cascade_defaults_to_patch_even_with_major_source() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        write_crate(&root.join("core"), "core", "1.2.3", &[]);
        write_crate(&root.join("app"), "app", "1.2.3", &[("core", "../core")]);

        // Hash both components up-front, then *only* modify core. That
        // way app's last_hash still matches and it's only dirty via
        // cascade — which is what we want to test.
        let ws = discovery::discover(&root).unwrap();
        let mut last = IndexMap::new();
        for c in ws.components.values() {
            last.insert(c.id.to_string(), hash::hash_component(&c.root, &c.inputs).unwrap());
        }
        fs::write(root.join("core/lib.rs"), "// modified\n").unwrap();

        let plan = propose(
            &root,
            &last,
            &ProposeInput {
                strategy: "conventional".into(),
                commit_messages: vec!["feat!: drop v1 core api".into()],
                ..Default::default()
            },
        )
        .unwrap();

        let core = plan.bumps.iter().find(|b| b.id == "core").unwrap();
        let app = plan.bumps.iter().find(|b| b.id == "app").unwrap();
        assert_eq!(core.level, BumpLevel::Major);
        assert_eq!(app.level, BumpLevel::Patch);
        assert!(matches!(&app.reason, BumpReason::Cascaded { .. }));
    }

    #[test]
    fn lockstep_group_escalates_all_members() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        write_crate(&root.join("sdk"), "sdk", "1.0.0", &[]);
        write_crate(&root.join("cli"), "cli", "1.0.0", &[]);

        let plan = propose(
            &root,
            &IndexMap::new(),
            &ProposeInput {
                strategy: "conventional".into(),
                commit_messages: vec!["feat: sdk change".into()],
                groups: vec![ReleaseGroup {
                    name: "cli-sdk".into(),
                    members: vec!["sdk".into(), "cli".into()],
                    mode: "lockstep".into(),
                }],
                ..Default::default()
            },
        )
        .unwrap();
        // Both get the same level (minor).
        assert_eq!(plan.bumps.iter().find(|b| b.id == "sdk").unwrap().level, BumpLevel::Minor);
        assert_eq!(plan.bumps.iter().find(|b| b.id == "cli").unwrap().level, BumpLevel::Minor);
    }

    #[test]
    fn clean_workspace_yields_empty_plan() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        write_crate(&root.join("core"), "core", "1.0.0", &[]);

        let ws = discovery::discover(&root).unwrap();
        let c = ws.components.values().next().unwrap();
        let current = hash::hash_component(&c.root, &c.inputs).unwrap();
        let mut last = IndexMap::new();
        last.insert("core".into(), current);

        let plan = propose(&root, &last, &ProposeInput::default()).unwrap();
        assert!(plan.bumps.is_empty());
    }
}
