//! `workspace_list` — discover components in the workspace.

use serde_json::{Value, json};

use super::{ToolDescriptor, ToolOutput, resolve_root};
use crate::McpResult;
use crate::context::McpContext;

#[must_use]
pub fn descriptor() -> ToolDescriptor {
    ToolDescriptor {
        name: "workspace_list",
        title: "List workspace components",
        description: "Discover every component (Node package, Rust crate, Python project, \
                      declared asset bundle, …) under the workspace root. Returns ids, kinds, \
                      versions, and intra-workspace dependency edges.",
        input_schema: json!({
            "type": "object",
            "properties": {
                "root": {
                    "type": "string",
                    "description": "Optional workspace root. Defaults to the server's cwd.",
                }
            },
        }),
        mutating: false,
    }
}

pub fn call(params: Value, ctx: &McpContext) -> McpResult<ToolOutput> {
    let root = resolve_root(&params, ctx);
    let bus = versionx_core::EventBus::new();
    let core_ctx = versionx_core::commands::CoreContext::detect(bus.sender())
        .map_err(|e| crate::McpError::Internal(e.to_string()))?;
    let outcome = versionx_core::commands::workspace::list(
        &core_ctx,
        &versionx_core::commands::workspace::ListOptions { root },
    )
    .map_err(|e| crate::McpError::Tool(e.to_string()))?;
    let structured = serde_json::to_value(&outcome).unwrap_or(Value::Null);
    let count = outcome.components.len();
    Ok(ToolOutput::ok(format!("{count} components discovered"), structured))
}
