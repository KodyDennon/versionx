//! BYO-API-key LLM clients.
//!
//! The MCP server itself doesn't ship a model. When a user wants AI
//! assistance without an MCP-capable client (e.g. from CI), we fall
//! back to one of four providers, driven by `[release.ai.byo]` in
//! `versionx.toml`:
//!
//! ```toml
//! [release]
//! ai_assist = "byo-api"
//!
//! [release.ai.byo]
//! provider = "anthropic"
//! model = "claude-sonnet-4-6"
//! api_key_env = "ANTHROPIC_API_KEY"
//! ```
//!
//! Each provider is its own module + implements [`Provider`]. `drive`
//! is the single entry point — pick the provider, call the model, get
//! back plain UTF-8 output.
//!
//! No streaming. No function-calling. No parallel tool use. The
//! workflow here is "turn a prompt into a paragraph" — we intentionally
//! keep the API minimal.

use camino::Utf8Path;
use serde_json::Value;

use crate::{McpError, McpResult};

pub mod anthropic;
pub mod gemini;
pub mod ollama;
pub mod openai;

/// A generic prompt envelope. The system prompt sets guardrails;
/// `user` is the concrete request.
#[derive(Clone, Debug)]
pub struct Prompt {
    pub system: String,
    pub user: String,
    /// Max tokens to generate. Providers that don't support this
    /// ignore it.
    pub max_tokens: u32,
    /// Generation temperature 0.0..=1.0. Most providers clamp their
    /// own out-of-range values.
    pub temperature: f32,
}

impl Prompt {
    #[must_use]
    pub fn new(system: impl Into<String>, user: impl Into<String>) -> Self {
        Self { system: system.into(), user: user.into(), max_tokens: 1024, temperature: 0.4 }
    }
}

/// What a provider needs to make a call. Parsed out of
/// `[release.ai.byo]` or explicit tool params.
#[derive(Clone, Debug)]
pub struct ProviderConfig {
    pub provider: ProviderKind,
    pub model: String,
    /// Env var holding the API key.
    pub api_key_env: Option<String>,
    /// Optional endpoint override env var (self-hosted / proxied).
    pub endpoint_env: Option<String>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ProviderKind {
    Anthropic,
    OpenAi,
    Gemini,
    Ollama,
}

impl ProviderKind {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "anthropic" => Some(Self::Anthropic),
            "openai" => Some(Self::OpenAi),
            "gemini" => Some(Self::Gemini),
            "ollama" => Some(Self::Ollama),
            _ => None,
        }
    }
    pub const fn default_api_key_env(self) -> &'static str {
        match self {
            Self::Anthropic => "ANTHROPIC_API_KEY",
            Self::OpenAi => "OPENAI_API_KEY",
            Self::Gemini => "GEMINI_API_KEY",
            Self::Ollama => "OLLAMA_API_KEY", // optional for local
        }
    }
    pub const fn default_endpoint(self) -> &'static str {
        match self {
            Self::Anthropic => "https://api.anthropic.com/v1/messages",
            Self::OpenAi => "https://api.openai.com/v1/chat/completions",
            Self::Gemini => "https://generativelanguage.googleapis.com/v1beta/models",
            Self::Ollama => "http://127.0.0.1:11434/api/chat",
        }
    }
}

impl ProviderConfig {
    /// Build from the explicit `provider/model/...` tool params when
    /// present, falling back to `versionx.toml [release.ai.byo]`.
    pub fn from_params(params: &Value, root: &Utf8Path) -> McpResult<Self> {
        // Inline wins.
        if let Some(provider) = params.get("provider").and_then(|v| v.as_str()) {
            let kind = ProviderKind::parse(provider)
                .ok_or_else(|| McpError::InvalidParams(format!("unknown provider `{provider}`")))?;
            let model = params
                .get("model")
                .and_then(|v| v.as_str())
                .ok_or_else(|| McpError::InvalidParams("provider given without `model`".into()))?
                .to_string();
            return Ok(Self {
                provider: kind,
                model,
                api_key_env: params.get("api_key_env").and_then(|v| v.as_str()).map(str::to_string),
                endpoint_env: params
                    .get("endpoint_env")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
            });
        }

        // Otherwise, read the config.
        let cfg_path = root.join("versionx.toml");
        let raw = std::fs::read_to_string(cfg_path.as_std_path()).map_err(|e| {
            McpError::InvalidParams(format!(
                "no provider in params and couldn't read versionx.toml: {e}"
            ))
        })?;
        let doc: toml::Value = toml::from_str(&raw)
            .map_err(|e| McpError::InvalidParams(format!("versionx.toml parse error: {e}")))?;
        let byo = doc
            .get("release")
            .and_then(|r| r.get("ai"))
            .and_then(|a| a.get("byo"))
            .and_then(|v| v.as_table())
            .ok_or_else(|| {
                McpError::InvalidParams("no [release.ai.byo] and no `provider` param".into())
            })?;
        let provider = byo
            .get("provider")
            .and_then(|v| v.as_str())
            .ok_or_else(|| McpError::InvalidParams("[release.ai.byo] missing `provider`".into()))?;
        let kind = ProviderKind::parse(provider)
            .ok_or_else(|| McpError::InvalidParams(format!("unknown provider `{provider}`")))?;
        let model = byo
            .get("model")
            .and_then(|v| v.as_str())
            .ok_or_else(|| McpError::InvalidParams("[release.ai.byo] missing `model`".into()))?
            .to_string();
        Ok(Self {
            provider: kind,
            model,
            api_key_env: byo.get("api_key_env").and_then(|v| v.as_str()).map(str::to_string),
            endpoint_env: byo.get("endpoint_env").and_then(|v| v.as_str()).map(str::to_string),
        })
    }

    pub fn provider_name(&self) -> &'static str {
        match self.provider {
            ProviderKind::Anthropic => "anthropic",
            ProviderKind::OpenAi => "openai",
            ProviderKind::Gemini => "gemini",
            ProviderKind::Ollama => "ollama",
        }
    }

    pub fn api_key(&self) -> Option<String> {
        let env = self
            .api_key_env
            .clone()
            .unwrap_or_else(|| self.provider.default_api_key_env().to_string());
        std::env::var(env).ok()
    }

    pub fn endpoint(&self) -> String {
        self.endpoint_env
            .as_deref()
            .and_then(|e| std::env::var(e).ok())
            .unwrap_or_else(|| self.provider.default_endpoint().to_string())
    }
}

/// Core trait: one-shot prompt → text.
#[async_trait::async_trait]
pub trait Provider {
    async fn complete(&self, prompt: &Prompt) -> McpResult<String>;
}

/// Dispatch: pick the right provider impl, call it, return text.
pub async fn drive(cfg: &ProviderConfig, prompt: &Prompt) -> McpResult<String> {
    match cfg.provider {
        ProviderKind::Anthropic => anthropic::AnthropicProvider::new(cfg).complete(prompt).await,
        ProviderKind::OpenAi => openai::OpenAiProvider::new(cfg).complete(prompt).await,
        ProviderKind::Gemini => gemini::GeminiProvider::new(cfg).complete(prompt).await,
        ProviderKind::Ollama => ollama::OllamaProvider::new(cfg).complete(prompt).await,
    }
}

/// Shared reqwest client factory. All providers use the same timeouts
/// + user agent so CI logs are consistent.
pub(crate) fn http_client() -> McpResult<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(concat!("versionx-mcp/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_mins(1))
        .build()
        .map_err(|e| McpError::Internal(format!("http client: {e}")))
}

pub(crate) fn require_api_key(cfg: &ProviderConfig) -> McpResult<String> {
    cfg.api_key().ok_or_else(|| {
        McpError::InvalidParams(format!(
            "no API key in env var `{}`",
            cfg.api_key_env.clone().unwrap_or_else(|| cfg.provider.default_api_key_env().into())
        ))
    })
}
