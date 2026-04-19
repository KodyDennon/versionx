//! Git write operations via `git2`.
//!
//! Writes go through libgit2 because gix's write story is still
//! incomplete for our needs (especially push auth + refspec handling).
//! Every write helper is idempotent when possible — re-running after a
//! partial failure should converge, not explode.

use camino::{Utf8Path, Utf8PathBuf};
use git2::{IndexAddOption, ObjectType, Repository, Signature};

#[derive(Debug, thiserror::Error)]
pub enum WriteError {
    #[error("git error: {0}")]
    Git(#[from] git2::Error),
    #[error("no git identity configured (set user.name + user.email)")]
    NoIdentity,
    #[error("{path} is not a git repository")]
    NotARepo { path: Utf8PathBuf },
}

pub type WriteResult<T> = Result<T, WriteError>;

/// Open or fail with [`WriteError::NotARepo`].
pub fn open(repo_path: &Utf8Path) -> WriteResult<Repository> {
    Repository::discover(repo_path.as_std_path())
        .map_err(|_| WriteError::NotARepo { path: repo_path.to_path_buf() })
}

/// Stage + commit the given repo-relative paths. Creates the commit on
/// HEAD (or as the first commit when HEAD is unborn).
pub fn commit(repo: &Repository, paths: &[String], message: &str) -> WriteResult<String> {
    let sig = signature(repo)?;
    let mut index = repo.index()?;
    index.add_all(paths.iter(), IndexAddOption::DEFAULT, None)?;
    index.write()?;
    let tree_oid = index.write_tree()?;
    let tree = repo.find_tree(tree_oid)?;
    let parents: Vec<git2::Commit> = match repo.head() {
        Ok(head) => vec![head.peel_to_commit()?],
        Err(_) => Vec::new(),
    };
    let refs: Vec<&git2::Commit> = parents.iter().collect();
    let oid = repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &refs)?;
    Ok(oid.to_string())
}

/// Create an annotated tag at HEAD. Idempotent: if the tag already
/// points at HEAD, returns Ok. If it points at a different commit,
/// returns an error.
pub fn tag(repo: &Repository, name: &str, message: &str) -> WriteResult<()> {
    let sig = signature(repo)?;
    let head = repo.head()?.peel(ObjectType::Commit)?;
    let head_oid = head.id();
    if let Ok(existing) = repo.find_reference(&format!("refs/tags/{name}")) {
        let existing_oid = existing.peel(ObjectType::Commit).map(|o| o.id()).unwrap_or(head_oid);
        if existing_oid == head_oid {
            return Ok(());
        }
        return Err(WriteError::Git(git2::Error::from_str(&format!(
            "tag {name} already exists and points at a different commit"
        ))));
    }
    repo.tag(name, &head, &sig, message, false)?;
    Ok(())
}

/// Delete a tag by name. Safe to call when the tag is missing.
pub fn delete_tag(repo: &Repository, name: &str) -> WriteResult<()> {
    match repo.tag_delete(name) {
        Ok(()) => Ok(()),
        Err(e) if e.code() == git2::ErrorCode::NotFound => Ok(()),
        Err(e) => Err(WriteError::Git(e)),
    }
}

/// Revert a commit with `git revert --no-edit <commit>` semantics —
/// we create a new commit that undoes the changes. Returns the new
/// revert commit's OID.
pub fn revert_commit(repo: &Repository, commit_sha: &str) -> WriteResult<String> {
    let oid = git2::Oid::from_str(commit_sha)?;
    let target = repo.find_commit(oid)?;
    let mut opts = git2::RevertOptions::new();
    repo.revert(&target, Some(&mut opts))?;

    let mut index = repo.index()?;
    let tree_oid = index.write_tree()?;
    let tree = repo.find_tree(tree_oid)?;
    let head = repo.head()?.peel_to_commit()?;
    let sig = signature(repo)?;
    let message = format!("Revert \"{}\"", target.message().unwrap_or(""));
    let new_oid = repo.commit(Some("HEAD"), &sig, &sig, &message, &tree, &[&head])?;
    Ok(new_oid.to_string())
}

/// Push branches + tags to a remote. Uses the ambient SSH / HTTPS
/// credentials (libgit2's default credential helper).
pub fn push(repo: &Repository, remote: &str, refspecs: &[String]) -> WriteResult<()> {
    let mut remote = repo.find_remote(remote)?;
    let mut cb = git2::RemoteCallbacks::new();
    cb.credentials(default_creds);
    let mut opts = git2::PushOptions::new();
    opts.remote_callbacks(cb);
    let specs: Vec<&str> = refspecs.iter().map(String::as_str).collect();
    remote.push(&specs, Some(&mut opts))?;
    Ok(())
}

fn default_creds(
    url: &str,
    username: Option<&str>,
    _allowed: git2::CredentialType,
) -> Result<git2::Cred, git2::Error> {
    if let Ok(cred) = git2::Cred::ssh_key_from_agent(username.unwrap_or("git")) {
        return Ok(cred);
    }
    if url.starts_with("https") {
        // Try git credential helper via git2's built-in default.
        if let Ok(config) = git2::Config::open_default()
            && let Ok(cred) = git2::Cred::credential_helper(&config, url, username)
        {
            return Ok(cred);
        }
    }
    Err(git2::Error::from_str("no credentials"))
}

/// Hard-reset HEAD to `commit_sha`. Destructive — caller must confirm.
pub fn reset_hard(repo: &Repository, commit_sha: &str) -> WriteResult<()> {
    let oid = git2::Oid::from_str(commit_sha)?;
    let obj = repo.find_object(oid, None)?;
    repo.reset(&obj, git2::ResetType::Hard, None)?;
    Ok(())
}

fn signature(repo: &Repository) -> WriteResult<Signature<'static>> {
    if let Ok(sig) = repo.signature() {
        return Ok(sig.to_owned());
    }
    if let (Ok(name), Ok(email)) =
        (std::env::var("GIT_AUTHOR_NAME"), std::env::var("GIT_AUTHOR_EMAIL"))
    {
        return Signature::now(&name, &email).map_err(WriteError::Git);
    }
    Err(WriteError::NoIdentity)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn fresh_repo() -> (tempfile::TempDir, Utf8PathBuf, Repository) {
        let tmp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        let repo = Repository::init(root.as_std_path()).unwrap();
        let mut cfg = repo.config().unwrap();
        cfg.set_str("user.name", "Tester").unwrap();
        cfg.set_str("user.email", "t@example.com").unwrap();
        (tmp, root, repo)
    }

    #[test]
    fn commit_and_tag_e2e() {
        let (_g, root, repo) = fresh_repo();
        fs::write(root.join("a.txt"), "hi").unwrap();
        let oid = commit(&repo, &["a.txt".into()], "init").unwrap();
        assert_eq!(oid.len(), 40);
        tag(&repo, "v1.0.0", "r").unwrap();
        // Idempotent retry.
        tag(&repo, "v1.0.0", "r").unwrap();
        delete_tag(&repo, "v1.0.0").unwrap();
        // Missing tag delete is a no-op.
        delete_tag(&repo, "v1.0.0").unwrap();
    }

    #[test]
    fn revert_creates_new_commit() {
        let (_g, root, repo) = fresh_repo();
        fs::write(root.join("a.txt"), "1").unwrap();
        let first = commit(&repo, &["a.txt".into()], "first").unwrap();
        fs::write(root.join("a.txt"), "2").unwrap();
        let second = commit(&repo, &["a.txt".into()], "second").unwrap();
        let revert = revert_commit(&repo, &second).unwrap();
        assert_ne!(revert, first);
        assert_ne!(revert, second);
        // The revert should restore "1".
        let body = fs::read_to_string(root.join("a.txt")).unwrap();
        assert_eq!(body, "1");
    }
}
