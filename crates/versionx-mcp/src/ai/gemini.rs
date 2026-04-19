//! Google Gemini client (v1beta / generateContent).
//!
//! The API url embeds the model + action; we build it from
//! `endpoint + /{model}:generateContent?key=…`.

use serde_json::json;

use super::{Prompt, Provider, ProviderConfig, http_client, require_api_key};
use crate::{McpError, McpResult};

#[derive(Debug)]
pub struct GeminiProvider<'a> {
    cfg: &'a ProviderConfig,
}

impl<'a> GeminiProvider<'a> {
    pub fn new(cfg: &'a ProviderConfig) -> Self {
        Self { cfg }
    }
}

#[async_trait::async_trait]
impl Provider for GeminiProvider<'_> {
    async fn complete(&self, prompt: &Prompt) -> McpResult<String> {
        let key = require_api_key(self.cfg)?;
        let client = http_client()?;
        let endpoint =
            format!("{}/{}:generateContent?key={}", self.cfg.endpoint(), self.cfg.model, key);
        // Gemini lacks a dedicated system prompt in the generateContent
        // shape; prepend the system prompt to the user text with a
        // delimiter.
        let combined = format!("{}\n\n---\n\n{}", prompt.system, prompt.user);
        let body = json!({
            "contents": [
                { "parts": [{ "text": combined }] }
            ],
            "generationConfig": {
                "maxOutputTokens": prompt.max_tokens,
                "temperature": prompt.temperature,
            }
        });
        let resp = client
            .post(endpoint)
            .json(&body)
            .send()
            .await
            .map_err(|e| McpError::Provider(format!("gemini request: {e}")))?;
        let status = resp.status();
        let raw = resp.text().await.map_err(|e| McpError::Provider(format!("gemini body: {e}")))?;
        if !status.is_success() {
            return Err(McpError::Provider(format!("gemini HTTP {status}: {raw}")));
        }
        let parsed: serde_json::Value = serde_json::from_str(&raw)
            .map_err(|e| McpError::Provider(format!("gemini JSON: {e}")))?;
        let text = parsed["candidates"][0]["content"]["parts"][0]["text"]
            .as_str()
            .ok_or_else(|| McpError::Provider("gemini response missing text".into()))?;
        Ok(text.to_string())
    }
}
