//! User-facing command implementations.
//!
//! Each module here owns one `versionx <verb>` subcommand. Frontends
//! (CLI / MCP / daemon RPC) call these functions with already-parsed
//! arguments and receive structured results.

pub mod init;

pub use init::{InitOptions, InitOutcome, init};
