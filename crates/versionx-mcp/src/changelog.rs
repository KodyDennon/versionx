//! Voice-aware changelog draft generation.
//!
//! Pipeline:
//!   1. Gather voice samples — prior CHANGELOG sections (up to the last
//!      10) + `README.md` first paragraph, if any.
//!   2. Fence every sample as untrusted input.
//!   3. Build an LLM prompt that asks for a single new section in the
//!      same voice + style.
//!   4. Call the configured provider via `ai::drive`.
//!
//! We deliberately don't try to edit CHANGELOG.md from here —
//! `changelog_draft` returns the draft + a path where it's been saved,
//! so a human or a downstream `release_apply` can decide what to do.

use camino::Utf8Path;

use crate::ai::{self, Prompt, ProviderConfig};
use crate::sanitize::fence_untrusted;
use crate::{McpError, McpResult};

/// Produce a draft CHANGELOG section for `version` using `commits` as
/// the raw material + the workspace's prior CHANGELOG/README as voice.
pub async fn draft_section(
    root: &Utf8Path,
    version: &str,
    commits: &[String],
    cfg: &ProviderConfig,
) -> McpResult<String> {
    let prior_samples = gather_voice_samples(root);
    let commit_block = if commits.is_empty() {
        "(no commits supplied)".to_string()
    } else {
        commits.iter().map(|c| format!("- {c}")).collect::<Vec<_>>().join("\n")
    };

    let system = "You are drafting a CHANGELOG entry for a software release. \
                  Use the same voice, cadence, and section structure as the prior samples. \
                  Keep it concise (≤ 15 lines). Emit valid Markdown. \
                  Do not add commentary outside the entry itself.";

    let user = format!(
        "Draft a CHANGELOG section for version **{version}**.\n\n\
         ## Commits since last release\n\n{commits}\n\n\
         ## Prior voice samples\n\n{samples}",
        commits = fence_untrusted(&commit_block),
        samples = fence_untrusted(&prior_samples),
    );

    let prompt = Prompt::new(system, user);
    ai::drive(cfg, &prompt).await
}

/// Collect up to 10 prior changelog sections + README preamble.
fn gather_voice_samples(root: &Utf8Path) -> String {
    let mut parts = Vec::new();

    if let Ok(changelog) = std::fs::read_to_string(root.join("CHANGELOG.md").as_std_path()) {
        let last_sections = slice_last_sections(&changelog, 10);
        if !last_sections.is_empty() {
            parts.push(format!("=== Prior CHANGELOG sections ===\n{last_sections}"));
        }
    }
    if let Ok(readme) = std::fs::read_to_string(root.join("README.md").as_std_path()) {
        let preamble = slice_readme_preamble(&readme, 400);
        if !preamble.is_empty() {
            parts.push(format!("=== README preamble ===\n{preamble}"));
        }
    }
    if parts.is_empty() {
        "(no prior samples available — use neutral Keep-a-Changelog tone)".to_string()
    } else {
        parts.join("\n\n")
    }
}

/// Grab the last `n` `## ` sections from a CHANGELOG. We intentionally
/// don't parse the structure — any section that starts with `## ` and
/// extends to the next `## ` counts.
fn slice_last_sections(changelog: &str, n: usize) -> String {
    // Split on "\n## " anchors and keep the tail `n` sections.
    let mut pieces: Vec<&str> = changelog.split("\n## ").collect();
    if pieces.len() <= 1 {
        return String::new();
    }
    // First piece is the preamble (before any `## ` header); drop it.
    pieces.remove(0);
    let tail_start = pieces.len().saturating_sub(n);
    pieces[tail_start..].iter().map(|p| format!("## {p}")).collect::<Vec<_>>().join("\n\n")
}

fn slice_readme_preamble(readme: &str, max_chars: usize) -> String {
    let trimmed = readme.trim_start();
    let cut = trimmed.char_indices().nth(max_chars).map(|(i, _)| i).unwrap_or(trimmed.len());
    trimmed[..cut].to_string()
}

/// Persist a user-approved draft back into CHANGELOG.md, prepending
/// above the most recent release section. Returns the new CHANGELOG
/// content length.
pub fn commit_draft(root: &Utf8Path, section: &str) -> McpResult<usize> {
    let path = root.join("CHANGELOG.md");
    let existing = std::fs::read_to_string(path.as_std_path()).unwrap_or_default();
    let body = if let Some(idx) = existing.find("\n## ") {
        let (head, tail) = existing.split_at(idx + 1);
        format!("{head}\n{section}\n{tail}")
    } else if existing.trim().is_empty() {
        format!("# Changelog\n\n{section}\n")
    } else if let Some(nl) = existing.find('\n') {
        let (head, tail) = existing.split_at(nl + 1);
        format!("{head}\n{section}\n{tail}")
    } else {
        format!("{section}\n{existing}")
    };
    std::fs::write(path.as_std_path(), &body)
        .map_err(|e| McpError::Internal(format!("writing changelog: {e}")))?;
    Ok(body.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slice_last_sections_picks_tail() {
        let cl = "# Changelog\n\n## [1.0.0]\nold\n\n## [1.1.0]\nmid\n\n## [1.2.0]\nnew\n";
        let tail = slice_last_sections(cl, 2);
        assert!(tail.contains("[1.1.0]"));
        assert!(tail.contains("[1.2.0]"));
        assert!(!tail.contains("[1.0.0]"));
    }

    #[test]
    fn readme_preamble_truncates() {
        let r = "A".repeat(1000);
        let pre = slice_readme_preamble(&r, 100);
        assert_eq!(pre.len(), 100);
    }

    #[test]
    fn commit_draft_prepends_above_prior_section() {
        let tmp = tempfile::tempdir().unwrap();
        let root = camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        std::fs::write(root.join("CHANGELOG.md"), "# Changelog\n\n## [1.0.0]\nold\n").unwrap();
        commit_draft(&root, "## [1.1.0]\nnew").unwrap();
        let body = std::fs::read_to_string(root.join("CHANGELOG.md")).unwrap();
        let new_idx = body.find("[1.1.0]").unwrap();
        let old_idx = body.find("[1.0.0]").unwrap();
        assert!(new_idx < old_idx);
    }
}
