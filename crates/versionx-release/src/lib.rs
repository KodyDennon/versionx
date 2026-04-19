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
//! Intentionally out of scope for 0.4 (deferred to 0.5+):
//! - Registry publishing (npm / PyPI / crates.io / RubyGems) and OIDC
//!   trusted-publishing credentials.
//! - Changesets-file workflow (separate strategy).
//! - Rollback / snapshot / prerelease lifecycle.
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
