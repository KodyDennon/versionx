//! `state_read` — mirror of the `versionx://state/*` resources.
//!
//! For 0.6 we expose a simple summary: lockfile content + release
//! plans + policy lockfile. Full state-DB browsing lands in 0.7 once
//! the query API is fleshed out.

use serde_json::{Value, json};

use super::{ToolDescriptor, ToolOutput, resolve_root};
use crate::McpResult;
use crate::context::McpContext;

#[must_use]
pub fn descriptor() -> ToolDescriptor {
    ToolDescriptor {
        name: "state_read",
        title: "Read persisted state",
        description: "Summarize the workspace's on-disk state: lockfile, release plans, policy \
                      lockfile. Returns paths + parsed contents for the files that exist.",
        input_schema: json!({
            "type": "object",
            "properties": { "root": { "type": "string" } },
        }),
        mutating: false,
    }
}

pub fn call(params: Value, ctx: &McpContext) -> McpResult<ToolOutput> {
    let root = resolve_root(&params, ctx);
    let lockfile_path = root.join("versionx.lock");
    let policy_lock_path = root.join("versionx.policy.lock");
    let plans_dir = versionx_release::plans_dir(&root);

    let lockfile = versionx_lockfile::Lockfile::load(&lockfile_path).ok();
    let plans = versionx_release::list_plans(&plans_dir).unwrap_or_default();
    let policy_lock = versionx_policy::PolicyLockfile::load(&policy_lock_path).ok();

    let structured = json!({
        "lockfile_path": lockfile_path.to_string(),
        "lockfile": lockfile,
        "plans_dir": plans_dir.to_string(),
        "plans": plans,
        "policy_lockfile_path": policy_lock_path.to_string(),
        "policy_lockfile": policy_lock,
    });
    let summary = format!(
        "{} plans, lockfile={}, policy-lock={}",
        plans.len(),
        lockfile_path.exists(),
        policy_lock_path.exists()
    );
    Ok(ToolOutput::ok(summary, structured))
}
