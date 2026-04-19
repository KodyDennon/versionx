//! Conventional Commits + PR-title parsing → [`BumpLevel`] inference.
//!
//! We support the subset of [Conventional Commits v1.0](https://www.conventionalcommits.org)
//! that the 0.4 spec calls out:
//!
//! - `type(scope)?: description`
//! - `type!(scope)?: description` — `!` marks a breaking change.
//! - `BREAKING CHANGE:` footer anywhere in the body.
//! - Types mapped to bump levels:
//!   - `feat` → Minor
//!   - `fix`, `perf`, `refactor`, `revert`, `build`, `chore`, `ci`,
//!     `docs`, `style`, `test` → Patch
//!   - Anything with `!` or a `BREAKING CHANGE:` footer → Major
//!
//! Unknown types default to [`BumpLevel::Patch`] so custom types don't
//! silently break the bump math.
//!
//! PR-title bumping additionally accepts bracket markers (`[major]`,
//! `[minor]`, `[patch]`) anywhere in the title — useful for teams that
//! don't use conventional commit syntax strictly.

use std::fmt;

/// Severity of a proposed bump.
#[derive(
    Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum BumpLevel {
    Patch = 0,
    Minor = 1,
    Major = 2,
}

impl BumpLevel {
    /// Return the higher of the two.
    #[must_use]
    pub const fn max(self, other: Self) -> Self {
        if (other as u8) > (self as u8) { other } else { self }
    }

    /// Apply to a semver. First-ever release (`version` is None) is
    /// handled by the caller — this just mutates existing versions.
    #[must_use]
    pub fn apply(self, v: &semver::Version) -> semver::Version {
        match self {
            Self::Patch => semver::Version::new(v.major, v.minor, v.patch + 1),
            Self::Minor => semver::Version::new(v.major, v.minor + 1, 0),
            Self::Major => semver::Version::new(v.major + 1, 0, 0),
        }
    }
}

impl fmt::Display for BumpLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Patch => "patch",
            Self::Minor => "minor",
            Self::Major => "major",
        })
    }
}

/// Parsed Conventional-Commits header (first line) + whether the body
/// contained a `BREAKING CHANGE:` footer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConventionalCommit {
    pub kind: String,
    pub scope: Option<String>,
    pub description: String,
    pub breaking: bool,
}

impl ConventionalCommit {
    /// Map the parsed commit to a bump level.
    #[must_use]
    pub fn bump_level(&self) -> BumpLevel {
        if self.breaking {
            return BumpLevel::Major;
        }
        match self.kind.as_str() {
            "feat" => BumpLevel::Minor,
            _ => BumpLevel::Patch,
        }
    }
}

/// Parse a full commit message (header + optional body).
///
/// Returns [`None`] if the first line doesn't match the
/// `type(scope)?: description` pattern — callers can fall back to the
/// PR-title parser or default to a patch bump.
#[must_use]
pub fn parse_commit(message: &str) -> Option<ConventionalCommit> {
    let mut lines = message.lines();
    let header = lines.next()?.trim();
    let (mut commit, body_breaking_prefix) = parse_header(header)?;

    // Scan the body for `BREAKING CHANGE:` or `BREAKING-CHANGE:` footer.
    // Per the spec these must start a line; we're liberal about
    // whitespace.
    let body: String = lines.collect::<Vec<&str>>().join("\n");
    let body_breaking = body
        .lines()
        .map(str::trim)
        .any(|l| l.starts_with("BREAKING CHANGE:") || l.starts_with("BREAKING-CHANGE:"));

    commit.breaking = commit.breaking || body_breaking || body_breaking_prefix;
    Some(commit)
}

/// Parse just a one-line header (useful for PR titles). Returns the
/// commit + whether `!` was present (indicating breaking change).
#[must_use]
pub fn parse_header(header: &str) -> Option<(ConventionalCommit, bool)> {
    // Strict regex: `type`, optional `(scope)`, optional `!`, `: ` or `:`.
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| {
        // Not using `^` anchored on its own because PR titles sometimes
        // include a leading `[area] ` tag before the conventional prefix.
        regex::Regex::new(
            r"^(?P<type>[a-zA-Z]+)(?:\((?P<scope>[^)]+)\))?(?P<bang>!)?:\s*(?P<desc>.+)$",
        )
        .expect("static regex")
    });
    let caps = re.captures(header)?;
    let kind = caps.name("type")?.as_str().to_lowercase();
    let scope = caps.name("scope").map(|m| m.as_str().to_string());
    let bang = caps.name("bang").is_some();
    let description = caps.name("desc")?.as_str().trim().to_string();
    Some((ConventionalCommit { kind, scope, description, breaking: bang }, bang))
}

/// Parse a PR title. Supports both Conventional-Commits syntax and
/// bracket markers (`[major]`, `[minor]`, `[patch]`). Markers win — if
/// someone writes `feat: add X [major]`, we return Major.
#[must_use]
pub fn parse_pr_title(title: &str) -> BumpLevel {
    let lower = title.to_lowercase();
    if lower.contains("[major]") || lower.contains("[breaking]") {
        return BumpLevel::Major;
    }
    if lower.contains("[minor]") {
        return BumpLevel::Minor;
    }
    if lower.contains("[patch]") {
        return BumpLevel::Patch;
    }
    if let Some((commit, _)) = parse_header(title) {
        return commit.bump_level();
    }
    BumpLevel::Patch
}

/// Fold a slice of commit messages into the highest bump level any one
/// of them requests.
#[must_use]
pub fn aggregate_commits(messages: &[&str]) -> BumpLevel {
    messages
        .iter()
        .filter_map(|m| parse_commit(m))
        .map(|c| c.bump_level())
        .fold(BumpLevel::Patch, BumpLevel::max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feat_maps_to_minor() {
        let c = parse_commit("feat: add dep graph").unwrap();
        assert_eq!(c.bump_level(), BumpLevel::Minor);
    }

    #[test]
    fn fix_maps_to_patch() {
        let c = parse_commit("fix(parser): off-by-one").unwrap();
        assert_eq!(c.scope.as_deref(), Some("parser"));
        assert_eq!(c.bump_level(), BumpLevel::Patch);
    }

    #[test]
    fn bang_means_breaking() {
        let c = parse_commit("feat!: drop node 16").unwrap();
        assert!(c.breaking);
        assert_eq!(c.bump_level(), BumpLevel::Major);
    }

    #[test]
    fn breaking_change_footer_detected() {
        let msg = "feat: new thing\n\nBREAKING CHANGE: rename X to Y";
        let c = parse_commit(msg).unwrap();
        assert!(c.breaking);
        assert_eq!(c.bump_level(), BumpLevel::Major);
    }

    #[test]
    fn non_conventional_returns_none() {
        assert!(parse_commit("just a random commit").is_none());
    }

    #[test]
    fn custom_type_defaults_to_patch() {
        let c = parse_commit("wip: in-progress thing").unwrap();
        assert_eq!(c.bump_level(), BumpLevel::Patch);
    }

    #[test]
    fn pr_title_bracket_marker_wins() {
        assert_eq!(parse_pr_title("feat: add X [major]"), BumpLevel::Major);
        assert_eq!(parse_pr_title("fix: quick fix [minor]"), BumpLevel::Minor);
    }

    #[test]
    fn pr_title_falls_back_to_conventional() {
        assert_eq!(parse_pr_title("feat: x"), BumpLevel::Minor);
        assert_eq!(parse_pr_title("fix: x"), BumpLevel::Patch);
        assert_eq!(parse_pr_title("random"), BumpLevel::Patch);
    }

    #[test]
    fn aggregate_picks_highest() {
        let commits = ["fix: a", "feat: b", "chore: c"];
        assert_eq!(aggregate_commits(&commits), BumpLevel::Minor);

        let with_breaking = ["fix: a", "feat!: drop v1"];
        assert_eq!(aggregate_commits(&with_breaking), BumpLevel::Major);
    }

    #[test]
    fn apply_bumps_version() {
        let v = semver::Version::new(1, 2, 3);
        assert_eq!(BumpLevel::Patch.apply(&v).to_string(), "1.2.4");
        assert_eq!(BumpLevel::Minor.apply(&v).to_string(), "1.3.0");
        assert_eq!(BumpLevel::Major.apply(&v).to_string(), "2.0.0");
    }
}
