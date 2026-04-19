//! `versionx-daemon` — the `versiond` long-running process + the IPC
//! protocol types + a thin client SDK.
//!
//! Consumers:
//! - The `versiond` binary (ships from this crate) owns the server side.
//! - `versionx-core` uses [`Client`] to route calls through the daemon
//!   when one is running, else falls back to in-process execution.
//! - Any 3rd-party tool can speak our JSON-RPC 2.0 wire format using the
//!   types in [`protocol`] + the length-prefixed codec in [`codec`].
//!
//! See `docs/spec/01-architecture-overview.md` for the overall design
//! and `docs/spec/11-version-roadmap.md §0.3.0` for the scope of this
//! phase.

#![deny(unsafe_code)]
// Clippy settings for this crate. `unused_async` fires on async fns we
// keep async for future-proofing (transport bind/accept may go async
// on Windows). `struct_field_names` is false-positive happy on
// workspace_* fields. The rest are minor style noise.
#![allow(
    clippy::unused_async,
    clippy::struct_field_names,
    clippy::needless_pass_by_value,
    clippy::ref_as_ptr, // platform FFI shims need the cast idiom
    // Stylistic: we deliberately construct short `Duration::from_secs(N)`
    // values with a comment explaining why, rather than using
    // `Duration::from_mins(N)` (less universally familiar).
    clippy::unreadable_literal
)]

pub mod client;
pub mod codec;
pub mod paths;
pub mod pidfile;
pub mod protocol;
pub mod server;
pub mod transport;
pub mod watcher;

pub use client::{Client, ClientError, ClientResult, ServerInfo, is_running};
pub use paths::DaemonPaths;
pub use protocol::{
    ErrorObject, Message, Notification, Request, Response, ResponsePayload, methods, notifications,
};
pub use server::{ServerConfig, run};
