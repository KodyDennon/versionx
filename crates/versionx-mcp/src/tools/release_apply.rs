//! `release_apply` — approve + execute a release plan.
//!
//! Mutating. Writes versions to native manifests, updates the lockfile,
//! creates a git commit + annotated tag per component. Refuses to run
//! against a plan whose `pre_requisite_hash` no longer matches.

use serde_json::{Value, json};
use versionx_release::{ApplyInput, ReleasePlan, apply, plans_dir};

use super::{ToolDescriptor, ToolOutput, resolve_root};
use crate::McpResult;
use crate::context::McpContext;

#[must_use]
pub fn descriptor() -> ToolDescriptor {
    ToolDescriptor {
        name: "release_apply",
        title: "Apply an approved release plan",
        description: "Apply a previously-proposed plan: write versions to manifests, refresh \
                      the lockfile, create release commit + tags. Requires an explicit \
                      `plan_id` — no auto-select. Set `approve = true` to approve in-place \
                      if the plan isn't yet approved.",
        input_schema: json!({
            "type": "object",
            "required": ["plan_id"],
            "properties": {
                "root": { "type": "string" },
                "plan_id": {
                    "type": "string",
                    "description": "Plan id (with or without the 'blake3:' prefix).",
                },
                "approve": {
                    "type": "boolean",
                    "default": false,
                    "description": "Approve the plan before applying.",
                },
                "allow_dirty": {
                    "type": "boolean",
                    "default": false,
                    "description": "Allow the working tree to have unrelated changes.",
                },
                "commit_messages": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Commit messages for the changelog section.",
                },
            },
        }),
        mutating: true,
    }
}

pub fn call(params: Value, ctx: &McpContext) -> McpResult<ToolOutput> {
    let root = resolve_root(&params, ctx);
    let Some(plan_id) = params.get("plan_id").and_then(|v| v.as_str()) else {
        return Err(crate::McpError::InvalidParams("`plan_id` is required".into()));
    };
    let approve = params.get("approve").and_then(|v| v.as_bool()).unwrap_or(false);
    let allow_dirty = params.get("allow_dirty").and_then(|v| v.as_bool()).unwrap_or(false);
    let commit_messages: Vec<String> = params
        .get("commit_messages")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|s| s.as_str().map(str::to_string)).collect())
        .unwrap_or_default();

    let dir = plans_dir(&root);
    let mut plan = ReleasePlan::load_by_id(&dir, plan_id)
        .map_err(|e| crate::McpError::Tool(format!("loading plan {plan_id}: {e}")))?;
    if approve {
        plan.approve();
        plan.save(&dir).map_err(|e| crate::McpError::Tool(e.to_string()))?;
    }

    let input =
        ApplyInput { commit_messages, enforce_clean_tree: !allow_dirty, ..ApplyInput::new(root) };
    match apply(&plan, &input) {
        Ok(outcome) => {
            let structured = serde_json::to_value(&outcome).unwrap_or(Value::Null);
            let summary = format!(
                "applied {} — commit {}, {} tags",
                outcome.plan_id,
                &outcome.commit[..outcome.commit.len().min(8)],
                outcome.tags.len()
            );
            Ok(ToolOutput::ok(summary, structured))
        }
        Err(e) => {
            Ok(ToolOutput::err(format!("apply failed: {e}"), json!({ "error": e.to_string() })))
        }
    }
}
