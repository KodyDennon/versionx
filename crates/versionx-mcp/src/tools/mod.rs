//! Tool implementations.
//!
//! Each tool lives in its own file to keep the dispatch table honest —
//! adding a tool is an `Edit` on `mod.rs` + a new file, not a hunt-for-
//! the-right-place. Tools are workflow-shaped (≤ 10 total) and split into
//! `_plan` / `_apply` variants where they mutate.
//!
//! ### Shape
//!
//! Every tool:
//!   - Takes one JSON `params: serde_json::Value` + shared [`McpContext`].
//!   - Returns a [`ToolOutput`] with structured content + text content.
//!   - Dispatches through [`dispatch`] which audit-logs every call.
//!
//! Tools deliberately avoid side channels. If a tool wants to write, it
//! does so in its own body and returns the on-disk path in the result.
//! Nothing is hidden in server state.

use serde::Serialize;
use serde_json::Value;

use crate::McpResult;
use crate::context::McpContext;

pub mod bump_propose;
pub mod changelog_draft;
pub mod config_read;
pub mod policy_check;
pub mod release_apply;
pub mod release_propose;
pub mod state_read;
pub mod workspace_graph;
pub mod workspace_list;
pub mod workspace_status;

/// Serializable shape returned by every tool. Text + structured JSON.
#[derive(Clone, Debug, Serialize)]
pub struct ToolOutput {
    /// Short human-readable summary (surfaced in MCP clients as a text
    /// block alongside the structured JSON).
    pub summary: String,
    /// Machine-consumable JSON payload.
    pub structured: Value,
    /// True when the tool wanted to flag a failure to the client. The
    /// MCP layer maps this to `isError: true` on the response.
    pub is_error: bool,
}

impl ToolOutput {
    pub fn ok(summary: impl Into<String>, structured: Value) -> Self {
        Self { summary: summary.into(), structured, is_error: false }
    }

    pub fn err(summary: impl Into<String>, structured: Value) -> Self {
        Self { summary: summary.into(), structured, is_error: true }
    }
}

/// Descriptor for the tool registry — what the server advertises in
/// `tools/list`.
#[derive(Clone, Debug, Serialize)]
pub struct ToolDescriptor {
    pub name: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub input_schema: Value,
    /// Tools that mutate on-disk state go here so the MCP client can
    /// present them differently from read-only calls.
    pub mutating: bool,
}

/// The canonical registry. Order matches `versionx`'s own CLI layout.
#[must_use]
pub fn descriptors() -> Vec<ToolDescriptor> {
    vec![
        workspace_list::descriptor(),
        workspace_status::descriptor(),
        workspace_graph::descriptor(),
        bump_propose::descriptor(),
        release_propose::descriptor(),
        release_apply::descriptor(),
        policy_check::descriptor(),
        changelog_draft::descriptor(),
        config_read::descriptor(),
        state_read::descriptor(),
    ]
}

/// Dispatch a tool call by name. Unknown names return a typed error so
/// the MCP transport can surface `-32601 method not found`.
pub async fn dispatch(name: &str, params: Value, ctx: &McpContext) -> McpResult<ToolOutput> {
    match name {
        "workspace_list" => workspace_list::call(params, ctx),
        "workspace_status" => workspace_status::call(params, ctx),
        "workspace_graph" => workspace_graph::call(params, ctx),
        "bump_propose" => bump_propose::call(params, ctx),
        "release_propose" => release_propose::call(params, ctx),
        "release_apply" => release_apply::call(params, ctx),
        "policy_check" => policy_check::call(params, ctx),
        "changelog_draft" => changelog_draft::call(params, ctx).await,
        "config_read" => config_read::call(params, ctx),
        "state_read" => state_read::call(params, ctx),
        other => Err(crate::McpError::UnknownTool { name: other.into() }),
    }
}

/// Helper: pull an optional `root` param, defaulting to the context's
/// workspace root. Used by every workspace-scoped tool so callers can
/// either pass `{}` (use default) or `{"root": "/other/path"}`.
pub(crate) fn resolve_root(params: &Value, ctx: &McpContext) -> camino::Utf8PathBuf {
    params
        .get("root")
        .and_then(|v| v.as_str())
        .map(camino::Utf8PathBuf::from)
        .unwrap_or_else(|| ctx.workspace_root.clone())
}
