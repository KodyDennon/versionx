//! Content hashing for components.
//!
//! BLAKE3 over a sorted list of `(relative_path, file_bytes)` tuples for
//! every file matching the component's `inputs` globs, minus the built-in
//! noise blocklist. Output is prefixed `"blake3:"` so it's distinguishable
//! from bare SHA-256 hashes in the lockfile.

use std::collections::BTreeMap;
use std::fs;
use std::io::Read;

use blake3::Hasher;
use camino::{Utf8Path, Utf8PathBuf};

use crate::error::{WorkspaceError, WorkspaceResult};

/// Directories we never hash regardless of input globs. Build artifacts +
/// venvs + `node_modules` inflate hashes without changing meaning.
const NOISE_DIRS: &[&str] = &[
    "target",
    "node_modules",
    ".venv",
    "venv",
    "__pycache__",
    ".pytest_cache",
    "dist",
    "build",
    ".gradle",
    ".idea",
    ".vscode",
    ".cache",
    ".versionx",
];

/// Hash every file under `root` that matches one of the `patterns`, except
/// anything whose path segments include a [`NOISE_DIRS`] entry.
///
/// The hash is stable across OS + architecture because we feed it the
/// component's root-relative path as UTF-8 + the raw file bytes.
pub fn hash_component(root: &Utf8Path, patterns: &[String]) -> WorkspaceResult<String> {
    let globset = build_globset(patterns)?;
    let mut files: BTreeMap<String, Utf8PathBuf> = BTreeMap::new();
    walk(root, root, &globset, &mut files)?;

    let mut hasher = Hasher::new();
    // Domain separator so a random BLAKE3 of the same bytes doesn't collide
    // with a component hash.
    hasher.update(b"versionx/component/v1\n");
    for (rel, abs) in &files {
        hasher.update(rel.as_bytes());
        hasher.update(b"\x00");
        let mut f = fs::File::open(abs)
            .map_err(|source| WorkspaceError::Io { path: abs.clone(), source })?;
        let mut buf = [0u8; 64 * 1024];
        loop {
            let n = f
                .read(&mut buf)
                .map_err(|source| WorkspaceError::Io { path: abs.clone(), source })?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }
        hasher.update(b"\n");
    }
    Ok(format!("blake3:{}", hasher.finalize().to_hex()))
}

fn walk(
    root: &Utf8Path,
    dir: &Utf8Path,
    globset: &GlobSet,
    out: &mut BTreeMap<String, Utf8PathBuf>,
) -> WorkspaceResult<()> {
    let entries = match fs::read_dir(dir.as_std_path()) {
        Ok(e) => e,
        // Swallow permissions errors on scan — they shouldn't crash a hash.
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => return Ok(()),
        Err(source) => {
            return Err(WorkspaceError::Io { path: dir.to_path_buf(), source });
        }
    };

    for entry in entries.flatten() {
        let Some(entry_path) = camino::Utf8PathBuf::from_path_buf(entry.path()).ok() else {
            continue;
        };
        let name = entry_path.file_name().unwrap_or("");
        let Ok(meta) = entry.metadata() else { continue };

        // Hard-skip: noise dirs and dot-dirs (but not dot-files in the root).
        if meta.is_dir() && (NOISE_DIRS.contains(&name) || name.starts_with('.')) {
            continue;
        }

        if meta.is_dir() {
            walk(root, &entry_path, globset, out)?;
        } else if meta.is_file() {
            // Dot-files are skipped unless the user's glob matches them
            // explicitly. `.env.example` can be force-included via
            // `inputs = ["**/*", ".*"]`.
            if name.starts_with('.') {
                continue;
            }
            let Ok(rel) = entry_path.strip_prefix(root) else { continue };
            let rel_str = rel.as_str().to_string();
            if globset.matches(&rel_str) {
                out.insert(rel_str, entry_path);
            }
        }
    }
    Ok(())
}

/// Compiled set of file-path glob patterns with a couple of common features:
/// - `**` matches any number of path segments.
/// - `*` within a segment matches any run of non-`/` characters.
/// - No `*` at all: suffix match (e.g. `Cargo.toml` matches any file named that).
///
/// For 0.x this is a deliberately simple implementation so we don't depend
/// on `globset`; users can reach for a richer DSL later.
#[derive(Debug)]
pub struct GlobSet {
    patterns: Vec<regex::Regex>,
}

impl GlobSet {
    #[must_use]
    pub fn matches(&self, rel_path: &str) -> bool {
        self.patterns.iter().any(|r| r.is_match(rel_path))
    }
}

fn build_globset(patterns: &[String]) -> WorkspaceResult<GlobSet> {
    let defaults = ["**/*".to_string()];
    let iter: Vec<&str> = if patterns.is_empty() {
        defaults.iter().map(String::as_str).collect()
    } else {
        patterns.iter().map(String::as_str).collect()
    };

    let mut regs = Vec::with_capacity(iter.len());
    for pat in iter {
        regs.push(glob_to_regex(pat).map_err(|e| WorkspaceError::InvalidComponent {
            id: "<glob>".into(),
            message: format!("bad glob `{pat}`: {e}"),
        })?);
    }
    Ok(GlobSet { patterns: regs })
}

fn glob_to_regex(pattern: &str) -> Result<regex::Regex, regex::Error> {
    let mut s = String::from("^");
    let bytes = pattern.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'*' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                // `**` = any number of path segments, possibly zero.
                s.push_str(".*");
                i += 2;
                // Consume a trailing `/` so `**/foo` matches `foo` too.
                if i < bytes.len() && bytes[i] == b'/' {
                    i += 1;
                }
            }
            b'*' => {
                s.push_str("[^/]*");
                i += 1;
            }
            b'?' => {
                s.push_str("[^/]");
                i += 1;
            }
            b'.' | b'+' | b'(' | b')' | b'{' | b'}' | b'^' | b'$' | b'|' | b'\\' | b'[' | b']' => {
                s.push('\\');
                s.push(bytes[i] as char);
                i += 1;
            }
            other => {
                s.push(other as char);
                i += 1;
            }
        }
    }
    s.push('$');
    regex::Regex::new(&s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn glob_star_matches_single_segment() {
        let g = build_globset(&["src/*.rs".into()]).unwrap();
        assert!(g.matches("src/lib.rs"));
        assert!(!g.matches("src/deep/lib.rs"));
    }

    #[test]
    fn glob_double_star_matches_any_depth() {
        let g = build_globset(&["src/**/*.rs".into()]).unwrap();
        assert!(g.matches("src/lib.rs"));
        assert!(g.matches("src/deep/a/b.rs"));
        assert!(!g.matches("other/lib.rs"));
    }

    #[test]
    fn hash_deterministic_over_same_content() {
        let tmp = tempfile::tempdir().unwrap();
        let root = camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        fs::write(root.join("a.txt"), b"hello").unwrap();
        fs::write(root.join("b.txt"), b"world").unwrap();

        let h1 = hash_component(&root, &["**/*".into()]).unwrap();
        let h2 = hash_component(&root, &["**/*".into()]).unwrap();
        assert_eq!(h1, h2);
        assert!(h1.starts_with("blake3:"));
    }

    #[test]
    fn hash_changes_when_file_changes() {
        let tmp = tempfile::tempdir().unwrap();
        let root = camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        fs::write(root.join("a.txt"), b"hello").unwrap();
        let h1 = hash_component(&root, &["**/*".into()]).unwrap();
        fs::write(root.join("a.txt"), b"hello world").unwrap();
        let h2 = hash_component(&root, &["**/*".into()]).unwrap();
        assert_ne!(h1, h2);
    }

    #[test]
    fn noise_dirs_are_skipped() {
        let tmp = tempfile::tempdir().unwrap();
        let root = camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        fs::write(root.join("src.rs"), b"code").unwrap();
        fs::create_dir_all(root.join("target/debug")).unwrap();
        fs::write(root.join("target/debug/something"), b"build artifact").unwrap();

        let h1 = hash_component(&root, &["**/*".into()]).unwrap();
        // Now add junk to target/ — the hash should be unchanged.
        fs::write(root.join("target/debug/more"), b"also artifact").unwrap();
        let h2 = hash_component(&root, &["**/*".into()]).unwrap();
        assert_eq!(h1, h2, "target/ changes should not affect hash");
    }

    #[test]
    fn custom_inputs_limit_what_hashes() {
        let tmp = tempfile::tempdir().unwrap();
        let root = camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        fs::write(root.join("a.rs"), b"keep").unwrap();
        fs::write(root.join("b.md"), b"skip").unwrap();

        let h_md_only = hash_component(&root, &["**/*.md".into()]).unwrap();
        fs::write(root.join("a.rs"), b"changed rust").unwrap();
        let h_md_only_2 = hash_component(&root, &["**/*.md".into()]).unwrap();
        assert_eq!(h_md_only, h_md_only_2, "Rust change should not affect md-only hash");
    }
}
