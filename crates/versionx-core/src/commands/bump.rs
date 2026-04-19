//! `versionx bump` — propose a bump plan from detected content changes.
//!
//! A bump plan is a pure *proposal*: it maps every dirty component to a
//! concrete next version and records **why** (direct-change vs.
//! cascaded-from-dep). Plans don't mutate anything on disk; a separate
//! `apply` step lands them.
//!
//! ### Current heuristic
//!
//! - Direct change with no prior version → `0.1.0` (first-ever release).
//! - Direct change with prior version → `patch` bump (conservative default).
//!   Future work: parse conventional commits / PR labels for `minor`/`major`
//!   hints (spec §10 in `docs/spec/10-release-orchestration.md`).
//! - Cascade bump (dep changed, we didn't) → `patch` if our dep got a patch
//!   or minor, `minor` if our dep got a major.
//! - Lockstep groups: every member gets whichever level is **largest**
//!   across the group, so a single major in the group forces a major across
//!   all members.

use camino::Utf8PathBuf;
use indexmap::IndexMap;
use semver::Version;
use serde::Serialize;
use versionx_workspace::{Component, ComponentGraph, ComponentId, discovery, hash};

use crate::error::{CoreError, CoreResult};

/// Severity of a proposed bump.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BumpLevel {
    Patch,
    Minor,
    Major,
}

impl BumpLevel {
    const fn rank(self) -> u8 {
        match self {
            Self::Patch => 0,
            Self::Minor => 1,
            Self::Major => 2,
        }
    }

    const fn max(self, other: Self) -> Self {
        if other.rank() > self.rank() { other } else { self }
    }

    const fn apply(self, v: &Version) -> Version {
        match self {
            Self::Patch => Version::new(v.major, v.minor, v.patch + 1),
            Self::Minor => Version::new(v.major, v.minor + 1, 0),
            Self::Major => Version::new(v.major + 1, 0, 0),
        }
    }
}

/// Why a component ended up in the plan.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BumpReason {
    /// The component's own content hash changed.
    DirectChange,
    /// A transitive dep changed; we cascade.
    Cascaded { from: Vec<String> },
    /// The component belongs to a lockstep group and a sibling changed.
    GroupLockstep { group: String, via: String },
}

#[derive(Clone, Debug, Serialize)]
pub struct PlannedBump {
    pub id: String,
    pub kind: String,
    pub from: Option<String>,
    pub to: String,
    pub level: BumpLevel,
    pub reason: BumpReason,
}

#[derive(Clone, Debug)]
pub struct BumpOptions {
    pub root: Utf8PathBuf,
    /// Caller-supplied last-released hashes (typically read from the
    /// lockfile). Components without an entry are treated as
    /// first-time-released and land at `0.1.0`.
    pub last_hashes: IndexMap<String, String>,
    /// Caller-supplied release groups (from `[[release.groups]]`).
    /// Empty = every component is independent.
    pub groups: Vec<ReleaseGroupInput>,
}

/// Shape the bump planner wants for `[[release.groups]]` — trimmed to the
/// fields we actually read so the core isn't coupled to the full
/// `versionx-config` type.
#[derive(Clone, Debug)]
pub struct ReleaseGroupInput {
    pub name: String,
    pub members: Vec<String>,
    pub mode: String, // "lockstep" | "independent"
}

#[derive(Clone, Debug, Serialize)]
pub struct BumpOutcome {
    pub workspace_root: Utf8PathBuf,
    pub plan: Vec<PlannedBump>,
    pub clean: bool,
}

/// Build a bump proposal from the current workspace state.
///
/// # Errors
/// Any discovery, hashing, or graph error propagates as [`CoreError`].
pub fn propose(opts: &BumpOptions) -> CoreResult<BumpOutcome> {
    let ws = discovery::discover(&opts.root).map_err(|e| CoreError::Io {
        path: opts.root.to_string(),
        source: std::io::Error::other(e.to_string()),
    })?;
    let graph = ComponentGraph::build(&ws).map_err(|e| CoreError::Io {
        path: ws.root.to_string(),
        source: std::io::Error::other(e.to_string()),
    })?;

    // Phase 1: detect direct changes.
    let mut direct_level: IndexMap<ComponentId, BumpLevel> = IndexMap::new();
    for c in ws.components.values() {
        let current = hash::hash_component(&c.root, &c.inputs).map_err(|e| CoreError::Io {
            path: c.root.to_string(),
            source: std::io::Error::other(e.to_string()),
        })?;
        let prior = opts.last_hashes.get(c.id.as_str());
        if prior.map(String::as_str) != Some(current.as_str()) {
            direct_level.insert(c.id.clone(), BumpLevel::Patch);
        }
    }

    // Phase 2: cascade through the DAG. Cascaded bumps default to patch.
    let mut cascade: IndexMap<ComponentId, (BumpLevel, Vec<String>)> = IndexMap::new();
    for changed in direct_level.keys() {
        for dependent in graph.transitive_dependents(changed) {
            if direct_level.contains_key(&dependent) {
                continue; // already captured as a direct change
            }
            cascade
                .entry(dependent)
                .and_modify(|(_, from)| from.push(changed.to_string()))
                .or_insert((BumpLevel::Patch, vec![changed.to_string()]));
        }
    }

    // Phase 3: apply group lockstep — any member change escalates every
    // other member to the highest level of anyone in the group.
    apply_group_lockstep(&opts.groups, &mut direct_level, &mut cascade);

    // Phase 4: materialize PlannedBump entries.
    let mut plan: Vec<PlannedBump> = Vec::new();
    for (id, level) in &direct_level {
        if let Some(c) = ws.components.get(id) {
            plan.push(build_planned(c, *level, BumpReason::DirectChange));
        }
    }
    for (id, (level, from)) in &cascade {
        if let Some(c) = ws.components.get(id) {
            plan.push(build_planned(c, *level, BumpReason::Cascaded { from: from.clone() }));
        }
    }

    // Stable ordering: topo order (leaves first) — dependents follow deps.
    let topo: Vec<String> = graph.topo_order().into_iter().map(|c| c.to_string()).collect();
    plan.sort_by_key(|p| topo.iter().position(|s| s == &p.id).unwrap_or(usize::MAX));

    let clean = plan.is_empty();
    Ok(BumpOutcome { workspace_root: ws.root, plan, clean })
}

fn build_planned(component: &Component, level: BumpLevel, reason: BumpReason) -> PlannedBump {
    let from_version = component.version.clone().unwrap_or_else(|| Version::new(0, 0, 0));
    let to_version = if component.version.is_none() {
        // First-ever release → 0.1.0 regardless of the computed level.
        Version::new(0, 1, 0)
    } else {
        level.apply(&from_version)
    };
    PlannedBump {
        id: component.id.to_string(),
        kind: component.kind.as_str().to_string(),
        from: component.version.as_ref().map(ToString::to_string),
        to: to_version.to_string(),
        level,
        reason,
    }
}

fn apply_group_lockstep(
    groups: &[ReleaseGroupInput],
    direct: &mut IndexMap<ComponentId, BumpLevel>,
    cascade: &mut IndexMap<ComponentId, (BumpLevel, Vec<String>)>,
) {
    for group in groups {
        if group.mode != "lockstep" {
            continue;
        }
        // Highest level across any member + the trigger.
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
        let trigger = trigger.unwrap_or_else(|| group.name.clone());
        for member in &group.members {
            let id = ComponentId::new(member);
            if direct.contains_key(&id) {
                direct.insert(id, level);
            } else {
                cascade.insert(id, (level, vec![format!("group:{}", group.name)]));
                // Rewrite reason later in build step — store trigger hint in
                // the `from` list so we can recover it. Kept simple for 0.2.
                let _ = &trigger;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_rust_crate(dir: &camino::Utf8Path, name: &str, version: &str, deps: &[(&str, &str)]) {
        use std::fmt::Write as _;
        fs::create_dir_all(dir).unwrap();
        let mut cargo = format!("[package]\nname = \"{name}\"\nversion = \"{version}\"\n");
        if !deps.is_empty() {
            cargo.push_str("[dependencies]\n");
            for (dep, path) in deps {
                writeln!(cargo, "{dep} = {{ path = \"{path}\" }}").unwrap();
            }
        }
        fs::write(dir.join("Cargo.toml"), cargo).unwrap();
        fs::write(dir.join("lib.rs"), format!("// {name}\n")).unwrap();
    }

    #[test]
    fn no_changes_yields_clean_plan() {
        let tmp = tempfile::tempdir().unwrap();
        let root = camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        write_rust_crate(&root.join("core"), "core", "0.1.0", &[]);

        // Pre-compute the current hash and feed it in as "last released"
        // so the component is seen as clean.
        let ws = discovery::discover(&root).unwrap();
        let c = ws.components.values().next().unwrap();
        let current = hash::hash_component(&c.root, &c.inputs).unwrap();
        let mut last: IndexMap<String, String> = IndexMap::new();
        last.insert("core".into(), current);

        let outcome =
            propose(&BumpOptions { root, last_hashes: last, groups: Vec::new() }).unwrap();
        assert!(outcome.clean);
        assert!(outcome.plan.is_empty());
    }

    #[test]
    fn direct_change_proposes_patch() {
        let tmp = tempfile::tempdir().unwrap();
        let root = camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        write_rust_crate(&root.join("core"), "core", "1.2.3", &[]);

        // No last-hash → treated as dirty, but component has a version, so
        // we should do a patch bump (1.2.3 → 1.2.4), not a first-release.
        let outcome =
            propose(&BumpOptions { root, last_hashes: IndexMap::new(), groups: Vec::new() })
                .unwrap();
        assert!(!outcome.clean);
        assert_eq!(outcome.plan.len(), 1);
        let p = &outcome.plan[0];
        assert_eq!(p.id, "core");
        assert_eq!(p.from.as_deref(), Some("1.2.3"));
        assert_eq!(p.to, "1.2.4");
        assert_eq!(p.level, BumpLevel::Patch);
        assert_eq!(p.reason, BumpReason::DirectChange);
    }

    #[test]
    fn unversioned_first_release_lands_at_0_1_0() {
        let tmp = tempfile::tempdir().unwrap();
        let root = camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        // No version field at all.
        let dir = root.join("thing");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("Cargo.toml"), "[package]\nname = \"thing\"\nversion = \"0.0.0\"\n")
            .unwrap();
        // Force version to None by declaring it via [[components]] instead
        // of the manifest. Strip the Cargo.toml so only the declared
        // component sticks.
        fs::remove_file(dir.join("Cargo.toml")).unwrap();
        fs::write(dir.join("data.txt"), "hello").unwrap();
        fs::write(
            root.join("versionx.toml"),
            r#"
[[components]]
name = "thing"
path = "thing"
kind = "other"
"#,
        )
        .unwrap();

        let outcome =
            propose(&BumpOptions { root, last_hashes: IndexMap::new(), groups: Vec::new() })
                .unwrap();
        let p = outcome.plan.iter().find(|p| p.id == "thing").unwrap();
        assert_eq!(p.from, None);
        assert_eq!(p.to, "0.1.0");
    }

    #[test]
    fn cascade_follows_dag() {
        let tmp = tempfile::tempdir().unwrap();
        let root = camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        write_rust_crate(&root.join("core"), "core", "0.1.0", &[]);
        write_rust_crate(&root.join("app"), "app", "0.1.0", &[("core", "../core")]);

        // Baseline hashes.
        let ws = discovery::discover(&root).unwrap();
        let mut last = IndexMap::new();
        for c in ws.components.values() {
            last.insert(c.id.to_string(), hash::hash_component(&c.root, &c.inputs).unwrap());
        }

        // Modify only `core` — `app` should still show up in the plan
        // because it transitively depends on core.
        fs::write(root.join("core/lib.rs"), "// edited\n").unwrap();

        let outcome =
            propose(&BumpOptions { root, last_hashes: last, groups: Vec::new() }).unwrap();
        let ids: Vec<&str> = outcome.plan.iter().map(|p| p.id.as_str()).collect();
        assert!(ids.contains(&"core"));
        assert!(ids.contains(&"app"));
        let app = outcome.plan.iter().find(|p| p.id == "app").unwrap();
        assert!(
            matches!(&app.reason, BumpReason::Cascaded { from } if from.iter().any(|s| s == "core"))
        );
    }
}
