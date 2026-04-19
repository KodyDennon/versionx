//! `versionx-git` — thin helpers around `gix` (reads) and `git2`
//! (writes).
//!
//! Two halves, strict boundary:
//!   - [`read`]: side-effect-free inspection. Uses `gix` where it's
//!     faster; falls back to `git2` for status/dirty (where gix's
//!     story is still maturing).
//!   - [`write`]: mutation. Uses `git2` exclusively because its write
//!     path is battle-tested and libgit2 handles credential helpers
//!     correctly.
//!
//! [`history`] is a read+write hybrid that maintains
//! `refs/versionx/history` — an append-only git-backed log used by
//! `versionx state` for recovery.

#![deny(unsafe_code)]
#![allow(
    clippy::module_name_repetitions,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::needless_pass_by_value,
    clippy::map_unwrap_or,
    clippy::option_if_let_else,
    clippy::redundant_closure_for_method_calls,
    clippy::missing_const_for_fn,
    clippy::unnecessary_wraps,
    clippy::struct_field_names,
    clippy::too_many_lines,
    clippy::redundant_else
)]

pub mod history;
pub mod read;
pub mod write;

pub use history::{HISTORY_REF, HistoryEvent};
pub use read::{
    LogEntry, ReadError, ReadResult, RemoteInfo, RepoSummary, is_dirty, latest_tag, log_messages,
    summarize,
};
pub use write::{
    WriteError, WriteResult, commit, delete_tag, open, push, reset_hard, revert_commit, tag,
};

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
