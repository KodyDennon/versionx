//! `versionx-release` — the release engine.
//!
//! Scope (0.4):
//! - Parse Conventional Commits / PR titles into [`BumpLevel`].
//! - Build + persist release [`ReleasePlan`]s with blake3 IDs, TTLs, and
//!   lockfile pre-requisite hashes.
//! - Apply: write manifest versions, update the lockfile's
//!   `[components.<id>]` baselines, generate CHANGELOG entries, commit,
//!   and tag.
//!
//! Cross-ecosystem version translation lives in [`translate`] and is
//! applied transparently by [`writeback::write_version`] so a SemVer
//! pre-release like `1.2.3-rc.1` lands in PyPI manifests as
//! `1.2.3rc1` (PEP 440) and in RubyGems specs as `1.2.3.rc.1`.
//!
//! See `docs/spec/05-release-orchestration.md` and
//! `docs/spec/11-version-roadmap.md §0.4.0`.

#![deny(unsafe_code)]
#![allow(
    // Release orchestration has enough branches that some pedantic lints
    // just produce noise with no readability gain.
    clippy::module_name_repetitions,
    clippy::too_many_lines,
    clippy::missing_errors_doc,
    clippy::needless_pass_by_value,
    clippy::format_push_string, // fine for short test fixtures
    clippy::map_unwrap_or,
    clippy::option_if_let_else,
    clippy::match_same_arms,
    clippy::single_char_pattern,
    clippy::type_complexity,
    clippy::missing_const_for_fn,
    clippy::iter_on_single_items,
    clippy::doc_lazy_continuation,
    clippy::doc_markdown
)]

pub mod apply;
pub mod changelog;
pub mod conventional;
pub mod git;
pub mod plan;
pub mod propose;
pub mod publish;
pub mod translate;
pub mod writeback;

pub use apply::{AppliedBump, ApplyError, ApplyInput, ApplyOutcome, ApplyResult, apply};
pub use conventional::{
    BumpLevel, ConventionalCommit, aggregate_commits, parse_commit, parse_pr_title,
};
pub use plan::{
    BumpReason, PlanError, PlanResult, PlannedBump, ReleasePlan, expire_plans, list_plans,
    plans_dir, validate_for_apply,
};
pub use propose::{ProposeError, ProposeInput, ProposeResult, ReleaseGroup, propose};
pub use publish::{
    PublishError, PublishOutcome, PublishResult, Registry, oidc_available,
    publish as publish_component,
};

/// Crate version as declared in `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_matches_cargo() {
        assert!(!VERSION.is_empty());
    }
}
