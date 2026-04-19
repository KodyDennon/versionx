//! SQLite-backed state store for Versionx.
//!
//! Uses `rusqlite` with WAL + `synchronous=NORMAL` + sensible `busy_timeout`.
//! The whole database is rebuildable from `git + versionx.toml + versionx.lock`,
//! so anything stored here is a cache — losing it never compromises
//! correctness (`docs/spec/02-config-and-state-model.md §5`).
//!
//! What lives here in 0.1.0:
//! - `schema_migrations` — migration version tracking.
//! - `repos` — every repo Versionx has touched on this machine.
//! - `runtimes_installed` — toolchain installations with SHA-256 + path.
//! - `repo_runtimes` — which runtime a repo pinned.
//! - `runs` — audit trail for sync/install/release runs.
//!
//! Later schema versions add plans, policies, releases, etc.

#![deny(unsafe_code)]

pub mod error;
pub mod model;

mod migrations;
mod store;

pub use error::{StateError, StateResult};
pub use model::{InstalledRuntime, Repo, Run, RunOutcome};
pub use store::{State, open, open_in_memory};
