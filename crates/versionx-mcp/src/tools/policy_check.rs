//! `policy_check` — evaluate policies against the workspace.

use serde_json::{Value, json};
use versionx_policy::{evaluate, load_and_verify};

use super::{ToolDescriptor, ToolOutput, resolve_root};
use crate::McpResult;
use crate::context::McpContext;

#[must_use]
pub fn descriptor() -> ToolDescriptor {
    ToolDescriptor {
        name: "policy_check",
        title: "Evaluate policies",
        description: "Load every policy under `.versionx/policies/` and evaluate against the \
                      current workspace state. Returns findings grouped by severity with \
                      waiver hits resolved.",
        input_schema: json!({
            "type": "object",
            "properties": { "root": { "type": "string" } },
        }),
        mutating: false,
    }
}

pub fn call(params: Value, ctx: &McpContext) -> McpResult<ToolOutput> {
    let root = resolve_root(&params, ctx);
    let set = load_and_verify(&root, &[]).map_err(|e| crate::McpError::Tool(e.to_string()))?;
    // Build a minimal PolicyContext from the workspace; for 0.6 we
    // mirror the CLI's `policy check` minus the runtime-pin read so the
    // MCP tool stays fast + pure.
    let ctx_pc = versionx_policy::PolicyContext::new(root);
    let report = evaluate(&set, &ctx_pc).map_err(|e| crate::McpError::Tool(e.to_string()))?;
    let tally = report.tally();
    let summary = format!(
        "deny={} warn={} info={} waived={}",
        tally.deny, tally.warn, tally.info, tally.waived
    );
    let is_error = report.has_blocking();
    let structured = serde_json::to_value(&report).unwrap_or(Value::Null);
    if is_error {
        Ok(ToolOutput::err(summary, structured))
    } else {
        Ok(ToolOutput::ok(summary, structured))
    }
}
