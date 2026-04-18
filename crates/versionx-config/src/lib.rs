//! `versionx.toml` parsing, schema validation, and zero-config detection.
//!
//! This crate owns three responsibilities:
//!
//! 1. **Types**: strongly-typed representation of the `versionx.toml` schema
//!    (see `docs/spec/02-config-and-state-model.md §2`).
//! 2. **Loading**: reading a config from disk, interpolating environment
//!    variables, and optionally merging `.env` / `.env.local` files.
//! 3. **Detection**: when no `versionx.toml` exists, synthesize an in-memory
//!    config from filesystem signals (`package.json`, `Cargo.toml`, etc.).
//!
//! Writing back out is handled by `versionx-core::commands::init` via
//! `toml_edit`, so we preserve user comments and formatting.

#![deny(unsafe_code)]
// The crate ships pub items for use across workspace frontends; most of the
// internal helpers are `pub` within `pub mod`s that are themselves gated by
// the top-level `pub` re-exports below. Rust's `unreachable_pub` lint flags
// this as "could be pub(crate)" — intentional, suppress.
#![allow(unreachable_pub)]

pub mod detect;
pub mod schema;

mod error;
mod interpolate;
mod loader;
mod workspace;

pub use error::{ConfigError, ConfigResult};
pub use loader::{EffectiveConfig, load, load_from_str};
pub use schema::{
    AdvancedConfig, EcosystemConfig, InheritPolicy, LinksConfig, OutputOverride, ReleaseConfig,
    RuntimeProviders, RuntimesConfig, VersionxConfig, VersionxMetaConfig,
};
pub use workspace::{WorkspaceRoot, detect_workspace_root};
