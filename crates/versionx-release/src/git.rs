//! Git operations for the release pipeline.
//!
//! - [`commit_release`] creates a single commit recording every bump in
//!   the plan. We stage only files known to be touched by the plan
//!   (native manifests + lockfile + changelog), never blindly stage
//!   everything — that would pick up user's unrelated WIP.
//! - [`tag_release`] creates annotated tags for each component. Tag
//!   naming uses the config's `tag_template` (default `v{version}` for
//!   single-component releases, `{id}@v{version}` for monorepo).
//! - [`working_tree_clean_except`] is a pre-flight safety check: refuse
//!   to release if there are changes outside the files we're about to
//!   touch.
//!
//! We use `git2` (libgit2 vendored) rather than shelling out — keeps the
//! release path deterministic and avoids requiring `git` on PATH in
//! container builds.

use camino::{Utf8Path, Utf8PathBuf};
use git2::{IndexAddOption, ObjectType, Repository, Signature};

#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("git error: {0}")]
    Git(#[from] git2::Error),
    #[error("working tree has uncommitted changes outside the release set: {paths:?}")]
    DirtyTree { paths: Vec<String> },
    #[error("no git identity configured — set user.name and user.email or pass signature")]
    NoIdentity,
    #[error("{path} is not inside a git repository")]
    NotARepo { path: Utf8PathBuf },
}

pub type GitResult<T> = Result<T, GitError>;

/// Open the repository that contains `path`.
pub fn open(path: &Utf8Path) -> GitResult<Repository> {
    Repository::discover(path.as_std_path())
        .map_err(|_| GitError::NotARepo { path: path.to_path_buf() })
}

/// Ensure every path outside `allowed_paths` is clean. `allowed_paths`
/// are repo-relative.
pub fn working_tree_clean_except(repo: &Repository, allowed_paths: &[String]) -> GitResult<()> {
    let mut opts = git2::StatusOptions::new();
    opts.include_ignored(false).include_untracked(true).recurse_untracked_dirs(true);
    let statuses = repo.statuses(Some(&mut opts))?;

    let mut dirty: Vec<String> = Vec::new();
    for entry in statuses.iter() {
        let Some(path) = entry.path() else { continue };
        // Skip files we're explicitly about to modify.
        if allowed_paths.iter().any(|a| path == a || path.starts_with(&format!("{a}/"))) {
            continue;
        }
        // WT_NEW is untracked; INDEX_* is already staged; WT_MODIFIED is
        // a tracked modified file. Any of these count as dirty.
        let flags = entry.status();
        if !flags.is_empty() {
            dirty.push(path.to_string());
        }
    }
    if dirty.is_empty() { Ok(()) } else { Err(GitError::DirtyTree { paths: dirty }) }
}

/// Stage the given repo-relative paths + commit. Returns the commit OID
/// as a hex string.
pub fn commit_release(
    repo: &Repository,
    repo_relative_paths: &[String],
    message: &str,
) -> GitResult<String> {
    let signature = resolve_signature(repo)?;
    let mut index = repo.index()?;
    // `add_all` lets us target paths individually and picks up both
    // modifications and new files (for CHANGELOG.md).
    index.add_all(repo_relative_paths.iter(), IndexAddOption::DEFAULT, None)?;
    index.write()?;
    let tree_oid = index.write_tree()?;
    let tree = repo.find_tree(tree_oid)?;

    // Collect existing HEAD (may be unborn in a freshly-init'd repo).
    let parents: Vec<git2::Commit> = match repo.head() {
        Ok(head_ref) => vec![head_ref.peel_to_commit()?],
        Err(_) => Vec::new(),
    };
    let parent_refs: Vec<&git2::Commit> = parents.iter().collect();

    let oid = repo.commit(Some("HEAD"), &signature, &signature, message, &tree, &parent_refs)?;
    Ok(oid.to_string())
}

/// Create an annotated tag at the current HEAD with `name`.
///
/// Idempotent: if a tag with this name already exists pointing at the
/// same commit we leave it alone. If it exists pointing elsewhere we
/// return an error — don't silently overwrite release history.
pub fn tag_release(repo: &Repository, tag_name: &str, message: &str) -> GitResult<String> {
    let signature = resolve_signature(repo)?;
    let head = repo.head()?.peel(ObjectType::Commit)?;
    let head_oid = head.id();

    // Idempotence + safety check.
    if let Ok(existing) = repo.find_reference(&format!("refs/tags/{tag_name}")) {
        let existing_target =
            existing.peel(ObjectType::Commit).map(|o| o.id()).unwrap_or_else(|_| head_oid);
        if existing_target == head_oid {
            return Ok(tag_name.to_string());
        }
        return Err(GitError::Git(git2::Error::from_str(&format!(
            "tag {tag_name} already exists and points at a different commit"
        ))));
    }

    repo.tag(tag_name, &head, &signature, message, false)?;
    Ok(tag_name.to_string())
}

/// Prefer git config user.name + user.email; fall back to an env-var
/// pair (useful in CI).
fn resolve_signature(repo: &Repository) -> GitResult<Signature<'static>> {
    // Try repo-local config first, then global.
    if let Ok(sig) = repo.signature() {
        return Ok(sig.to_owned());
    }
    if let (Ok(name), Ok(email)) =
        (std::env::var("GIT_AUTHOR_NAME"), std::env::var("GIT_AUTHOR_EMAIL"))
    {
        return Signature::now(&name, &email).map_err(GitError::Git);
    }
    Err(GitError::NoIdentity)
}

/// Derive an annotated-tag name from a template and a bumped component.
///
/// Supported placeholders:
/// - `{version}`
/// - `{id}` — the component id
/// - `{package}` — alias for `{id}` (Conventional Commits + cargo-release
///   shipping tag convention)
///
/// Defaults:
/// - For single-component releases: `v{version}`
/// - For monorepo: `{id}@v{version}` (mise-en-place convention)
#[must_use]
pub fn format_tag(template: &str, id: &str, version: &str) -> String {
    template.replace("{version}", version).replace("{id}", id).replace("{package}", id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn make_repo(dir: &camino::Utf8Path) -> Repository {
        let repo = Repository::init(dir.as_std_path()).unwrap();
        {
            let mut cfg = repo.config().unwrap();
            cfg.set_str("user.name", "Tester").unwrap();
            cfg.set_str("user.email", "test@example.com").unwrap();
        }
        repo
    }

    #[test]
    fn format_tag_applies_placeholders() {
        assert_eq!(format_tag("v{version}", "core", "1.2.3"), "v1.2.3");
        assert_eq!(format_tag("{id}@v{version}", "core", "1.2.3"), "core@v1.2.3");
        assert_eq!(format_tag("{package}/{version}", "core", "1.2.3"), "core/1.2.3");
    }

    #[test]
    fn commit_and_tag_end_to_end() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        let repo = make_repo(&root);

        fs::write(root.join("file.txt"), "hello").unwrap();
        let oid = commit_release(&repo, &["file.txt".to_string()], "test: initial").unwrap();
        assert_eq!(oid.len(), 40); // sha1 hex length

        let tag = tag_release(&repo, "v1.0.0", "release v1.0.0").unwrap();
        assert_eq!(tag, "v1.0.0");
        // Idempotent: re-tagging the same commit is a no-op, not an error.
        tag_release(&repo, "v1.0.0", "release v1.0.0").unwrap();
    }

    #[test]
    fn dirty_tree_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        let repo = make_repo(&root);
        fs::write(root.join("tracked.txt"), "a").unwrap();
        commit_release(&repo, &["tracked.txt".into()], "init").unwrap();
        // Create an unrelated untracked file → dirty.
        fs::write(root.join("unrelated.txt"), "b").unwrap();

        let err = working_tree_clean_except(&repo, &["tracked.txt".into()]).unwrap_err();
        match err {
            GitError::DirtyTree { paths } => {
                assert!(paths.iter().any(|p| p == "unrelated.txt"));
            }
            other => panic!("wrong error: {other:?}"),
        }
    }

    #[test]
    fn working_tree_clean_ignores_allowed() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        let repo = make_repo(&root);
        fs::write(root.join("base.txt"), "a").unwrap();
        commit_release(&repo, &["base.txt".into()], "init").unwrap();
        fs::write(root.join("allowed.txt"), "new").unwrap();
        working_tree_clean_except(&repo, &["allowed.txt".into()]).unwrap();
    }
}
