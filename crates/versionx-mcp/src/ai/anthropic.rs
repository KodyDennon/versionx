//! Anthropic Messages API client.
//!
//! Targets `/v1/messages` with API version `2023-06-01` (the current
//! stable header as of the 0.6 cut). Model ids flow straight through —
//! users who want `claude-sonnet-4-6` or newer just put that in the
//! config.

use serde_json::json;

use super::{Prompt, Provider, ProviderConfig, http_client, require_api_key};
use crate::{McpError, McpResult};

#[derive(Debug)]
pub struct AnthropicProvider<'a> {
    cfg: &'a ProviderConfig,
}

impl<'a> AnthropicProvider<'a> {
    pub fn new(cfg: &'a ProviderConfig) -> Self {
        Self { cfg }
    }
}

#[async_trait::async_trait]
impl Provider for AnthropicProvider<'_> {
    async fn complete(&self, prompt: &Prompt) -> McpResult<String> {
        let key = require_api_key(self.cfg)?;
        let client = http_client()?;
        let body = json!({
            "model": self.cfg.model,
            "max_tokens": prompt.max_tokens,
            "temperature": prompt.temperature,
            "system": prompt.system,
            "messages": [
                {"role": "user", "content": prompt.user}
            ]
        });
        let resp = client
            .post(self.cfg.endpoint())
            .header("x-api-key", key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| McpError::Provider(format!("anthropic request: {e}")))?;
        let status = resp.status();
        let raw =
            resp.text().await.map_err(|e| McpError::Provider(format!("anthropic body: {e}")))?;
        if !status.is_success() {
            return Err(McpError::Provider(format!("anthropic HTTP {status}: {raw}")));
        }
        let parsed: serde_json::Value = serde_json::from_str(&raw)
            .map_err(|e| McpError::Provider(format!("anthropic JSON: {e}")))?;
        // Messages API shape: content is an array of blocks, first text block
        // is our answer.
        let text = parsed["content"]
            .as_array()
            .and_then(|a| a.iter().find(|b| b["type"] == "text"))
            .and_then(|b| b["text"].as_str())
            .ok_or_else(|| McpError::Provider("anthropic response missing text block".into()))?;
        Ok(text.to_string())
    }
}
