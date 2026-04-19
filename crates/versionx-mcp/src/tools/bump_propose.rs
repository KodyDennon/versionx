//! `bump_propose` — compute version bumps (no disk writes).

use serde_json::{Value, json};

use super::{ToolDescriptor, ToolOutput, resolve_root};
use crate::McpResult;
use crate::context::McpContext;

#[must_use]
pub fn descriptor() -> ToolDescriptor {
    ToolDescriptor {
        name: "bump_propose",
        title: "Propose version bumps",
        description: "Compute per-component bump levels from content-hash deltas vs. the \
                      lockfile baselines. Does not mutate disk. Use `release_propose` for the \
                      fuller plan artifact.",
        input_schema: json!({
            "type": "object",
            "properties": { "root": { "type": "string" } },
        }),
        mutating: false,
    }
}

pub fn call(params: Value, ctx: &McpContext) -> McpResult<ToolOutput> {
    let root = resolve_root(&params, ctx);
    let last_hashes = versionx_core::commands::workspace::load_last_hashes(&root);
    let outcome =
        versionx_core::commands::bump::propose(&versionx_core::commands::bump::BumpOptions {
            root,
            last_hashes,
            groups: Vec::new(),
        })
        .map_err(|e| crate::McpError::Tool(e.to_string()))?;
    let n = outcome.plan.len();
    let summary = if outcome.clean {
        "no bumps needed — workspace clean".into()
    } else {
        format!("{n} bumps proposed")
    };
    Ok(ToolOutput::ok(summary, serde_json::to_value(&outcome).unwrap_or(Value::Null)))
}
