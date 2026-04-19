//! Versionx core library — the intent-driven planner shared by every frontend.
//!
//! See `docs/spec/01-architecture-overview.md §3` for the library boundary.
//! Frontends (`versionx-cli`, `versionx-mcp`, `versionx-daemon`, …) never
//! call git, adapters, or ecosystem tools directly. They call functions in
//! this crate, which orchestrates the real work.

#![deny(unsafe_code)]

pub mod adapter_registry;
pub mod commands;
pub mod error;
pub mod paths;
pub mod runtime_registry;

pub use adapter_registry::{AdapterRegistry, registry as adapter_registry};
pub use error::{CoreError, CoreResult};
pub use paths::VersionxHome;
pub use runtime_registry::{RuntimeRegistry, registry};
pub use versionx_events::{Event, EventBus, EventSender, Level};

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
