//! `changelog_draft` — voice-aware changelog draft via BYO-API-key LLM.
//!
//! Gathers prior samples from `CHANGELOG.md`, the repo README (if any),
//! and the supplied commit messages, then asks the configured provider
//! to produce a draft that matches the project's voice. The draft is
//! returned as structured content + written to
//! `.versionx/mcp/changelog-draft-<ts>.md` so the caller can pick it up.
//!
//! Untrusted content (commit messages, existing changelog prose) is
//! fenced via [`crate::sanitize::fence_untrusted`] in the prompt.

use chrono::Utc;
use serde_json::{Value, json};

use super::{ToolDescriptor, ToolOutput, resolve_root};
use crate::McpResult;
use crate::ai::ProviderConfig;
use crate::changelog;
use crate::context::McpContext;

#[must_use]
pub fn descriptor() -> ToolDescriptor {
    ToolDescriptor {
        name: "changelog_draft",
        title: "Draft a voice-aware changelog section",
        description: "Use a configured BYO-API-key provider to draft a CHANGELOG section for \
                      the next release, matching the repo's prior voice. Does not commit — the \
                      draft is returned + written to `.versionx/mcp/`.",
        input_schema: json!({
            "type": "object",
            "required": ["version"],
            "properties": {
                "root": { "type": "string" },
                "version": {
                    "type": "string",
                    "description": "Next version (e.g. '1.2.4').",
                },
                "commit_messages": {
                    "type": "array",
                    "items": { "type": "string" },
                },
                "provider": {
                    "type": "string",
                    "enum": ["anthropic", "openai", "gemini", "ollama"],
                },
                "model": { "type": "string" },
                "api_key_env": { "type": "string" },
                "endpoint_env": { "type": "string" },
            },
        }),
        mutating: true,
    }
}

pub async fn call(params: Value, ctx: &McpContext) -> McpResult<ToolOutput> {
    let root = resolve_root(&params, ctx);
    let Some(version) = params.get("version").and_then(|v| v.as_str()) else {
        return Err(crate::McpError::InvalidParams("`version` is required".into()));
    };
    let commits: Vec<String> = params
        .get("commit_messages")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|s| s.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    let provider_cfg = ProviderConfig::from_params(&params, &root)?;

    let draft = changelog::draft_section(&root, version, &commits, &provider_cfg)
        .await
        .map_err(|e| crate::McpError::Tool(format!("changelog draft failed: {e}")))?;

    let filename = format!("changelog-draft-{}-{version}.md", Utc::now().format("%Y%m%d%H%M%S"));
    let draft_path = ctx.artifacts_dir.join(&filename);
    std::fs::write(draft_path.as_std_path(), &draft)
        .map_err(|e| crate::McpError::Tool(format!("writing draft: {e}")))?;

    let structured = json!({
        "draft": draft,
        "path": draft_path.to_string(),
        "provider": provider_cfg.provider_name(),
        "model": provider_cfg.model.clone(),
    });
    let summary = format!("draft written ({} bytes) to {draft_path}", draft.len());
    Ok(ToolOutput::ok(summary, structured))
}
