//! Strongly-typed representation of `versionx.toml`.
//!
//! This is the deserialization target. Writing back out goes through
//! `toml_edit` in `versionx-core::commands::init` so user formatting is
//! preserved.
//!
//! See `docs/spec/02-config-and-state-model.md §2` for the canonical schema
//! spec. The types here implement it for 0.1.0 — features called out as
//! "v1.1+" in the spec (waivers, advanced policies, etc.) are represented
//! loosely via `extra` fields so the parser doesn't reject future additions.

use std::collections::BTreeMap;

use camino::Utf8PathBuf;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// Maximum `schema_version` this binary understands. Bumped on breaking
/// changes to the schema.
pub const SUPPORTED_SCHEMA_VERSION: &str = "1";

/// The root of `versionx.toml`.
///
/// Unknown top-level keys are **not** forwarded to `extra` — we fail loudly
/// so typos get caught.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VersionxConfig {
    /// Top-level metadata.
    #[serde(default, skip_serializing_if = "VersionxMetaConfig::is_empty")]
    pub versionx: VersionxMetaConfig,

    /// Environment variables exported into every adapter/task invocation.
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub vars: IndexMap<String, String>,

    /// Runtime pins (node, python, rust, pnpm, etc.).
    #[serde(default, skip_serializing_if = "RuntimesConfig::is_empty")]
    pub runtimes: RuntimesConfig,

    /// Per-ecosystem configuration.
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub ecosystems: IndexMap<String, EcosystemConfig>,

    /// Native task definitions. v1.0 ships topo-exec without caching.
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub tasks: IndexMap<String, TaskConfig>,

    /// Release orchestration settings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release: Option<ReleaseConfig>,

    /// External-repo links (submodule / subtree / virtual / ref).
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub links: IndexMap<String, LinkConfig>,

    /// Policies referenced by this repo.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policies: Option<PoliciesConfig>,

    /// GitHub integration (non-App — used by reusable Actions).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github: Option<GitHubConfig>,

    /// Inheritance semantics for array-valued keys.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inherit: Option<InheritPolicy>,

    /// Advanced / rarely-touched knobs.
    #[serde(default, skip_serializing_if = "AdvancedConfig::is_empty")]
    pub advanced: AdvancedConfig,
}

/// `[versionx]` block: project metadata.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VersionxMetaConfig {
    /// Schema version. Required at L3+. Defaults to
    /// [`SUPPORTED_SCHEMA_VERSION`] when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_version: Option<String>,

    /// Human-readable project name. Defaults to the containing dir.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// When `true`, this file is a workspace root and `vx` walks up to it.
    #[serde(default, skip_serializing_if = "is_false")]
    pub workspace: bool,
}

impl VersionxMetaConfig {
    pub(crate) const fn is_empty(&self) -> bool {
        self.schema_version.is_none() && self.name.is_none() && !self.workspace
    }
}

/// `[runtimes]` block.
///
/// Cannot set `deny_unknown_fields` here because `tools` is flattened —
/// every tool name (`node`, `python`, `pnpm`, ...) appears as a top-level
/// key and serde would reject them all.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RuntimesConfig {
    /// Provider URLs / names per tool. Optional.
    #[serde(default, skip_serializing_if = "RuntimeProviders::is_empty")]
    pub providers: RuntimeProviders,

    /// Tool -> version spec. Captures everything not matched above.
    /// Values may be a plain string (`"20.11.1"`) or a table with distribution
    /// hints (`{ version = "21", distribution = "temurin" }`).
    #[serde(flatten, default)]
    pub tools: IndexMap<String, RuntimeSpec>,
}

impl RuntimesConfig {
    pub(crate) fn is_empty(&self) -> bool {
        self.providers.is_empty() && self.tools.is_empty()
    }
}

/// `[runtimes.providers]` — where to fetch installers from.
///
/// Same flatten caveat as [`RuntimesConfig`]: no `deny_unknown_fields`.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RuntimeProviders {
    /// Tool -> provider string (URL or named provider like
    /// `"python-build-standalone"`, `"nodejs.org"`, `"temurin"`).
    #[serde(flatten, default)]
    pub providers: BTreeMap<String, String>,
}

impl RuntimeProviders {
    pub(crate) fn is_empty(&self) -> bool {
        self.providers.is_empty()
    }
}

/// A runtime spec — either a plain version string or a structured table.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RuntimeSpec {
    /// `node = "20.11.1"` or `node = "lts"`
    Version(String),
    /// `jvm = { version = "21", distribution = "temurin" }`
    Detailed {
        version: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        distribution: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        channel: Option<String>,
    },
}

impl RuntimeSpec {
    /// Return the version string regardless of shape.
    #[must_use]
    pub fn version(&self) -> &str {
        match self {
            Self::Version(v) => v,
            Self::Detailed { version, .. } => version,
        }
    }
}

/// `[ecosystems.<id>]` block.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EcosystemConfig {
    /// `"pnpm"`, `"uv"`, `"cargo"`, etc. Auto-detected when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package_manager: Option<String>,

    /// Path (relative to this file) where the ecosystem's manifest lives.
    /// Defaults to the repo root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<Utf8PathBuf>,

    /// Workspace member globs (Node pnpm workspaces, Cargo workspaces, uv workspaces).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub workspaces: Vec<String>,

    /// Python-specific: which tool owns venv creation (`"uv"` / `"poetry"` / `"versionx"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub venv_manager: Option<String>,
}

/// `[tasks.<name>]` block. Full schema lands with the task runner (0.9).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskConfig {
    /// Shell-free command (run via `versionx::proc`, never invoking a shell).
    pub run: String,
    /// Task-level env overrides.
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub env: IndexMap<String, String>,
    /// Upstream tasks that must succeed first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub depends_on: Vec<String>,
    /// Input globs (feed into the v1.2 content-addressed cache).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inputs: Vec<String>,
    /// Output globs (ditto).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outputs: Vec<String>,
}

/// `[release]` block.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseConfig {
    /// `"pr-title"` (default), `"conventional"`, `"changesets"`, `"manual"`.
    #[serde(default = "default_strategy")]
    pub strategy: String,

    /// `"mcp"` (agent drives), `"byo-api"`, or `"off"`.
    #[serde(default = "default_ai_assist")]
    pub ai_assist: String,

    /// `"semver"` or `"calver"`.
    #[serde(default = "default_versioning")]
    pub versioning: String,

    /// Template for the git tag name. `{version}` and `{package}` supported.
    #[serde(default = "default_tag_template")]
    pub tag_template: String,

    /// Path to the changelog file relative to the repo root.
    #[serde(default = "default_changelog")]
    pub changelog: String,

    /// Plan TTL (human duration: `"1h"`, `"24h"`, `"7d"`).
    #[serde(default = "default_plan_ttl")]
    pub plan_ttl: String,

    /// `"prompt"` (default, TTY-only) or `"explicit"` (always require flag).
    #[serde(default = "default_push_mode")]
    pub push_mode: String,

    /// BYO-API-key LLM provider config, only consulted when `ai_assist = "byo-api"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ai: Option<AiConfig>,

    /// Per-package overrides (paths and bump rules).
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub packages: IndexMap<String, PackageReleaseConfig>,
}

impl Default for ReleaseConfig {
    fn default() -> Self {
        Self {
            strategy: default_strategy(),
            ai_assist: default_ai_assist(),
            versioning: default_versioning(),
            tag_template: default_tag_template(),
            changelog: default_changelog(),
            plan_ttl: default_plan_ttl(),
            push_mode: default_push_mode(),
            ai: None,
            packages: IndexMap::new(),
        }
    }
}

fn default_strategy() -> String {
    "pr-title".into()
}
fn default_ai_assist() -> String {
    "mcp".into()
}
fn default_versioning() -> String {
    "semver".into()
}
fn default_tag_template() -> String {
    "v{version}".into()
}
fn default_changelog() -> String {
    "CHANGELOG.md".into()
}
fn default_plan_ttl() -> String {
    "24h".into()
}
fn default_push_mode() -> String {
    "prompt".into()
}

/// `[release.ai.byo]` — headless LLM config.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AiConfig {
    /// `"anthropic"`, `"openai"`, `"gemini"`, `"ollama"`.
    pub provider: String,
    /// Model name (`"claude-sonnet-4-6"`, `"gpt-4o"`, `"llama3.2"`, etc.).
    pub model: String,
    /// Env var to read the API key from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key_env: Option<String>,
    /// Optional endpoint override for self-hosted / proxied servers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint_env: Option<String>,
}

/// Per-package release knobs inside a monorepo.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageReleaseConfig {
    #[serde(default, skip_serializing_if = "is_false")]
    pub public: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registry: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<String>,
}

/// `[links.<name>]` block.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinkConfig {
    /// `"submodule"` | `"subtree"` | `"virtual"` | `"ref"`.
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<Utf8PathBuf>,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub track: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub update: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub bidirectional: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub squash: bool,
}

/// Compat alias for callers building link collections.
pub type LinksConfig = IndexMap<String, LinkConfig>;

/// `[policies]` block.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PoliciesConfig {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inherit: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
}

/// `[github]` block.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitHubConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_checks: Vec<String>,
}

/// `[inherit]` block — controls array merge semantics.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InheritPolicy {
    /// Keys whose arrays are concatenated across levels instead of replaced.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub append: Vec<String>,
}

/// `[advanced]` block.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdvancedConfig {
    /// `"auto"` | `"always"` | `"never"`. Default: `"auto"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub daemon: Option<String>,
    /// Max parallelism for adapter operations (0 = auto).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jobs: Option<usize>,
    /// Remote state backend URL (post-v1.0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_backend: Option<String>,
    /// Env var that holds the remote-state URL (preferred over inline).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_backend_env: Option<String>,
    /// Disable the lockfile entirely (not recommended).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lockfile: Option<bool>,
}

impl AdvancedConfig {
    pub(crate) const fn is_empty(&self) -> bool {
        self.daemon.is_none()
            && self.jobs.is_none()
            && self.state_backend.is_none()
            && self.state_backend_env.is_none()
            && self.lockfile.is_none()
    }
}

/// Hint type for the CLI `--output` flag when it's persisted into config.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "lowercase")]
pub enum OutputOverride {
    #[default]
    Human,
    Json,
    Ndjson,
}

/// Helper: `skip_serializing_if = "is_false"`.
const fn is_false(b: &bool) -> bool {
    !*b
}
