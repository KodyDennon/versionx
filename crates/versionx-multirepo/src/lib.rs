//! `versionx-multirepo` — fleet-level orchestration.
//!
//! Scope (0.7):
//!   - `versionx-fleet.toml` schema + discovery.
//!   - Link handlers for the four supported kinds (submodule, subtree,
//!     virtual, ref).
//!   - Cross-ecosystem version translation (SemVer ↔ PEP 440 ↔ RubyGems)
//!     with lossy-conversion warnings.
//!   - Saga release protocol in three modes (independent / gated /
//!     coordinated) with rollback strategies.
//!   - State backup + restore via `refs/versionx/history`.
//!
//! Each module is one responsibility; the CLI threads them together.

#![deny(unsafe_code)]
#![allow(
    clippy::module_name_repetitions,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::too_many_lines,
    clippy::doc_markdown,
    clippy::needless_pass_by_value,
    clippy::map_unwrap_or,
    clippy::option_if_let_else,
    clippy::match_same_arms,
    clippy::redundant_closure_for_method_calls,
    clippy::struct_field_names,
    clippy::missing_const_for_fn,
    clippy::unnecessary_wraps,
    clippy::unused_self,
    clippy::manual_let_else,
    clippy::single_match_else,
    clippy::items_after_statements,
    clippy::cast_possible_wrap,
    clippy::missing_fields_in_debug,
    clippy::manual_range_contains,
    clippy::format_push_string,
    clippy::implicit_clone,
    clippy::unnecessary_to_owned,
    clippy::redundant_clone,
    clippy::useless_conversion
)]

pub mod fleet;
pub mod links;
pub mod saga;
pub mod state;
pub mod translate;

pub use fleet::{FleetConfig, FleetError, FleetResult, Member, ReleaseSet};
pub use links::{
    LinkError, LinkHandler, LinkKind, LinkResult, LinkSpec, LinkStatus, check_updates_all,
    handler_for, sync_all,
};
pub use saga::{
    MemberOutcome, MemberStep, Phase, RollbackStrategy, SagaError, SagaReport, SagaResult, TagInfo,
    run as run_saga,
};
pub use state::{
    BackupManifest, StateError, StateResult, backup, record_release_apply, repair, restore,
};
pub use translate::{Ecosystem, Translated, Warning, from_semver, into_semver};

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
