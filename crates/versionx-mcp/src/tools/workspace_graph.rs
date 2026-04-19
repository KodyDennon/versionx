//! `workspace_graph` — nodes, edges, topological order.

use serde_json::{Value, json};

use super::{ToolDescriptor, ToolOutput, resolve_root};
use crate::McpResult;
use crate::context::McpContext;

#[must_use]
pub fn descriptor() -> ToolDescriptor {
    ToolDescriptor {
        name: "workspace_graph",
        title: "Dependency graph",
        description: "Intra-workspace dependency DAG. Returns every node, every edge (from → to), \
                      and a topological order (leaves first) safe for release iteration.",
        input_schema: json!({
            "type": "object",
            "properties": { "root": { "type": "string" } },
        }),
        mutating: false,
    }
}

pub fn call(params: Value, ctx: &McpContext) -> McpResult<ToolOutput> {
    let root = resolve_root(&params, ctx);
    let bus = versionx_core::EventBus::new();
    let core_ctx = versionx_core::commands::CoreContext::detect(bus.sender())
        .map_err(|e| crate::McpError::Internal(e.to_string()))?;
    let outcome = versionx_core::commands::workspace::graph(
        &core_ctx,
        &versionx_core::commands::workspace::GraphOptions { root },
    )
    .map_err(|e| crate::McpError::Tool(e.to_string()))?;
    let summary = format!("{} nodes, {} edges", outcome.nodes.len(), outcome.edges.len());
    Ok(ToolOutput::ok(summary, serde_json::to_value(&outcome).unwrap_or(Value::Null)))
}
