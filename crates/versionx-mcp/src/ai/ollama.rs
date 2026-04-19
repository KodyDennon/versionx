//! Ollama local-server client (`/api/chat`).
//!
//! No auth by default. If `api_key_env` is set we pass it as `Bearer`
//! for proxied deployments.

use serde_json::json;

use super::{Prompt, Provider, ProviderConfig, http_client};
use crate::{McpError, McpResult};

#[derive(Debug)]
pub struct OllamaProvider<'a> {
    cfg: &'a ProviderConfig,
}

impl<'a> OllamaProvider<'a> {
    pub fn new(cfg: &'a ProviderConfig) -> Self {
        Self { cfg }
    }
}

#[async_trait::async_trait]
impl Provider for OllamaProvider<'_> {
    async fn complete(&self, prompt: &Prompt) -> McpResult<String> {
        let client = http_client()?;
        let body = json!({
            "model": self.cfg.model,
            "stream": false,
            "options": { "num_predict": prompt.max_tokens, "temperature": prompt.temperature },
            "messages": [
                {"role": "system", "content": prompt.system},
                {"role": "user", "content": prompt.user},
            ]
        });
        let mut req = client.post(self.cfg.endpoint()).json(&body);
        if let Some(key) = self.cfg.api_key() {
            req = req.bearer_auth(key);
        }
        let resp =
            req.send().await.map_err(|e| McpError::Provider(format!("ollama request: {e}")))?;
        let status = resp.status();
        let raw = resp.text().await.map_err(|e| McpError::Provider(format!("ollama body: {e}")))?;
        if !status.is_success() {
            return Err(McpError::Provider(format!("ollama HTTP {status}: {raw}")));
        }
        let parsed: serde_json::Value = serde_json::from_str(&raw)
            .map_err(|e| McpError::Provider(format!("ollama JSON: {e}")))?;
        let text = parsed["message"]["content"]
            .as_str()
            .ok_or_else(|| McpError::Provider("ollama response missing content".into()))?;
        Ok(text.to_string())
    }
}
