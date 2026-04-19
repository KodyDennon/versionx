//! OpenAI Chat Completions client.
//!
//! Targets `/v1/chat/completions`. Works against any API-compatible
//! endpoint (Azure OpenAI, openrouter, etc.) via the `endpoint_env`
//! override.

use serde_json::json;

use super::{Prompt, Provider, ProviderConfig, http_client, require_api_key};
use crate::{McpError, McpResult};

#[derive(Debug)]
pub struct OpenAiProvider<'a> {
    cfg: &'a ProviderConfig,
}

impl<'a> OpenAiProvider<'a> {
    pub fn new(cfg: &'a ProviderConfig) -> Self {
        Self { cfg }
    }
}

#[async_trait::async_trait]
impl Provider for OpenAiProvider<'_> {
    async fn complete(&self, prompt: &Prompt) -> McpResult<String> {
        let key = require_api_key(self.cfg)?;
        let client = http_client()?;
        let body = json!({
            "model": self.cfg.model,
            "max_tokens": prompt.max_tokens,
            "temperature": prompt.temperature,
            "messages": [
                {"role": "system", "content": prompt.system},
                {"role": "user", "content": prompt.user},
            ]
        });
        let resp = client
            .post(self.cfg.endpoint())
            .bearer_auth(key)
            .json(&body)
            .send()
            .await
            .map_err(|e| McpError::Provider(format!("openai request: {e}")))?;
        let status = resp.status();
        let raw = resp.text().await.map_err(|e| McpError::Provider(format!("openai body: {e}")))?;
        if !status.is_success() {
            return Err(McpError::Provider(format!("openai HTTP {status}: {raw}")));
        }
        let parsed: serde_json::Value = serde_json::from_str(&raw)
            .map_err(|e| McpError::Provider(format!("openai JSON: {e}")))?;
        let text = parsed["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| McpError::Provider("openai response missing content".into()))?;
        Ok(text.to_string())
    }
}
