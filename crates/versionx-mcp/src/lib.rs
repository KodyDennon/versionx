//! `versionx-mcp` — MCP server + BYO-API-key clients.
//!
//! Scope (0.6):
//!   - Full ServerHandler impl backed by the official `rmcp` SDK.
//!   - 10 workflow-shaped tools (read-only + `_propose`/`_apply` pairs).
//!   - 3 preloaded prompts.
//!   - 4 resources, each also mirrored as a tool for clients that don't
//!     surface resources.
//!   - BYO-API-key providers: Anthropic, OpenAI, Gemini, Ollama.
//!   - Voice-aware changelog pipeline (README + prior CHANGELOG voice
//!     samples → provider → fenced-output draft).
//!   - Prompt-injection defenses: every user-origin blob that enters a
//!     prompt gets [`sanitize::fence_untrusted`]-wrapped.
//!   - Per-call audit log appended to `.versionx/mcp/audit.ndjson`.
//!
//! See `docs/spec/09-programmatic-and-ai-api.md` and
//! `docs/spec/11-version-roadmap.md §0.6.0`.

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
    clippy::redundant_closure,
    clippy::redundant_closure_for_method_calls,
    clippy::unused_self,
    clippy::missing_const_for_fn,
    clippy::unnecessary_wraps,
    clippy::unused_async,
    clippy::useless_conversion,
    clippy::implicit_clone,
    clippy::unreadable_literal
)]

pub mod ai;
pub mod audit;
pub mod changelog;
pub mod context;
pub mod prompts;
pub mod resources;
pub mod sanitize;
pub mod server;
pub mod tools;

pub use ai::{Provider, ProviderConfig, ProviderKind, drive as drive_provider};
pub use audit::{AuditEntry, AuditLog};
pub use context::McpContext;
pub use server::{VersionxServer, serve_http, serve_stdio};

use rmcp::ErrorData as RpcError;
use rmcp::model::ErrorCode;

#[derive(Debug, thiserror::Error)]
pub enum McpError {
    #[error("unknown tool `{name}`")]
    UnknownTool { name: String },
    #[error("invalid params: {0}")]
    InvalidParams(String),
    #[error("tool failed: {0}")]
    Tool(String),
    #[error("provider error: {0}")]
    Provider(String),
    #[error("internal error: {0}")]
    Internal(String),
}

impl McpError {
    /// Translate into an MCP JSON-RPC error. We deliberately never
    /// surface internal stack traces to clients — just the short
    /// message.
    pub fn into_rpc_error(self) -> RpcError {
        let (code, msg) = match &self {
            Self::UnknownTool { .. } => (ErrorCode::METHOD_NOT_FOUND, self.to_string()),
            Self::InvalidParams(_) => (ErrorCode::INVALID_PARAMS, self.to_string()),
            _ => (ErrorCode::INTERNAL_ERROR, self.to_string()),
        };
        RpcError::new(code, msg, None)
    }
}

pub type McpResult<T> = Result<T, McpError>;

/// Crate version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
