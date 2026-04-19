//! `workspace_status` — per-component change state vs. last release.

use serde_json::{Value, json};

use super::{ToolDescriptor, ToolOutput, resolve_root};
use crate::McpResult;
use crate::context::McpContext;

#[must_use]
pub fn descriptor() -> ToolDescriptor {
    ToolDescriptor {
        name: "workspace_status",
        title: "Workspace change state",
        description: "For every component, compute the BLAKE3 content hash and compare to the \
                      last-released hash in `versionx.lock`. Returns dirty/clean + transitive \
                      cascade of dependents that would need to re-release.",
        input_schema: json!({
            "type": "object",
            "properties": {
                "root": { "type": "string" }
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
    let outcome = versionx_core::commands::workspace::status(
        &core_ctx,
        &versionx_core::commands::workspace::StatusOptions { root },
    )
    .map_err(|e| crate::McpError::Tool(e.to_string()))?;
    let dirty = outcome.components.iter().filter(|c| c.dirty).count();
    let summary = if outcome.any_dirty {
        format!("{dirty} of {} components dirty", outcome.components.len())
    } else {
        "workspace is clean".into()
    };
    Ok(ToolOutput::ok(summary, serde_json::to_value(&outcome).unwrap_or(Value::Null)))
}
