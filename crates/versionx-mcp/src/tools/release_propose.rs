//! `release_propose` — build and persist a release plan.
//!
//! Writes to `.versionx/plans/<blake3>.toml`. The plan is **unapproved**
//! by default — call `release_apply` (and approve the plan manually) to
//! actually land versions.

use serde_json::{Value, json};
use versionx_release::{ProposeInput, propose};

use super::{ToolDescriptor, ToolOutput, resolve_root};
use crate::McpResult;
use crate::context::McpContext;

#[must_use]
pub fn descriptor() -> ToolDescriptor {
    ToolDescriptor {
        name: "release_propose",
        title: "Propose a release plan",
        description: "Build a release plan from content-hash diffs + optional commit messages. \
                      Persists to `.versionx/plans/<id>.toml`. Unapproved — must be approved by \
                      a human (or the `release_apply` caller) before landing.",
        input_schema: json!({
            "type": "object",
            "properties": {
                "root": { "type": "string" },
                "strategy": {
                    "type": "string",
                    "enum": ["pr-title", "conventional", "manual"],
                    "default": "conventional",
                    "description": "Bump-inference strategy.",
                },
                "pr_title": { "type": "string" },
                "commit_messages": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Commit messages since the last release (for Conventional).",
                },
            },
        }),
        mutating: true,
    }
}

pub fn call(params: Value, ctx: &McpContext) -> McpResult<ToolOutput> {
    let root = resolve_root(&params, ctx);
    let strategy =
        params.get("strategy").and_then(|v| v.as_str()).unwrap_or("conventional").to_string();
    let commit_messages: Vec<String> = params
        .get("commit_messages")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|s| s.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    let pr_title = params.get("pr_title").and_then(|v| v.as_str()).map(str::to_string);

    let last_hashes = versionx_core::commands::workspace::load_last_hashes(&root);
    let plan = propose(
        &root,
        &last_hashes,
        &ProposeInput { strategy, commit_messages, pr_title, groups: Vec::new(), ttl: None },
    )
    .map_err(|e| crate::McpError::Tool(e.to_string()))?;

    let plans_dir = versionx_release::plans_dir(&root);
    let saved_to = plan.save(&plans_dir).map_err(|e| crate::McpError::Tool(e.to_string()))?;

    let structured = json!({
        "plan_id": plan.plan_id,
        "saved_to": saved_to.to_string(),
        "strategy": plan.strategy,
        "approved": plan.approved,
        "expires_at": plan.expires_at,
        "bumps": plan.bumps,
    });
    let summary = format!("plan {} written ({} bumps)", plan.plan_id, plan.bumps.len());
    Ok(ToolOutput::ok(summary, structured))
}
