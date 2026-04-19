//! Generate / update CHANGELOG.md entries from Conventional Commits.
//!
//! For the 0.4 ship we produce a Keep-a-Changelog-flavored block per
//! release, grouped by commit `type`:
//!
//! ```markdown
//! ## [1.2.4] — 2026-04-18
//!
//! ### Fixed
//! - parser: off-by-one (#123)
//!
//! ### Added
//! - cli: new `--json` flag (#124)
//! ```
//!
//! Features we intentionally defer to later 0.x releases:
//! - Full git-cliff template support (this is a simple, opinionated
//!   renderer).
//! - Cross-linking to GitHub PRs/issues (we copy the `(#NN)` tail from
//!   commit messages verbatim if present, but don't resolve hyperlinks).
//! - Per-component CHANGELOG files (0.4 writes a single root CHANGELOG).

use std::collections::BTreeMap;
use std::fs;

use camino::Utf8Path;
use chrono::{DateTime, Utc};

use crate::conventional::{ConventionalCommit, parse_commit};

/// One rendered release section, ready to prepend to CHANGELOG.md.
#[derive(Clone, Debug)]
pub struct ChangelogSection {
    pub version: String,
    pub date: DateTime<Utc>,
    /// Group name → ordered list of human-facing lines. Groups are the
    /// Keep-a-Changelog names (Added, Changed, Fixed, …), keyed by the
    /// conventional type.
    pub groups: BTreeMap<&'static str, Vec<String>>,
}

impl ChangelogSection {
    pub fn from_commits<'a, I: IntoIterator<Item = &'a str>>(
        version: impl Into<String>,
        date: DateTime<Utc>,
        commits: I,
    ) -> Self {
        let mut groups: BTreeMap<&'static str, Vec<String>> = BTreeMap::new();
        for msg in commits {
            let Some(parsed) = parse_commit(msg) else { continue };
            let group = group_for(&parsed);
            let line = line_for(&parsed);
            groups.entry(group).or_default().push(line);
        }
        Self { version: version.into(), date, groups }
    }

    /// Render as markdown. Always emits at least a header + date line
    /// even if there are no grouped entries (so the user can see that a
    /// release happened).
    pub fn render(&self) -> String {
        let mut out = String::new();
        let date = self.date.format("%Y-%m-%d");
        out.push_str(&format!("## [{}] — {}\n\n", self.version, date));

        if self.groups.is_empty() {
            out.push_str("_No user-facing changes recorded._\n\n");
            return out;
        }

        // Stable section order — roughly Keep-a-Changelog.
        for (name, entries) in stable_group_order(&self.groups) {
            out.push_str(&format!("### {name}\n"));
            for entry in entries {
                out.push_str(&format!("- {entry}\n"));
            }
            out.push('\n');
        }
        out
    }
}

fn group_for(c: &ConventionalCommit) -> &'static str {
    if c.breaking {
        return "Breaking Changes";
    }
    match c.kind.as_str() {
        "feat" => "Added",
        "fix" => "Fixed",
        "perf" => "Performance",
        "refactor" => "Changed",
        "revert" => "Changed",
        "docs" => "Docs",
        "build" | "ci" | "chore" | "style" | "test" => "Housekeeping",
        _ => "Other",
    }
}

fn line_for(c: &ConventionalCommit) -> String {
    match &c.scope {
        Some(s) => format!("{s}: {}", c.description),
        None => c.description.clone(),
    }
}

fn stable_group_order<'a>(
    groups: &'a BTreeMap<&'static str, Vec<String>>,
) -> Vec<(&'static str, &'a Vec<String>)> {
    const ORDER: &[&str] = &[
        "Breaking Changes",
        "Added",
        "Changed",
        "Fixed",
        "Performance",
        "Docs",
        "Housekeeping",
        "Other",
    ];
    let mut out: Vec<(&'static str, &Vec<String>)> = Vec::new();
    for name in ORDER {
        if let Some(entries) = groups.get(*name) {
            out.push((*name, entries));
        }
    }
    out
}

/// Prepend `section` to `changelog_path`. If the file doesn't exist
/// yet, creates a minimal header + the new section.
pub fn prepend_section(
    changelog_path: &Utf8Path,
    section: &ChangelogSection,
) -> std::io::Result<()> {
    let rendered = section.render();
    let existing = fs::read_to_string(changelog_path.as_std_path()).unwrap_or_default();

    let body = if existing.trim().is_empty() {
        format!("# Changelog\n\n{rendered}")
    } else {
        // Insert after the first `# …` header if present; otherwise just
        // prepend.
        if let Some(split) = existing.find("\n## ") {
            let (head, tail) = existing.split_at(split + 1); // keep the newline
            format!("{head}\n{rendered}{tail}")
        } else if let Some(newline_after_h1) = existing.find('\n') {
            let (head, tail) = existing.split_at(newline_after_h1 + 1);
            format!("{head}\n{rendered}{tail}")
        } else {
            format!("{rendered}{existing}")
        }
    };

    // Atomic write.
    let tmp = changelog_path.with_extension("md.tmp");
    fs::write(tmp.as_std_path(), body)?;
    fs::rename(tmp.as_std_path(), changelog_path.as_std_path())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_by_conventional_type() {
        let section = ChangelogSection::from_commits(
            "1.2.4",
            Utc::now(),
            ["fix: off-by-one", "feat: add X", "chore: deps"],
        );
        let rendered = section.render();
        assert!(rendered.contains("### Added"));
        assert!(rendered.contains("### Fixed"));
        assert!(rendered.contains("### Housekeeping"));
        assert!(rendered.contains("off-by-one"));
    }

    #[test]
    fn breaking_lands_in_its_own_group() {
        let section = ChangelogSection::from_commits("2.0.0", Utc::now(), ["feat!: drop v1"]);
        let rendered = section.render();
        assert!(rendered.contains("### Breaking Changes"));
        assert!(rendered.contains("drop v1"));
    }

    #[test]
    fn empty_commits_still_emit_a_header() {
        let section = ChangelogSection::from_commits("0.1.0", Utc::now(), ["not conventional"]);
        let rendered = section.render();
        assert!(rendered.contains("## [0.1.0]"));
        assert!(rendered.contains("No user-facing"));
    }

    #[test]
    fn prepend_creates_new_changelog() {
        let tmp = tempfile::tempdir().unwrap();
        let p = camino::Utf8PathBuf::from_path_buf(tmp.path().join("CHANGELOG.md")).unwrap();
        let section = ChangelogSection::from_commits("1.0.0", Utc::now(), ["feat: x"]);
        prepend_section(&p, &section).unwrap();
        let body = fs::read_to_string(p.as_std_path()).unwrap();
        assert!(body.starts_with("# Changelog"));
        assert!(body.contains("### Added"));
    }

    #[test]
    fn prepend_inserts_above_existing_section() {
        let tmp = tempfile::tempdir().unwrap();
        let p = camino::Utf8PathBuf::from_path_buf(tmp.path().join("CHANGELOG.md")).unwrap();
        fs::write(
            p.as_std_path(),
            "# Changelog\n\n## [0.9.0] — 2026-01-01\n\n### Added\n- old thing\n",
        )
        .unwrap();
        let section = ChangelogSection::from_commits("1.0.0", Utc::now(), ["feat: new thing"]);
        prepend_section(&p, &section).unwrap();
        let body = fs::read_to_string(p.as_std_path()).unwrap();
        let new_idx = body.find("## [1.0.0]").unwrap();
        let old_idx = body.find("## [0.9.0]").unwrap();
        assert!(new_idx < old_idx, "new section should be above old");
    }
}
