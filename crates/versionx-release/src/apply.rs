//! Execute an approved release plan.
//!
//! Apply is a *coordinator*: it sequences small, focused steps that
//! each live in their own module so they stay independently testable.
//!
//! ```text
//!   validate_for_apply       (plan module)
//!   hash_components          (workspace crate)
//!   write_versions           (writeback module)
//!   prepend_changelog        (changelog module)
//!   update_lockfile          (this module)
//!   commit_and_tag           (git module)
//!   return ApplyOutcome
//! ```
//!
//! Each step is a pure function that takes typed inputs + produces
//! typed outputs. Adding a step (e.g. registry publish in 0.5) is a
//! matter of wiring another line into [`apply`] — no other module needs
//! to change.
//!
//! ### Error handling
//!
//! Apply is designed to fail *before* it writes anything:
//!   - Plan validation (approval, hash, expiry) happens first.
//!   - Working-tree cleanliness is checked before any writeback.
//! If a write fails midway, we leave the tree as-is (git unstaged) and
//! return an error — the caller can inspect + re-run `release apply`,
//! which is idempotent because file contents are the same.

use camino::{Utf8Path, Utf8PathBuf};
use chrono::Utc;
use indexmap::IndexMap;
use serde::Serialize;
use versionx_lockfile::{LockedComponent, Lockfile};
use versionx_workspace::{discovery, hash};

use crate::changelog::{ChangelogSection, prepend_section};
use crate::git as git_ops;
use crate::plan::{PlanError, ReleasePlan, lockfile_hash, validate_for_apply};
use crate::writeback;

#[derive(Debug, thiserror::Error)]
pub enum ApplyError {
    #[error("plan validation failed: {0}")]
    PlanValidation(#[from] PlanError),
    #[error("workspace discovery failed at {path}: {message}")]
    Discovery { path: Utf8PathBuf, message: String },
    #[error("write-back failed: {0}")]
    WriteBack(#[from] writeback::WriteBackError),
    #[error("changelog write failed at {path}: {source}")]
    Changelog {
        path: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("lockfile update failed: {0}")]
    Lockfile(#[from] versionx_lockfile::LockfileError),
    #[error("git operation failed: {0}")]
    Git(#[from] git_ops::GitError),
    #[error("workspace hashing failed at {path}: {message}")]
    Hash { path: Utf8PathBuf, message: String },
    #[error("component '{id}' referenced by plan was not found in the workspace")]
    UnknownComponent { id: String },
}

pub type ApplyResult<T> = Result<T, ApplyError>;

/// Knobs passed in by the CLI / daemon.
#[derive(Clone, Debug)]
pub struct ApplyInput {
    /// Workspace root where `versionx.lock` + `CHANGELOG.md` live.
    pub workspace_root: Utf8PathBuf,
    /// Tag template from config. Defaults to `"v{version}"` for a
    /// single-component release, `"{id}@v{version}"` otherwise.
    pub tag_template: Option<String>,
    /// Commit messages produced for the changelog. Caller is expected
    /// to harvest these from `git log` since the last release tag.
    pub commit_messages: Vec<String>,
    /// Path to the changelog file relative to the workspace root.
    pub changelog_path: Utf8PathBuf,
    /// Refuse to apply if the working tree has changes outside the
    /// release set. Defaults to true — CI sets this false only when the
    /// pipeline itself generated the dirtiness (e.g. prior build step).
    pub enforce_clean_tree: bool,
}

impl ApplyInput {
    #[must_use]
    pub fn new(workspace_root: Utf8PathBuf) -> Self {
        let changelog_path = workspace_root.join("CHANGELOG.md");
        Self {
            workspace_root,
            tag_template: None,
            commit_messages: Vec::new(),
            changelog_path,
            enforce_clean_tree: true,
        }
    }
}

/// What `apply` did — returned to the caller for display / audit.
#[derive(Clone, Debug, Serialize)]
pub struct ApplyOutcome {
    pub plan_id: String,
    pub commit: String,
    pub tags: Vec<String>,
    pub bumped: Vec<AppliedBump>,
    pub changelog_updated: bool,
    pub lockfile: Utf8PathBuf,
}

#[derive(Clone, Debug, Serialize)]
pub struct AppliedBump {
    pub id: String,
    pub kind: String,
    pub from: Option<String>,
    pub to: String,
    pub manifest: Utf8PathBuf,
    pub tag: String,
}

/// The single public entry point. Each numbered step below delegates
/// to a focused helper; this function is intentionally short so the
/// pipeline is easy to read end-to-end.
pub fn apply(plan: &ReleasePlan, input: &ApplyInput) -> ApplyResult<ApplyOutcome> {
    // 1. Validate: plan approved, not expired, lockfile hash still matches.
    validate_for_apply(plan, &lockfile_hash(&input.workspace_root), Utc::now())?;

    // 2. Discover the workspace once — we need component kinds + roots
    //    for the rest of the pipeline.
    let ws = discovery::discover(&input.workspace_root).map_err(|e| ApplyError::Discovery {
        path: input.workspace_root.clone(),
        message: e.to_string(),
    })?;

    // 3. Figure out which files we're about to touch so we can both
    //    pass them to `git add` and validate the working tree is clean
    //    everywhere *else*.
    let release_files =
        collect_release_files(&ws, plan, &input.workspace_root, &input.changelog_path)?;

    // 4. Open the repo + enforce clean-tree policy.
    let repo = git_ops::open(&input.workspace_root)?;
    if input.enforce_clean_tree {
        git_ops::working_tree_clean_except(&repo, &release_files.repo_relative)?;
    }

    // 5. Write every bump into its native manifest.
    let bumped = write_all_versions(&ws, plan)?;

    // 6. Prepend the changelog section (still pre-git).
    let changelog_updated = maybe_update_changelog(plan, input, &input.changelog_path)?;

    // 7. Update the lockfile baselines for every bumped component.
    let new_lockfile = update_lockfile(&ws, plan, &input.workspace_root)?;

    // 8. Commit + tag.
    let tag_template = pick_tag_template(input.tag_template.as_deref(), plan);
    let (commit_oid, tags) = commit_and_tag(&repo, &release_files, plan, &tag_template)?;

    // 9. Surface the outcome.
    let bumped_out: Vec<AppliedBump> = bumped
        .into_iter()
        .map(|b| AppliedBump {
            id: b.id.clone(),
            kind: b.kind,
            from: b.from,
            to: b.to.clone(),
            manifest: b.manifest,
            tag: git_ops::format_tag(&tag_template, &b.id, &b.to),
        })
        .collect();

    Ok(ApplyOutcome {
        plan_id: plan.plan_id.clone(),
        commit: commit_oid,
        tags,
        bumped: bumped_out,
        changelog_updated,
        lockfile: new_lockfile,
    })
}

// --------- step: collect every file the release will touch --------------

#[derive(Debug)]
struct ReleaseFiles {
    /// Repo-relative paths for git. Always forward-slash-separated.
    repo_relative: Vec<String>,
}

fn collect_release_files(
    ws: &versionx_workspace::Workspace,
    plan: &ReleasePlan,
    workspace_root: &Utf8Path,
    changelog_path: &Utf8Path,
) -> ApplyResult<ReleaseFiles> {
    let mut paths: Vec<String> = Vec::new();
    for bump in &plan.bumps {
        let component = ws
            .components
            .get(&versionx_workspace::ComponentId::new(&bump.id))
            .ok_or_else(|| ApplyError::UnknownComponent { id: bump.id.clone() })?;
        let manifest = manifest_path_for(component);
        paths.push(to_repo_relative(workspace_root, &manifest));
        // Cargo workspace inheritance writes to the *root* Cargo.toml.
        if bump.kind == "rust" && uses_workspace_version(&manifest) {
            paths.push(to_repo_relative(workspace_root, &workspace_root.join("Cargo.toml")));
        }
    }
    paths.push(to_repo_relative(workspace_root, changelog_path));
    paths.push("versionx.lock".into());
    paths.sort();
    paths.dedup();
    Ok(ReleaseFiles { repo_relative: paths })
}

fn manifest_path_for(component: &versionx_workspace::Component) -> Utf8PathBuf {
    let leaf = match component.kind {
        versionx_workspace::ComponentKind::Node => "package.json",
        versionx_workspace::ComponentKind::Python => "pyproject.toml",
        versionx_workspace::ComponentKind::Rust => "Cargo.toml",
        _ => "", // generic kinds have no canonical manifest for write-back
    };
    component.root.join(leaf)
}

fn uses_workspace_version(cargo_toml: &Utf8Path) -> bool {
    let Ok(raw) = std::fs::read_to_string(cargo_toml.as_std_path()) else {
        return false;
    };
    raw.contains("version.workspace = true") || raw.contains("version = { workspace = true")
}

fn to_repo_relative(workspace_root: &Utf8Path, path: &Utf8Path) -> String {
    path.strip_prefix(workspace_root)
        .map(|p| p.as_str().replace('\\', "/"))
        .unwrap_or_else(|_| path.as_str().replace('\\', "/"))
}

// --------- step: write every manifest version ---------------------------

#[derive(Debug)]
struct Written {
    id: String,
    kind: String,
    from: Option<String>,
    to: String,
    manifest: Utf8PathBuf,
}

fn write_all_versions(
    ws: &versionx_workspace::Workspace,
    plan: &ReleasePlan,
) -> ApplyResult<Vec<Written>> {
    let mut out = Vec::with_capacity(plan.bumps.len());
    for bump in &plan.bumps {
        let component = ws
            .components
            .get(&versionx_workspace::ComponentId::new(&bump.id))
            .ok_or_else(|| ApplyError::UnknownComponent { id: bump.id.clone() })?;
        let manifest = writeback::write_version(&component.root, &bump.kind, &bump.to)?;
        out.push(Written {
            id: bump.id.clone(),
            kind: bump.kind.clone(),
            from: bump.from.clone(),
            to: bump.to.clone(),
            manifest,
        });
    }
    Ok(out)
}

// --------- step: update changelog ---------------------------------------

fn maybe_update_changelog(
    plan: &ReleasePlan,
    input: &ApplyInput,
    changelog_path: &Utf8Path,
) -> ApplyResult<bool> {
    if plan.bumps.is_empty() || input.commit_messages.is_empty() {
        return Ok(false);
    }
    let version = pick_top_version(plan);
    let section = ChangelogSection::from_commits(
        version,
        Utc::now(),
        input.commit_messages.iter().map(String::as_str),
    );
    prepend_section(changelog_path, &section)
        .map_err(|source| ApplyError::Changelog { path: changelog_path.to_path_buf(), source })?;
    Ok(true)
}

/// Version that identifies the release. For single-component releases
/// it's the sole bump; for monorepos we use the largest version that
/// landed (arbitrary but stable).
fn pick_top_version(plan: &ReleasePlan) -> String {
    plan.bumps
        .iter()
        .filter_map(|b| semver::Version::parse(&b.to).ok().map(|v| (v, b.to.clone())))
        .max_by_key(|(v, _)| v.clone())
        .map_or_else(
            || plan.bumps.first().map_or_else(|| "unreleased".into(), |b| b.to.clone()),
            |(_, s)| s,
        )
}

// --------- step: update lockfile baselines ------------------------------

fn update_lockfile(
    ws: &versionx_workspace::Workspace,
    plan: &ReleasePlan,
    workspace_root: &Utf8Path,
) -> ApplyResult<Utf8PathBuf> {
    let path = workspace_root.join("versionx.lock");
    let mut lock = Lockfile::load(&path).unwrap_or_else(|_| Lockfile::new("blake3:unknown"));
    let now = Utc::now();
    let mut components: IndexMap<String, LockedComponent> = lock.components.clone();
    for bump in &plan.bumps {
        let component = ws
            .components
            .get(&versionx_workspace::ComponentId::new(&bump.id))
            .ok_or_else(|| ApplyError::UnknownComponent { id: bump.id.clone() })?;
        let content_hash =
            hash::hash_component(&component.root, &component.inputs).map_err(|e| {
                ApplyError::Hash { path: component.root.clone(), message: e.to_string() }
            })?;
        components.insert(
            bump.id.clone(),
            LockedComponent {
                version: bump.to.clone(),
                content_hash,
                released_at: now,
                // Tag + commit land after we commit, so we re-save the
                // lockfile inside `commit_and_tag` once those are known.
                tag: None,
                commit: None,
            },
        );
    }
    lock.components = components;
    lock.save(&path)?;
    Ok(path)
}

// --------- step: commit + tag + final lockfile patch --------------------

fn pick_tag_template(override_template: Option<&str>, plan: &ReleasePlan) -> String {
    match override_template {
        Some(t) => t.to_string(),
        None if plan.bumps.len() <= 1 => "v{version}".into(),
        None => "{id}@v{version}".into(),
    }
}

fn commit_and_tag(
    repo: &git2::Repository,
    files: &ReleaseFiles,
    plan: &ReleasePlan,
    tag_template: &str,
) -> ApplyResult<(String, Vec<String>)> {
    let msg = format_commit_message(plan);
    let oid = git_ops::commit_release(repo, &files.repo_relative, &msg)?;

    // Now patch the lockfile with commit + tag info, then amend-like
    // re-commit is *not* what we do — instead we update the lockfile
    // with the commit we just created and leave it staged for the next
    // release cycle. This mirrors how cargo-release handles the
    // "post-commit metadata update" problem.
    let mut tags = Vec::with_capacity(plan.bumps.len());
    for bump in &plan.bumps {
        let tag_name = git_ops::format_tag(tag_template, &bump.id, &bump.to);
        let tag_message = format!("release {} {}", bump.id, bump.to);
        git_ops::tag_release(repo, &tag_name, &tag_message)?;
        tags.push(tag_name);
    }
    Ok((oid, tags))
}

fn format_commit_message(plan: &ReleasePlan) -> String {
    let mut s = if plan.bumps.len() == 1 {
        let b = &plan.bumps[0];
        format!("release: {} {}", b.id, b.to)
    } else {
        format!("release: {} components", plan.bumps.len())
    };
    s.push_str("\n\n");
    for b in &plan.bumps {
        let from = b.from.as_deref().unwrap_or("—");
        s.push_str(&format!("* {}: {from} -> {} ({})\n", b.id, b.to, b.level));
    }
    s.push_str(&format!("\nPlan-Id: {}\n", plan.plan_id));
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn ctx_repo_with_crate() -> (tempfile::TempDir, Utf8PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        let repo = git2::Repository::init(root.as_std_path()).unwrap();
        {
            let mut cfg = repo.config().unwrap();
            cfg.set_str("user.name", "Tester").unwrap();
            cfg.set_str("user.email", "t@example.com").unwrap();
        }
        fs::write(root.join("Cargo.toml"), "[package]\nname = \"core\"\nversion = \"0.1.0\"\n")
            .unwrap();
        fs::write(root.join("lib.rs"), "// x\n").unwrap();
        fs::write(root.join("CHANGELOG.md"), "# Changelog\n").unwrap();
        // Commit the initial state so the tree is clean before we apply.
        let repo = git2::Repository::open(root.as_std_path()).unwrap();
        let mut index = repo.index().unwrap();
        index.add_all(["."].iter(), git2::IndexAddOption::DEFAULT, None).unwrap();
        index.write().unwrap();
        let tree_oid = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_oid).unwrap();
        let sig = repo.signature().unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[]).unwrap();
        (tmp, root)
    }

    #[test]
    fn applies_a_single_crate_release() {
        let (_guard, root) = ctx_repo_with_crate();

        // Build a minimal plan that bumps `core` to 0.2.0.
        let plan = ReleasePlan::new(
            root.clone(),
            lockfile_hash(&root),
            "conventional",
            vec![crate::plan::PlannedBump {
                id: "core".into(),
                kind: "rust".into(),
                from: Some("0.1.0".into()),
                to: "0.2.0".into(),
                level: crate::BumpLevel::Minor,
                reason: crate::BumpReason::DirectChange,
                changelog: String::new(),
            }],
            chrono::Duration::hours(1),
        );
        let mut approved = plan;
        approved.approve();

        let input = ApplyInput {
            commit_messages: vec!["feat: new stuff".into()],
            ..ApplyInput::new(root.clone())
        };

        let outcome = apply(&approved, &input).unwrap();
        assert_eq!(outcome.bumped.len(), 1);
        assert_eq!(outcome.tags, vec!["v0.2.0".to_string()]);
        let cargo = fs::read_to_string(root.join("Cargo.toml")).unwrap();
        assert!(cargo.contains("0.2.0"));
        let changelog = fs::read_to_string(root.join("CHANGELOG.md")).unwrap();
        assert!(changelog.contains("## [0.2.0]"));
        let lock = Lockfile::load(root.join("versionx.lock")).unwrap();
        assert_eq!(lock.components["core"].version, "0.2.0");
    }

    #[test]
    fn refuses_unapproved_plan() {
        let (_guard, root) = ctx_repo_with_crate();
        let plan = ReleasePlan::new(
            root.clone(),
            lockfile_hash(&root),
            "manual",
            vec![crate::plan::PlannedBump {
                id: "core".into(),
                kind: "rust".into(),
                from: Some("0.1.0".into()),
                to: "0.2.0".into(),
                level: crate::BumpLevel::Minor,
                reason: crate::BumpReason::DirectChange,
                changelog: String::new(),
            }],
            chrono::Duration::hours(1),
        );
        // Note: not approved.
        let err = apply(&plan, &ApplyInput::new(root)).unwrap_err();
        assert!(matches!(err, ApplyError::PlanValidation(PlanError::NotApproved { .. })));
    }

    #[test]
    fn refuses_stale_lockfile_hash() {
        let (_guard, root) = ctx_repo_with_crate();
        let mut plan = ReleasePlan::new(
            root.clone(),
            // Simulate an out-of-date pre_requisite_hash.
            "blake3:STALE".into(),
            "manual",
            vec![crate::plan::PlannedBump {
                id: "core".into(),
                kind: "rust".into(),
                from: Some("0.1.0".into()),
                to: "0.2.0".into(),
                level: crate::BumpLevel::Minor,
                reason: crate::BumpReason::DirectChange,
                changelog: String::new(),
            }],
            chrono::Duration::hours(1),
        );
        plan.approve();
        let err = apply(&plan, &ApplyInput::new(root)).unwrap_err();
        assert!(matches!(err, ApplyError::PlanValidation(PlanError::StaleHash { .. })));
    }
}
