//! `versionx-policy` — declarative policy engine with sandboxed Luau
//! escape hatch.
//!
//! Scope (0.5):
//!   - 10 built-in rule kinds (runtime_version, dependency_version,
//!     dependency_presence, advisory_block, release_gate, commit_format,
//!     lockfile_integrity, link_freshness, provenance_required, custom).
//!   - Luau sandbox stripping `io`/`os`/`package`/`debug` with CPU +
//!     memory limits.
//!   - Waivers with mandatory `expires_at`, 7-day pre-expiry warning.
//!   - `versionx.policy.lock` pinning inherited content SHAs + sealing
//!     policies so downstream repos can't disable them.
//!
//! See `docs/spec/07-policy-engine.md` and `docs/spec/11-version-roadmap.md §0.5.0`.

#![deny(unsafe_code)]
#![allow(
    clippy::module_name_repetitions,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::needless_pass_by_value,
    clippy::too_many_lines,
    clippy::useless_conversion, // test fixtures intentionally over-use .into() for consistency
    clippy::uninlined_format_args,
    clippy::needless_for_each,
    clippy::missing_const_for_fn,
    clippy::cloned_ref_to_slice_refs,
    clippy::redundant_closure_for_method_calls,
    clippy::doc_markdown,
    clippy::map_unwrap_or,
    clippy::default_trait_access,
    clippy::cast_possible_wrap,
    clippy::field_reassign_with_default,
    clippy::missing_fields_in_debug,
    clippy::manual_range_contains,
    clippy::needless_lifetimes,
    clippy::match_same_arms,
    clippy::double_must_use,
    clippy::unnecessary_wraps,
    clippy::needless_raw_string_hashes,
    clippy::option_if_let_else
)]

pub mod context;
pub mod engine;
pub mod finding;
pub mod lockfile;
pub mod rules;
pub mod sandbox;
pub mod schema;
pub mod waiver;

pub use context::{ContextCommit, ContextComponent, ContextLink, ContextRuntime, PolicyContext};
pub use engine::{
    EngineError, EngineResult, LoadedDocument, PolicySet, default_policies_dir, evaluate,
    evaluate_with_sandbox, load_dir,
};
pub use finding::{Finding, PolicyReport, ReportedFinding, Tally, WaiverHit};
pub use lockfile::{LockedSource, LockfileError, PolicyLockfile, hash_source};
pub use sandbox::{LuauSandbox, SandboxError, SandboxResult};
pub use schema::{
    Policy, PolicyDocument, PolicyKind, PolicyParseError, Scope, Severity, Trigger, Waiver,
    from_toml as parse_policy_toml, to_toml as render_policy_toml,
};
pub use waiver::{WaiverAudit, audit as audit_waivers, match_waiver};

/// Crate version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn version_nonempty() {
        assert!(!VERSION.is_empty());
    }
}
