//! Preloaded MCP prompts.
//!
//! Three prompts ship with 0.6, mirroring the demo scenarios in the
//! spec:
//!   - `propose_release` — end-to-end: read workspace, propose plan,
//!     draft changelog, request approval.
//!   - `audit_dependency_freshness` — scan components for stale deps +
//!     summarize risk.
//!   - `remediate_policy_violation` — pull recent `policy_check`
//!     findings and propose fixes / waivers with mandatory rationale.
//!
//! Prompts are deliberately short + reference-oriented. Real instructions
//! live in tool descriptors so a smart client can assemble the workflow
//! without re-prompting.

use serde_json::Value;

#[derive(Clone, Debug)]
pub struct PromptDescriptor {
    pub name: &'static str,
    pub description: &'static str,
    pub arguments: Vec<PromptArgument>,
    pub messages: Vec<PromptMessage>,
}

#[derive(Clone, Debug)]
pub struct PromptArgument {
    pub name: &'static str,
    pub description: &'static str,
    pub required: bool,
}

#[derive(Clone, Debug)]
pub struct PromptMessage {
    pub role: &'static str, // "user" | "assistant"
    pub text: &'static str,
}

#[must_use]
pub fn descriptors() -> Vec<PromptDescriptor> {
    vec![
        PromptDescriptor {
            name: "propose_release",
            description: "Walk the workspace, propose a release plan, draft a voice-aware \
                          CHANGELOG section, and request approval before applying.",
            arguments: vec![PromptArgument {
                name: "strategy",
                description: "Bump strategy (default: conventional)",
                required: false,
            }],
            messages: vec![PromptMessage {
                role: "user",
                text: "I'd like to cut a release. Please:\n\
                           1. Call `workspace_status` to see which components changed.\n\
                           2. Call `bump_propose` to see the proposed bumps.\n\
                           3. Call `release_propose` with `strategy = conventional` to persist \
                              the plan.\n\
                           4. Call `changelog_draft` with the plan's target version + the \
                              commits covered.\n\
                           5. Summarize the plan + draft and wait for my approval before \
                              calling `release_apply`.",
            }],
        },
        PromptDescriptor {
            name: "audit_dependency_freshness",
            description: "Inspect every component's declared dependencies and highlight \
                          obviously-stale ones with a concise risk summary.",
            arguments: vec![],
            messages: vec![PromptMessage {
                role: "user",
                text: "Audit this workspace for dependency freshness. Use `workspace_list` to \
                       enumerate components, review the `depends_on` + manifest versions, and \
                       flag anything older than one major version behind recent releases. \
                       Present findings grouped by component, severity first.",
            }],
        },
        PromptDescriptor {
            name: "remediate_policy_violation",
            description: "Re-run `policy_check` and propose concrete fixes (or mandatory-expiry \
                          waivers) for any Deny findings.",
            arguments: vec![],
            messages: vec![PromptMessage {
                role: "user",
                text: "Run `policy_check` and for each unwaivered Deny finding, propose either \
                       (a) the minimum change needed to fix the violation, or (b) a waiver with \
                       an explicit `expires_at` ≤ 90 days and a concrete reason. Never suggest \
                       a waiver with no rationale.",
            }],
        },
    ]
}

/// Convert a descriptor into the shape MCP's `prompts/get` expects.
pub fn to_prompt_content(d: &PromptDescriptor) -> Value {
    serde_json::json!({
        "description": d.description,
        "messages": d.messages.iter().map(|m| serde_json::json!({
            "role": m.role,
            "content": {
                "type": "text",
                "text": m.text,
            }
        })).collect::<Vec<_>>(),
    })
}
