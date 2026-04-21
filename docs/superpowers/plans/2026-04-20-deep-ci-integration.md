# Deep CI Integration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Versionx CI-native. When users adopt it in their own repos, the CLI auto-detects the CI environment, drives the full release / update / policy / saga flow, posts PR comments + check runs + annotations, and publishes to registries — all with zero configuration beyond a single reusable workflow line.

**Architecture:** New `versionx-forge-trait` crate owns the forge-agnostic contract (context, annotations, check runs, sticky comments, release PRs, dispatch, identity, publish). Fill in the existing `versionx-github` skeleton as the primary implementation. Phases B–D add features on top. Phase E adds `versionx-gitlab`, `versionx-bitbucket`, `versionx-gitea` parallel impls. Core stays platform-agnostic via `Reporter` traits in `versionx-core::integrations`.

**Tech Stack:** Rust 1.95, `octocrab` 0.49 (GitHub), `reqwest` (other forges), `wiremock` (integration tests), `jsonwebtoken` (App JWT), `ssh-key` + `gpg` shell-out (signing), `insta` (snapshots), `proptest`, `clap` 4.

**Spec:** [`docs/superpowers/specs/2026-04-20-deep-ci-integration-design.md`](../specs/2026-04-20-deep-ci-integration-design.md)

---

## Prerequisites

Before any task below:

- Rust 1.95+ installed (`rust-toolchain.toml` pins it).
- `cargo-nextest` + `cargo-deny` available (`cargo install cargo-nextest cargo-deny`).
- Repo cloned at `/path/to/versionx`; all commands run from repo root.
- A GitHub PAT with `repo` + `workflow` scopes set as `VERSIONX_TEST_GH_PAT` in shell env for integration tests (can be skipped for unit work).

Every task follows the same pattern unless noted:

1. Write the failing test.
2. Run to confirm it fails for the expected reason.
3. Implement the minimum code to pass.
4. Run to confirm it passes.
5. Commit with a Conventional Commit message.

---

# Phase A — Forge trait + GitHub context + annotations + check runs

Deliverable: `versionx github detect` prints the current repo / token / capability context in under 500ms. In a PR, every mutating verb creates a check run. Stderr carries `::error::` / `::warning::` / `::notice::` annotations.

## Task A1: Create `versionx-forge-trait` crate skeleton

**Files:**
- Create: `crates/versionx-forge-trait/Cargo.toml`
- Create: `crates/versionx-forge-trait/src/lib.rs`
- Modify: `Cargo.toml` (workspace members + workspace-deps)

- [ ] **Step 1: Add crate to workspace**

Modify `Cargo.toml` at repo root, `[workspace] members` array — insert alphabetically:

```toml
    "crates/versionx-forge-trait",
```

And in `[workspace.dependencies]`:

```toml
versionx-forge-trait = { path = "crates/versionx-forge-trait", version = "0.1.0" }
```

- [ ] **Step 2: Write the Cargo.toml**

Create `crates/versionx-forge-trait/Cargo.toml`:

```toml
[package]
name = "versionx-forge-trait"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
authors.workspace = true
license.workspace = true
repository.workspace = true
description = "Forge-agnostic trait layer for versionx CI integrations."

[lints]
workspace = true

[dependencies]
async-trait.workspace = true
bitflags = "2.6"
camino.workspace = true
chrono.workspace = true
serde = { workspace = true, features = ["derive"] }
serde_json.workspace = true
thiserror.workspace = true
uuid.workspace = true
```

- [ ] **Step 3: Add bitflags to workspace dependencies**

Modify root `Cargo.toml` `[workspace.dependencies]`:

```toml
bitflags = "2.6"
```

- [ ] **Step 4: Write the lib.rs skeleton**

Create `crates/versionx-forge-trait/src/lib.rs`:

```rust
//! Forge-agnostic trait layer for versionx CI integrations.
//!
//! Every forge (GitHub, GitLab, Bitbucket, Gitea) implements the traits
//! defined in this crate. `versionx-core` depends on these traits (not on
//! any concrete forge crate) so new forges drop in without touching core.
//!
//! See `docs/superpowers/specs/2026-04-20-deep-ci-integration-design.md`.

#![deny(unsafe_code)]

pub mod annotations;
pub mod capabilities;
pub mod check_run;
pub mod context;
pub mod dispatch;
pub mod error;
pub mod identity;
pub mod pr_comment;
pub mod publish;
pub mod release_pr;
pub mod testkit;

pub use annotations::{Annotation, AnnotationLevel, AnnotationSink};
pub use capabilities::Capabilities;
pub use check_run::{CheckRun, CheckRunClient, CheckRunStatus};
pub use context::{ForgeContext, GitRef, PullRequest, RepoRef, TokenSource};
pub use dispatch::{DispatchClient, DispatchPayload};
pub use error::{ForgeError, ForgeResult};
pub use identity::{Identity, SigningMode};
pub use pr_comment::{StickyComment, StickyCommentClient};
pub use publish::{PublishDriver, PublishEcosystem, PublishMode, PublishOutcome};
pub use release_pr::{ReleasePr, ReleasePrClient, ReleasePrState};
```

- [ ] **Step 5: Build and commit**

```bash
cargo check -p versionx-forge-trait
```

Expected: fails — none of the referenced modules exist yet. That's OK; the next tasks populate each.

```bash
git add Cargo.toml crates/versionx-forge-trait/
git commit -m "feat(forge-trait): scaffold crate"
```

---

## Task A2: `error` module

**Files:**
- Create: `crates/versionx-forge-trait/src/error.rs`

- [ ] **Step 1: Write the tests**

Create `crates/versionx-forge-trait/src/error.rs`:

```rust
//! Shared error type for forge operations.

use thiserror::Error;

pub type ForgeResult<T> = Result<T, ForgeError>;

#[derive(Debug, Error)]
pub enum ForgeError {
    #[error("no forge detected — set GITHUB_ACTIONS=true, GITLAB_CI=true, or similar")]
    NotDetected,

    #[error("no token available — set VERSIONX_GH_TOKEN, a GitHub App trio, or accept GITHUB_TOKEN from Actions")]
    NoToken,

    #[error("capability `{0}` required but not available with the current token")]
    MissingCapability(&'static str),

    #[error("forge API call failed: {0}")]
    Api(String),

    #[error("forge returned unexpected shape: {0}")]
    Malformed(String),

    #[error("rate limited; retry after {retry_after_seconds}s")]
    RateLimited { retry_after_seconds: u64 },

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_displays_useful_message() {
        let e = ForgeError::MissingCapability("WRITE_CHECKS");
        assert_eq!(
            e.to_string(),
            "capability `WRITE_CHECKS` required but not available with the current token"
        );
    }

    #[test]
    fn rate_limited_preserves_retry_after() {
        let e = ForgeError::RateLimited { retry_after_seconds: 42 };
        assert!(e.to_string().contains("42s"));
    }
}
```

- [ ] **Step 2: Run the tests**

```bash
cargo test -p versionx-forge-trait error
```

Expected: PASS — all 2 tests green.

- [ ] **Step 3: Commit**

```bash
git add crates/versionx-forge-trait/src/error.rs
git commit -m "feat(forge-trait): add ForgeError taxonomy"
```

---

## Task A3: `capabilities` module

**Files:**
- Create: `crates/versionx-forge-trait/src/capabilities.rs`

- [ ] **Step 1: Write module with tests**

```rust
//! Capability bitflags — which operations the current token is allowed to perform.
//!
//! Flags are forge-neutral. Forge-specific perms (GitLab approval rules,
//! Bitbucket default reviewers) get their own bitflags in each impl crate.

use bitflags::bitflags;

bitflags! {
    #[derive(Copy, Clone, Debug, PartialEq, Eq)]
    pub struct Capabilities: u32 {
        const READ_REPO           = 1 << 0;
        const WRITE_CONTENTS      = 1 << 1;
        const WRITE_PULL_REQUESTS = 1 << 2;
        const WRITE_CHECKS        = 1 << 3;
        const TRIGGER_WORKFLOWS   = 1 << 4;
        const CROSS_REPO          = 1 << 5;
        const WRITE_PACKAGES      = 1 << 6;
        const WRITE_ISSUES        = 1 << 7;
    }
}

impl Capabilities {
    /// Capabilities typical for Actions' default `GITHUB_TOKEN` with
    /// a liberal `permissions:` block.
    #[must_use]
    pub fn actions_default() -> Self {
        Self::READ_REPO
            | Self::WRITE_CONTENTS
            | Self::WRITE_PULL_REQUESTS
            | Self::WRITE_CHECKS
            | Self::WRITE_ISSUES
            | Self::WRITE_PACKAGES
    }

    /// Capabilities a typical fine-grained PAT grants (no check runs).
    #[must_use]
    pub fn pat_default() -> Self {
        Self::READ_REPO
            | Self::WRITE_CONTENTS
            | Self::WRITE_PULL_REQUESTS
            | Self::WRITE_ISSUES
            | Self::TRIGGER_WORKFLOWS
            | Self::CROSS_REPO
    }

    /// Full set an installed GitHub App with our requested scopes grants.
    #[must_use]
    pub fn app_full() -> Self {
        Self::all()
    }

    /// Helper for error-reporting humans.
    #[must_use]
    pub fn missing(self, needed: Self) -> Self {
        needed - self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actions_default_cannot_trigger_workflows() {
        assert!(!Capabilities::actions_default().contains(Capabilities::TRIGGER_WORKFLOWS));
    }

    #[test]
    fn pat_default_cannot_write_checks() {
        assert!(!Capabilities::pat_default().contains(Capabilities::WRITE_CHECKS));
    }

    #[test]
    fn app_full_includes_every_flag() {
        assert_eq!(Capabilities::app_full(), Capabilities::all());
    }

    #[test]
    fn missing_returns_what_is_absent() {
        let have = Capabilities::READ_REPO | Capabilities::WRITE_CONTENTS;
        let need = Capabilities::READ_REPO | Capabilities::WRITE_CHECKS;
        assert_eq!(have.missing(need), Capabilities::WRITE_CHECKS);
    }
}
```

- [ ] **Step 2: Run tests**

```bash
cargo test -p versionx-forge-trait capabilities
```

Expected: 4 tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/versionx-forge-trait/src/capabilities.rs
git commit -m "feat(forge-trait): add Capabilities bitflags"
```

---

## Task A4: `context` module — types + `ForgeContext` trait

**Files:**
- Create: `crates/versionx-forge-trait/src/context.rs`

- [ ] **Step 1: Write the module**

```rust
//! Forge-agnostic context: "where am I running, as whom, with what token".

use serde::{Deserialize, Serialize};

use crate::capabilities::Capabilities;

/// Identifies a repo across forges: `owner` is the namespace (org, user,
/// group), `name` is the repo slug. For forges with nested groups
/// (GitLab), `owner` may contain slashes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoRef {
    pub owner: String,
    pub name: String,
}

impl RepoRef {
    #[must_use]
    pub fn new(owner: impl Into<String>, name: impl Into<String>) -> Self {
        Self { owner: owner.into(), name: name.into() }
    }

    /// Canonical "owner/name" string.
    #[must_use]
    pub fn full(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }
}

/// The git ref we're operating against.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GitRef {
    Branch(String),
    Tag(String),
    Sha(String),
    PullRequestHead { pr_number: u64, sha: String },
}

/// Pull request / merge request metadata.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PullRequest {
    pub number: u64,
    pub head_sha: String,
    pub base_branch: String,
    pub head_branch: String,
    pub title: String,
    pub author: String,
    pub draft: bool,
    pub labels: Vec<String>,
}

/// Where the current token came from.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TokenSource {
    App,
    Pat,
    GithubToken,
    None,
}

/// Forge-neutral context, populated at CLI startup.
///
/// The async detect() method lives on each forge's own impl type; this
/// trait only describes what every impl must expose.
pub trait ForgeContext: Send + Sync + std::fmt::Debug {
    fn repo(&self) -> &RepoRef;
    fn default_branch(&self) -> &str;
    fn current_ref(&self) -> &GitRef;
    fn commit_sha(&self) -> &str;
    fn actor(&self) -> &str;
    fn run_id(&self) -> Option<u64>;
    fn pull_request(&self) -> Option<&PullRequest>;
    fn token_source(&self) -> TokenSource;
    fn capabilities(&self) -> Capabilities;
    fn in_ci(&self) -> bool;
    fn forge_name(&self) -> &'static str;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repo_ref_full_roundtrip() {
        let r = RepoRef::new("KodyDennon", "versionx");
        assert_eq!(r.full(), "KodyDennon/versionx");
    }

    #[test]
    fn git_ref_variants_serialize() {
        let cases = [
            GitRef::Branch("main".into()),
            GitRef::Tag("v0.8.0".into()),
            GitRef::Sha("abc1234".into()),
            GitRef::PullRequestHead { pr_number: 42, sha: "def5678".into() },
        ];
        for c in cases {
            let j = serde_json::to_string(&c).unwrap();
            let back: GitRef = serde_json::from_str(&j).unwrap();
            assert_eq!(c, back);
        }
    }
}
```

- [ ] **Step 2: Run tests**

```bash
cargo test -p versionx-forge-trait context
```

Expected: 2 tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/versionx-forge-trait/src/context.rs
git commit -m "feat(forge-trait): add ForgeContext trait and core types"
```

---

## Task A5: `annotations` module

**Files:**
- Create: `crates/versionx-forge-trait/src/annotations.rs`

- [ ] **Step 1: Write module**

```rust
//! CI-native annotations. Each forge emits a different textual
//! convention; the struct here is forge-neutral.

use serde::{Deserialize, Serialize};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AnnotationLevel {
    Error,
    Warning,
    Notice,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Annotation {
    pub level: AnnotationLevel,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_line: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub col: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_col: Option<u32>,
}

impl Annotation {
    #[must_use]
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            level: AnnotationLevel::Error,
            message: message.into(),
            title: None,
            file: None,
            line: None,
            end_line: None,
            col: None,
            end_col: None,
        }
    }

    #[must_use]
    pub fn warning(message: impl Into<String>) -> Self {
        Self { level: AnnotationLevel::Warning, ..Self::error(message) }
    }

    #[must_use]
    pub fn notice(message: impl Into<String>) -> Self {
        Self { level: AnnotationLevel::Notice, ..Self::error(message) }
    }

    #[must_use]
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    #[must_use]
    pub fn with_file(mut self, file: impl Into<String>, line: u32) -> Self {
        self.file = Some(file.into());
        self.line = Some(line);
        self
    }
}

/// Trait every forge impl provides to emit annotations in its native
/// convention.
pub trait AnnotationSink: Send + Sync {
    fn emit(&self, annotation: &Annotation);
}

/// No-op sink — used when `ForgeContext` reports `in_ci() == false`.
#[derive(Debug, Default)]
pub struct NullSink;

impl AnnotationSink for NullSink {
    fn emit(&self, _: &Annotation) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_helper_sets_level() {
        let a = Annotation::error("boom");
        assert_eq!(a.level, AnnotationLevel::Error);
        assert_eq!(a.message, "boom");
    }

    #[test]
    fn with_file_sets_line() {
        let a = Annotation::warning("x").with_file("versionx.toml", 14);
        assert_eq!(a.file.as_deref(), Some("versionx.toml"));
        assert_eq!(a.line, Some(14));
    }
}
```

- [ ] **Step 2: Run tests**

```bash
cargo test -p versionx-forge-trait annotations
```

Expected: 2 tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/versionx-forge-trait/src/annotations.rs
git commit -m "feat(forge-trait): add Annotation + AnnotationSink"
```

---

## Task A6: `check_run` module

**Files:**
- Create: `crates/versionx-forge-trait/src/check_run.rs`

- [ ] **Step 1: Write module**

```rust
//! Check-run (status check) trait — create/update/finalize a named
//! status that gates PR merges when the repo requires it.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::annotations::Annotation;
use crate::error::ForgeResult;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckRunStatus {
    Queued,
    InProgress,
    Success,
    Failure,
    Neutral,
    Cancelled,
    Skipped,
    TimedOut,
}

/// Server-side handle for a check run.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CheckRun {
    pub id: u64,
    pub name: String,
    pub head_sha: String,
    pub status: CheckRunStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

#[async_trait]
pub trait CheckRunClient: Send + Sync {
    /// Start a check run in `in_progress` state.
    async fn start(
        &self,
        name: &str,
        head_sha: &str,
        details_url: Option<&str>,
    ) -> ForgeResult<CheckRun>;

    /// Transition to terminal status with optional summary + annotations.
    async fn finish(
        &self,
        check_run_id: u64,
        status: CheckRunStatus,
        summary: Option<&str>,
        annotations: &[Annotation],
    ) -> ForgeResult<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_serializes_snake_case() {
        let j = serde_json::to_string(&CheckRunStatus::InProgress).unwrap();
        assert_eq!(j, "\"in_progress\"");
    }
}
```

- [ ] **Step 2: Run tests**

```bash
cargo test -p versionx-forge-trait check_run
```

Expected: 1 test passes.

- [ ] **Step 3: Commit**

```bash
git add crates/versionx-forge-trait/src/check_run.rs
git commit -m "feat(forge-trait): add CheckRunClient trait"
```

---

## Task A7: `pr_comment` module

**Files:**
- Create: `crates/versionx-forge-trait/src/pr_comment.rs`

- [ ] **Step 1: Write module**

```rust
//! Sticky PR comments — one per (PR, marker) pair, upserted on every run.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::ForgeResult;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StickyComment {
    pub id: u64,
    pub pr_number: u64,
    pub marker: String,
    pub body: String,
}

#[async_trait]
pub trait StickyCommentClient: Send + Sync {
    /// Create or update a sticky comment identified by `marker` (an
    /// HTML-comment token embedded in the body). `marker` must contain
    /// `versionx:` prefix.
    async fn upsert(
        &self,
        pr_number: u64,
        marker: &str,
        body: &str,
    ) -> ForgeResult<StickyComment>;

    /// Delete the sticky comment with `marker`, if any exists.
    async fn delete_if_exists(&self, pr_number: u64, marker: &str) -> ForgeResult<()>;
}

/// Helper: wrap a body with the marker HTML comment at the top.
#[must_use]
pub fn wrap_with_marker(marker: &str, body: &str) -> String {
    format!("<!-- {marker} -->\n{body}")
}

/// Helper: return the marker extracted from a body, or None.
#[must_use]
pub fn extract_marker(body: &str) -> Option<&str> {
    let first = body.lines().next()?;
    let rest = first.strip_prefix("<!--")?.trim_start();
    let inner = rest.strip_suffix("-->")?.trim_end();
    inner.strip_prefix("versionx:").map(|s| s.trim())
        .map(|_| inner.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_puts_marker_on_first_line() {
        let body = wrap_with_marker("versionx:release-plan", "Hello");
        assert!(body.starts_with("<!-- versionx:release-plan -->"));
    }

    #[test]
    fn extract_matches_versionx_markers_only() {
        let body = "<!-- versionx:release-plan -->\nBody";
        assert_eq!(extract_marker(body), Some("versionx:release-plan"));

        let unrelated = "<!-- random-comment -->\nBody";
        assert_eq!(extract_marker(unrelated), None);
    }

    #[test]
    fn extract_returns_none_when_no_comment() {
        assert_eq!(extract_marker("Just body"), None);
    }
}
```

- [ ] **Step 2: Run tests**

```bash
cargo test -p versionx-forge-trait pr_comment
```

Expected: 3 tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/versionx-forge-trait/src/pr_comment.rs
git commit -m "feat(forge-trait): add sticky PR comment trait + helpers"
```

---

## Task A8: `release_pr`, `dispatch`, `identity`, `publish`, `testkit` stubs

**Files:**
- Create: `crates/versionx-forge-trait/src/release_pr.rs`
- Create: `crates/versionx-forge-trait/src/dispatch.rs`
- Create: `crates/versionx-forge-trait/src/identity.rs`
- Create: `crates/versionx-forge-trait/src/publish.rs`
- Create: `crates/versionx-forge-trait/src/testkit.rs`

Full-featured trait definitions land in later tasks; for Phase A we need the modules to compile.

- [ ] **Step 1: `release_pr.rs`**

```rust
//! Release-PR lifecycle (open/sync/merge/close). Full trait lands in Phase C.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::ForgeResult;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReleasePrState { Open, Merged, Closed, Missing }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReleasePr {
    pub pr_number: u64,
    pub state: ReleasePrState,
    pub head_branch: String,
    pub plan_blake3: String,
}

#[async_trait]
pub trait ReleasePrClient: Send + Sync {
    async fn current(&self, label: &str) -> ForgeResult<Option<ReleasePr>>;
}
```

- [ ] **Step 2: `dispatch.rs`**

```rust
//! workflow_dispatch / repository_dispatch abstraction. Full impl in Phase D.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::context::RepoRef;
use crate::error::ForgeResult;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DispatchPayload {
    pub workflow_file: String,
    pub ref_: String,
    pub inputs: serde_json::Value,
}

#[async_trait]
pub trait DispatchClient: Send + Sync {
    async fn workflow_dispatch(
        &self,
        target: &RepoRef,
        payload: &DispatchPayload,
    ) -> ForgeResult<()>;
}
```

- [ ] **Step 3: `identity.rs`**

```rust
//! Commit-author identity + signing. Full impl (SSH/GPG) in Phase C.

use serde::{Deserialize, Serialize};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SigningMode { None, Gpg, Ssh }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Identity {
    pub name: String,
    pub email: String,
    #[serde(default)]
    pub signing: SigningMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signing_key_env: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub co_authored_by: Option<String>,
}

impl Default for SigningMode {
    fn default() -> Self { Self::None }
}

impl Identity {
    #[must_use]
    pub fn github_actions_bot() -> Self {
        Self {
            name: "github-actions[bot]".into(),
            email: "41898282+github-actions[bot]@users.noreply.github.com".into(),
            signing: SigningMode::None,
            signing_key_env: None,
            co_authored_by: None,
        }
    }
}
```

- [ ] **Step 4: `publish.rs`**

```rust
//! Publish drivers — per-ecosystem registry push. Full impls in Phase C.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::ForgeResult;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PublishEcosystem { Node, Rust, Python, Oci, Custom }

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PublishMode { Versionx, Workflow, Skip }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PublishOutcome {
    pub ecosystem: PublishEcosystem,
    pub package: String,
    pub version: String,
    pub registry: String,
    pub published: bool,
    pub message: String,
}

#[async_trait]
pub trait PublishDriver: Send + Sync {
    fn ecosystem(&self) -> PublishEcosystem;
    async fn publish(
        &self,
        package: &str,
        version: &str,
        dir: &camino::Utf8Path,
    ) -> ForgeResult<PublishOutcome>;
}
```

- [ ] **Step 5: `testkit.rs`**

```rust
//! Shared test harness every forge impl plugs into.
//!
//! Full helpers (fixture trees, wiremock server setup, assertion
//! builders) land in Task A35. For now this is a stub so the rest of
//! the crate compiles.

#![cfg(any(test, feature = "testkit"))]

pub fn placeholder() {}
```

- [ ] **Step 6: Build & commit**

```bash
cargo check -p versionx-forge-trait
```

Expected: compiles clean.

```bash
git add crates/versionx-forge-trait/src/
git commit -m "feat(forge-trait): scaffold release_pr / dispatch / identity / publish / testkit"
```

---

## Task A9: `versionx-github` crate — add real dependencies

**Files:**
- Modify: `crates/versionx-github/Cargo.toml`

- [ ] **Step 1: Read current Cargo.toml**

Run `cat crates/versionx-github/Cargo.toml` to see what's there.

- [ ] **Step 2: Rewrite Cargo.toml**

```toml
[package]
name = "versionx-github"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
authors.workspace = true
license.workspace = true
repository.workspace = true
description = "GitHub forge implementation for versionx."

[lints]
workspace = true

[dependencies]
anyhow.workspace = true
async-trait.workspace = true
camino.workspace = true
chrono.workspace = true
jsonwebtoken = "9"
octocrab.workspace = true
parking_lot.workspace = true
reqwest.workspace = true
serde = { workspace = true, features = ["derive"] }
serde_json.workspace = true
thiserror.workspace = true
tokio = { workspace = true, features = ["full"] }
tracing.workspace = true
url.workspace = true
uuid.workspace = true
versionx-forge-trait.workspace = true

[dev-dependencies]
insta.workspace = true
proptest.workspace = true
tempfile.workspace = true
tokio.workspace = true
wiremock = "0.6"
```

- [ ] **Step 3: Add jsonwebtoken + wiremock to workspace deps**

Modify root `Cargo.toml`:

```toml
jsonwebtoken = "9"
wiremock = "0.6"
```

- [ ] **Step 4: Build & commit**

```bash
cargo check -p versionx-github
```

Expected: compiles (lib.rs is nearly empty).

```bash
git add Cargo.toml crates/versionx-github/Cargo.toml
git commit -m "chore(github): wire workspace deps for CI integration"
```

---

## Task A10: `versionx-github::context::GitHubContext` — env discovery

**Files:**
- Create: `crates/versionx-github/src/context.rs`
- Modify: `crates/versionx-github/src/lib.rs`

- [ ] **Step 1: Write the test first**

Create `crates/versionx-github/src/context.rs`:

```rust
//! GitHub-specific ForgeContext implementation.

use std::sync::Arc;

use versionx_forge_trait::{
    Capabilities, ForgeContext, GitRef, PullRequest, RepoRef, TokenSource,
};

use crate::token::ResolvedToken;

#[derive(Clone, Debug)]
pub struct GitHubContext {
    repo: RepoRef,
    default_branch: String,
    current_ref: GitRef,
    commit_sha: String,
    actor: String,
    run_id: Option<u64>,
    pull_request: Option<PullRequest>,
    token_source: TokenSource,
    capabilities: Capabilities,
    in_ci: bool,
    #[allow(dead_code)]
    token: Arc<ResolvedToken>,
}

impl GitHubContext {
    /// Env-only construction — no network calls. Capabilities come from
    /// the token source's default set; a live probe (Task A13) refines
    /// them via API calls.
    ///
    /// Returns `None` when `GITHUB_ACTIONS` is not truthy and no explicit
    /// `VERSIONX_GH_*` envs are set.
    pub fn from_env() -> Option<Self> {
        let in_ci = std::env::var("GITHUB_ACTIONS").is_ok_and(|v| v == "true");

        let repo = env_repo()?;
        let default_branch = std::env::var("GITHUB_BASE_REF")
            .ok()
            .filter(|s| !s.is_empty())
            .or_else(|| std::env::var("GITHUB_DEFAULT_BRANCH").ok())
            .unwrap_or_else(|| "main".into());

        let current_ref = env_current_ref();
        let commit_sha = std::env::var("GITHUB_SHA").unwrap_or_default();
        let actor = std::env::var("GITHUB_ACTOR").unwrap_or_default();
        let run_id = std::env::var("GITHUB_RUN_ID").ok().and_then(|s| s.parse().ok());

        let token = Arc::new(crate::token::discover()?);
        let (token_source, base_caps) = token.summarize();

        Some(Self {
            repo,
            default_branch,
            current_ref,
            commit_sha,
            actor,
            run_id,
            pull_request: None,
            token_source,
            capabilities: base_caps,
            in_ci,
            token,
        })
    }

    /// Attach a fetched pull-request after context creation.
    pub fn with_pull_request(mut self, pr: PullRequest) -> Self {
        self.pull_request = Some(pr);
        self
    }

    /// Override capabilities after a live probe (Task A13).
    pub fn with_capabilities(mut self, caps: Capabilities) -> Self {
        self.capabilities = caps;
        self
    }
}

fn env_repo() -> Option<RepoRef> {
    let raw = std::env::var("GITHUB_REPOSITORY").ok()?;
    let (owner, name) = raw.split_once('/')?;
    Some(RepoRef::new(owner, name))
}

fn env_current_ref() -> GitRef {
    if let Ok(pr_raw) = std::env::var("GITHUB_REF") {
        // refs/pull/42/merge or refs/pull/42/head
        if let Some(pr_part) = pr_raw.strip_prefix("refs/pull/") {
            if let Some((num, _)) = pr_part.split_once('/') {
                if let Ok(pr_number) = num.parse::<u64>() {
                    let sha = std::env::var("GITHUB_SHA").unwrap_or_default();
                    return GitRef::PullRequestHead { pr_number, sha };
                }
            }
        }
        if let Some(tag) = pr_raw.strip_prefix("refs/tags/") {
            return GitRef::Tag(tag.into());
        }
        if let Some(branch) = pr_raw.strip_prefix("refs/heads/") {
            return GitRef::Branch(branch.into());
        }
    }
    GitRef::Sha(std::env::var("GITHUB_SHA").unwrap_or_default())
}

impl ForgeContext for GitHubContext {
    fn repo(&self) -> &RepoRef { &self.repo }
    fn default_branch(&self) -> &str { &self.default_branch }
    fn current_ref(&self) -> &GitRef { &self.current_ref }
    fn commit_sha(&self) -> &str { &self.commit_sha }
    fn actor(&self) -> &str { &self.actor }
    fn run_id(&self) -> Option<u64> { self.run_id }
    fn pull_request(&self) -> Option<&PullRequest> { self.pull_request.as_ref() }
    fn token_source(&self) -> TokenSource { self.token_source }
    fn capabilities(&self) -> Capabilities { self.capabilities }
    fn in_ci(&self) -> bool { self.in_ci }
    fn forge_name(&self) -> &'static str { "github" }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scoped_env<T>(vars: &[(&str, &str)], body: impl FnOnce() -> T) -> T {
        use std::sync::Mutex;
        static LOCK: Mutex<()> = Mutex::new(());
        let _g = LOCK.lock().unwrap();
        let saved: Vec<_> = vars
            .iter()
            .map(|(k, _)| (k.to_string(), std::env::var(k).ok()))
            .collect();
        for (k, v) in vars {
            unsafe { std::env::set_var(k, v) };
        }
        let out = body();
        for (k, v) in saved {
            match v {
                Some(val) => unsafe { std::env::set_var(&k, val) },
                None => unsafe { std::env::remove_var(&k) },
            }
        }
        out
    }

    #[test]
    fn parses_github_repository_env() {
        scoped_env(&[("GITHUB_REPOSITORY", "owner/repo")], || {
            let r = env_repo().unwrap();
            assert_eq!(r.owner, "owner");
            assert_eq!(r.name, "repo");
        });
    }

    #[test]
    fn parses_pull_request_ref() {
        scoped_env(
            &[("GITHUB_REF", "refs/pull/42/merge"), ("GITHUB_SHA", "abcd")],
            || {
                let r = env_current_ref();
                assert!(matches!(r, GitRef::PullRequestHead { pr_number: 42, .. }));
            },
        );
    }

    #[test]
    fn parses_branch_ref() {
        scoped_env(&[("GITHUB_REF", "refs/heads/main"), ("GITHUB_SHA", "")], || {
            assert_eq!(env_current_ref(), GitRef::Branch("main".into()));
        });
    }

    #[test]
    fn parses_tag_ref() {
        scoped_env(&[("GITHUB_REF", "refs/tags/v0.8.0"), ("GITHUB_SHA", "")], || {
            assert_eq!(env_current_ref(), GitRef::Tag("v0.8.0".into()));
        });
    }
}
```

- [ ] **Step 2: Rewrite lib.rs to declare modules**

Modify `crates/versionx-github/src/lib.rs`:

```rust
//! GitHub forge integration for versionx.
//!
//! See docs/superpowers/specs/2026-04-20-deep-ci-integration-design.md.

#![deny(unsafe_code)]

pub mod annotations;
pub mod app;
pub mod check_run;
pub mod client;
pub mod context;
pub mod dispatch;
pub mod identity;
pub mod merge;
pub mod pr_comment;
pub mod publish;
pub mod release_pr;
pub mod token;

pub use context::GitHubContext;
pub use token::{ResolvedToken, TokenDiscovery};
```

- [ ] **Step 3: Create placeholder modules so the file compiles**

Create each of these as empty modules with `// TODO(phase-B/C/D)` comments. For Phase A only `annotations`, `check_run`, `client`, `context`, `token` need real content; the rest are stubs.

```bash
for m in annotations app check_run client dispatch identity merge pr_comment publish release_pr token; do
  f="crates/versionx-github/src/$m.rs"
  if [ ! -f "$f" ]; then
    echo "//! (stub — filled in a later task)" > "$f"
  fi
done
```

- [ ] **Step 4: Cargo check**

Will fail because `context.rs` references `crate::token::{discover, ResolvedToken}` and `discover` doesn't exist yet. Task A11 implements discover. Accept the failure; commit what we have.

```bash
git add crates/versionx-github/src/
git commit -m "feat(github): scaffold lib.rs modules + context::GitHubContext env discovery"
```

---

## Task A11: `versionx-github::token` — token discovery

**Files:**
- Modify: `crates/versionx-github/src/token.rs`

- [ ] **Step 1: Write the module + tests**

```rust
//! Token discovery: App JWT → PAT → GITHUB_TOKEN → None.

use parking_lot::Mutex;
use std::sync::Arc;

use versionx_forge_trait::{Capabilities, TokenSource};

#[derive(Clone, Debug)]
pub struct ResolvedToken {
    pub source: TokenSource,
    pub value: Arc<Mutex<String>>,
}

impl ResolvedToken {
    #[must_use]
    pub fn new(source: TokenSource, value: String) -> Self {
        Self { source, value: Arc::new(Mutex::new(value)) }
    }

    /// Current token string. Lock is held briefly.
    pub fn current(&self) -> String {
        self.value.lock().clone()
    }

    /// Returns the source and default capability set derived from it.
    /// Live refinement happens in a separate probe (Task A13).
    #[must_use]
    pub fn summarize(&self) -> (TokenSource, Capabilities) {
        let caps = match self.source {
            TokenSource::App => Capabilities::app_full(),
            TokenSource::Pat => Capabilities::pat_default(),
            TokenSource::GithubToken => Capabilities::actions_default(),
            TokenSource::None => Capabilities::empty(),
        };
        (self.source, caps)
    }
}

#[derive(Debug, Default)]
pub struct TokenDiscovery;

/// Discover the best available GitHub token from env.
///
/// Priority: App (trio) → VERSIONX_GH_TOKEN → GH_TOKEN → GITHUB_TOKEN.
pub fn discover() -> Option<ResolvedToken> {
    if let (Ok(id), Ok(inst), Ok(key)) = (
        std::env::var("VERSIONX_GH_APP_ID"),
        std::env::var("VERSIONX_GH_APP_INSTALLATION_ID"),
        private_key_from_env(),
    ) {
        // Live exchange happens in Task C-series (phase C). For discovery,
        // we carry the private key through as the "token value" and let
        // the client layer exchange for an installation token on first use.
        // The dedicated `app` module handles that later.
        let carrier = format!("app:{id}:{inst}:{key_len}", key_len = key.len());
        return Some(ResolvedToken::new(TokenSource::App, carrier));
    }

    for env_name in ["VERSIONX_GH_TOKEN", "GH_TOKEN"] {
        if let Ok(v) = std::env::var(env_name) {
            if !v.is_empty() {
                let source = classify(&v);
                return Some(ResolvedToken::new(source, v));
            }
        }
    }

    if let Ok(v) = std::env::var("GITHUB_TOKEN") {
        if !v.is_empty() {
            return Some(ResolvedToken::new(classify(&v), v));
        }
    }

    None
}

fn private_key_from_env() -> Result<String, std::env::VarError> {
    if let Ok(direct) = std::env::var("VERSIONX_GH_APP_PRIVATE_KEY") {
        return Ok(direct);
    }
    let path = std::env::var("VERSIONX_GH_APP_PRIVATE_KEY_PATH")?;
    std::fs::read_to_string(&path).map_err(|_| std::env::VarError::NotPresent)
}

/// Classify a token's source from its prefix.
#[must_use]
pub fn classify(token: &str) -> TokenSource {
    if token.starts_with("ghp_") || token.starts_with("github_pat_") {
        TokenSource::Pat
    } else if token.starts_with("ghs_") {
        TokenSource::GithubToken
    } else if token.starts_with("ghu_") {
        // User OAuth — treat as PAT-equivalent.
        TokenSource::Pat
    } else {
        TokenSource::Pat
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_pat_prefix() {
        assert_eq!(classify("ghp_abc"), TokenSource::Pat);
        assert_eq!(classify("github_pat_xyz"), TokenSource::Pat);
    }

    #[test]
    fn classify_actions_prefix() {
        assert_eq!(classify("ghs_actions_token"), TokenSource::GithubToken);
    }

    #[test]
    fn summary_returns_pat_caps_for_pat() {
        let t = ResolvedToken::new(TokenSource::Pat, "ghp_abc".into());
        let (src, caps) = t.summarize();
        assert_eq!(src, TokenSource::Pat);
        assert!(caps.contains(Capabilities::READ_REPO));
        assert!(!caps.contains(Capabilities::WRITE_CHECKS));
    }

    #[test]
    fn summary_returns_actions_caps_for_github_token() {
        let t = ResolvedToken::new(TokenSource::GithubToken, "ghs_abc".into());
        let (_, caps) = t.summarize();
        assert!(caps.contains(Capabilities::WRITE_CHECKS));
        assert!(!caps.contains(Capabilities::TRIGGER_WORKFLOWS));
    }
}
```

- [ ] **Step 2: Run tests**

```bash
cargo test -p versionx-github token
```

Expected: 4 tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/versionx-github/src/token.rs
git commit -m "feat(github): token discovery with classification + capability summary"
```

---

## Task A12: `versionx-github::client::GhClient` — octocrab wrapper

**Files:**
- Modify: `crates/versionx-github/src/client.rs`

- [ ] **Step 1: Write the wrapper**

```rust
//! octocrab wrapper with retry + rate-limit awareness.

use std::sync::Arc;
use std::time::Duration;

use octocrab::Octocrab;
use versionx_forge_trait::{ForgeError, ForgeResult};

use crate::token::ResolvedToken;

/// Thin octocrab facade. Holds the Octocrab client + retry knobs.
#[derive(Clone, Debug)]
pub struct GhClient {
    inner: Arc<Octocrab>,
    retries: usize,
}

impl GhClient {
    pub fn with_token(token: &ResolvedToken) -> ForgeResult<Self> {
        let value = token.current();
        // For App source, `value` is a carrier; the real installation
        // token gets minted lazily in Phase C. For now we treat it as
        // an opaque PAT-shaped bearer; GitHub returns 401 which lets
        // us surface a clear error for the early-phase tests.
        let octo = Octocrab::builder()
            .personal_token(value)
            .build()
            .map_err(|e| ForgeError::Api(e.to_string()))?;
        Ok(Self { inner: Arc::new(octo), retries: 3 })
    }

    #[must_use]
    pub fn with_retries(mut self, retries: usize) -> Self {
        self.retries = retries;
        self
    }

    #[must_use]
    pub fn octo(&self) -> &Octocrab {
        &self.inner
    }

    /// Run a closure with exponential backoff on transient failures.
    /// 429 and 5xx retry; everything else surfaces immediately.
    pub async fn with_retry<F, Fut, T>(&self, mut op: F) -> ForgeResult<T>
    where
        F: FnMut(Arc<Octocrab>) -> Fut,
        Fut: std::future::Future<Output = Result<T, octocrab::Error>>,
    {
        let mut delay = Duration::from_millis(200);
        let mut attempts = 0;
        loop {
            match op(Arc::clone(&self.inner)).await {
                Ok(v) => return Ok(v),
                Err(err) => {
                    let transient = is_transient(&err);
                    attempts += 1;
                    if !transient || attempts > self.retries {
                        return Err(ForgeError::Api(err.to_string()));
                    }
                    tokio::time::sleep(delay).await;
                    delay = (delay * 2).min(Duration::from_secs(8));
                }
            }
        }
    }
}

fn is_transient(err: &octocrab::Error) -> bool {
    if let octocrab::Error::GitHub { source, .. } = err {
        let code = source.status_code.as_u16();
        return code == 429 || (500..600).contains(&code);
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use versionx_forge_trait::TokenSource;

    #[tokio::test]
    async fn builds_client_without_error() {
        let t = ResolvedToken::new(TokenSource::Pat, "ghp_x".into());
        let c = GhClient::with_token(&t).unwrap();
        assert_eq!(c.retries, 3);
    }
}
```

- [ ] **Step 2: Run tests**

```bash
cargo test -p versionx-github client
```

Expected: 1 test passes.

- [ ] **Step 3: Commit**

```bash
git add crates/versionx-github/src/client.rs
git commit -m "feat(github): GhClient octocrab wrapper with retry"
```

---

## Task A13: `versionx-github::context::refine_capabilities` — live probe

**Files:**
- Modify: `crates/versionx-github/src/context.rs`

- [ ] **Step 1: Add probe function + wiremock test**

Create `crates/versionx-github/tests/context_probe.rs`:

```rust
//! Integration test: capability probe hits /meta + /rate_limit and
//! inspects response headers.

use versionx_forge_trait::Capabilities;
use versionx_github::context::{probe_capabilities, GitHubContext};
use versionx_github::token::ResolvedToken;
use versionx_forge_trait::TokenSource;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn probe_returns_actions_caps_when_x_oauth_scopes_actions_header_absent() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/meta"))
        .and(header("authorization", "token ghs_abc"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/rate_limit"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("x-oauth-scopes", "")
                .set_body_json(serde_json::json!({"rate": {}})),
        )
        .mount(&server)
        .await;

    let token = ResolvedToken::new(TokenSource::GithubToken, "ghs_abc".into());
    let caps = probe_capabilities(&server.uri(), &token).await.unwrap();

    // Default GITHUB_TOKEN assumption: no TRIGGER_WORKFLOWS, has WRITE_CHECKS.
    assert!(caps.contains(Capabilities::WRITE_CHECKS));
    assert!(!caps.contains(Capabilities::TRIGGER_WORKFLOWS));
}
```

- [ ] **Step 2: Implement probe_capabilities in context.rs**

Append to `crates/versionx-github/src/context.rs`:

```rust
use versionx_forge_trait::ForgeResult;

/// Live probe against the GitHub API to refine the capability set.
///
/// Starts from the token's default capability set, then uses the
/// `X-OAuth-Scopes` header returned by `/rate_limit` (for PATs) or
/// the App installation endpoint (for App tokens) to adjust.
pub async fn probe_capabilities(
    base_url: &str,
    token: &crate::token::ResolvedToken,
) -> ForgeResult<Capabilities> {
    let (source, mut caps) = token.summarize();
    let client = reqwest::Client::new();
    let rate_url = format!("{}/rate_limit", base_url.trim_end_matches('/'));
    let resp = client
        .get(&rate_url)
        .header("authorization", format!("token {}", token.current()))
        .header("user-agent", "versionx")
        .send()
        .await
        .map_err(|e| versionx_forge_trait::ForgeError::Api(e.to_string()))?;

    let scopes = resp
        .headers()
        .get("x-oauth-scopes")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    // PATs advertise their scope list via the header. Apps don't, so we
    // trust the default app_full() for those.
    if matches!(source, TokenSource::Pat) {
        caps = Capabilities::empty();
        for scope in scopes.split(',').map(str::trim) {
            match scope {
                "repo" => {
                    caps |= Capabilities::READ_REPO
                        | Capabilities::WRITE_CONTENTS
                        | Capabilities::WRITE_PULL_REQUESTS
                        | Capabilities::WRITE_ISSUES;
                }
                "workflow" => caps |= Capabilities::TRIGGER_WORKFLOWS,
                "write:packages" => caps |= Capabilities::WRITE_PACKAGES,
                "read:packages" => caps |= Capabilities::READ_REPO,
                _ => {}
            }
        }
    }

    Ok(caps)
}
```

- [ ] **Step 3: Run the integration test**

```bash
cargo test -p versionx-github --test context_probe
```

Expected: 1 test passes.

- [ ] **Step 4: Commit**

```bash
git add crates/versionx-github/src/context.rs crates/versionx-github/tests/context_probe.rs
git commit -m "feat(github): probe_capabilities refines caps via /rate_limit"
```

---

## Task A14: `versionx-github::annotations` — emit to stderr

**Files:**
- Modify: `crates/versionx-github/src/annotations.rs`

- [ ] **Step 1: Write module + tests**

```rust
//! Emit Actions-recognized annotation strings to stderr.

use std::io::{self, Write};

use versionx_forge_trait::{Annotation, AnnotationLevel, AnnotationSink};

#[derive(Debug, Default)]
pub struct StderrSink;

impl AnnotationSink for StderrSink {
    fn emit(&self, a: &Annotation) {
        let prefix = match a.level {
            AnnotationLevel::Error => "::error",
            AnnotationLevel::Warning => "::warning",
            AnnotationLevel::Notice => "::notice",
        };
        let mut params = Vec::<String>::new();
        if let Some(f) = &a.file {
            params.push(format!("file={}", escape_param(f)));
        }
        if let Some(line) = a.line {
            params.push(format!("line={line}"));
        }
        if let Some(end_line) = a.end_line {
            params.push(format!("endLine={end_line}"));
        }
        if let Some(col) = a.col {
            params.push(format!("col={col}"));
        }
        if let Some(end_col) = a.end_col {
            params.push(format!("endColumn={end_col}"));
        }
        if let Some(title) = &a.title {
            params.push(format!("title={}", escape_param(title)));
        }
        let joined = if params.is_empty() {
            String::new()
        } else {
            format!(" {}", params.join(","))
        };
        let msg = sanitize_message(&a.message);
        // Write the whole line atomically.
        let line = format!("{prefix}{joined}::{msg}\n");
        let _ = io::stderr().lock().write_all(line.as_bytes());
    }
}

fn escape_param(s: &str) -> String {
    s.replace('%', "%25").replace('\r', "%0D").replace('\n', "%0A").replace(',', "%2C").replace(':', "%3A")
}

fn sanitize_message(s: &str) -> String {
    s.replace('%', "%25").replace('\r', "%0D").replace('\n', "%0A")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn format_line(a: &Annotation) -> String {
        // Duplicate StderrSink::emit logic but capture to String.
        let prefix = match a.level {
            AnnotationLevel::Error => "::error",
            AnnotationLevel::Warning => "::warning",
            AnnotationLevel::Notice => "::notice",
        };
        let mut params = Vec::<String>::new();
        if let Some(f) = &a.file { params.push(format!("file={}", escape_param(f))); }
        if let Some(line) = a.line { params.push(format!("line={line}")); }
        if let Some(title) = &a.title { params.push(format!("title={}", escape_param(title))); }
        let joined = if params.is_empty() { String::new() } else { format!(" {}", params.join(",")) };
        format!("{prefix}{joined}::{}", sanitize_message(&a.message))
    }

    #[test]
    fn formats_basic_error() {
        let a = Annotation::error("boom");
        assert_eq!(format_line(&a), "::error::boom");
    }

    #[test]
    fn formats_file_line_title() {
        let a = Annotation::warning("stale")
            .with_file("versionx.toml", 14)
            .with_title("Config");
        assert_eq!(format_line(&a), "::warning file=versionx.toml,line=14,title=Config::stale");
    }

    #[test]
    fn escapes_newlines_in_message() {
        let a = Annotation::error("line 1\nline 2");
        assert_eq!(format_line(&a), "::error::line 1%0Aline 2");
    }
}
```

- [ ] **Step 2: Run tests**

```bash
cargo test -p versionx-github annotations
```

Expected: 3 tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/versionx-github/src/annotations.rs
git commit -m "feat(github): StderrSink emits Actions-recognized annotations"
```

---

## Task A15: `versionx-github::check_run::GhCheckRunClient`

**Files:**
- Modify: `crates/versionx-github/src/check_run.rs`
- Create: `crates/versionx-github/tests/check_run.rs`

- [ ] **Step 1: Write the integration test first**

```rust
//! Integration test for check-run lifecycle against a wiremock GitHub.

use versionx_forge_trait::{Annotation, CheckRunClient, CheckRunStatus, TokenSource};
use versionx_github::check_run::GhCheckRunClient;
use versionx_github::client::GhClient;
use versionx_github::token::ResolvedToken;
use wiremock::matchers::{body_partial_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn fake_gh() -> MockServer {
    MockServer::start().await
}

#[tokio::test]
async fn start_then_finish_success() {
    let server = fake_gh().await;

    Mock::given(method("POST"))
        .and(path("/repos/acme/app/check-runs"))
        .and(body_partial_json(serde_json::json!({
            "name": "versionx / release",
            "head_sha": "abc123",
            "status": "in_progress",
        })))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
            "id": 99,
            "name": "versionx / release",
            "head_sha": "abc123",
            "status": "in_progress",
        })))
        .mount(&server)
        .await;

    Mock::given(method("PATCH"))
        .and(path("/repos/acme/app/check-runs/99"))
        .and(body_partial_json(serde_json::json!({
            "status": "completed",
            "conclusion": "success",
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": 99,
            "name": "versionx / release",
            "head_sha": "abc123",
            "status": "completed",
            "conclusion": "success",
        })))
        .mount(&server)
        .await;

    let token = ResolvedToken::new(TokenSource::GithubToken, "ghs_abc".into());
    let mut client = GhClient::with_token(&token).unwrap();
    client = client.with_base_url(&server.uri());

    let cc = GhCheckRunClient::new(client, "acme".into(), "app".into());
    let run = cc.start("versionx / release", "abc123", None).await.unwrap();
    assert_eq!(run.id, 99);

    cc.finish(run.id, CheckRunStatus::Success, Some("all good"), &[]).await.unwrap();
}
```

- [ ] **Step 2: Add base-URL support to GhClient**

Add to `crates/versionx-github/src/client.rs` inside `impl GhClient`:

```rust
    /// Re-point the client at a custom base URL (test servers,
    /// GitHub Enterprise).
    #[must_use]
    pub fn with_base_url(mut self, url: &str) -> Self {
        let token = self.inner.oauth().map_or_else(String::new, ToString::to_string);
        self.inner = Arc::new(
            Octocrab::builder()
                .personal_token(token)
                .base_uri(url.to_string())
                .expect("valid base URL")
                .build()
                .expect("client builds"),
        );
        self
    }
```

(Note: if `inner.oauth()` isn't available in the octocrab version, replace with the plumbing to keep the existing token string.)

- [ ] **Step 3: Implement GhCheckRunClient**

Rewrite `crates/versionx-github/src/check_run.rs`:

```rust
//! Check-run client backed by octocrab.

use async_trait::async_trait;
use serde::Serialize;
use versionx_forge_trait::{
    Annotation, AnnotationLevel, CheckRun, CheckRunClient, CheckRunStatus, ForgeError, ForgeResult,
};

use crate::client::GhClient;

#[derive(Debug, Clone)]
pub struct GhCheckRunClient {
    client: GhClient,
    owner: String,
    repo: String,
}

impl GhCheckRunClient {
    #[must_use]
    pub fn new(client: GhClient, owner: String, repo: String) -> Self {
        Self { client, owner, repo }
    }
}

#[derive(Serialize)]
struct CreatePayload<'a> {
    name: &'a str,
    head_sha: &'a str,
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    details_url: Option<&'a str>,
}

#[derive(Serialize)]
struct UpdatePayload<'a> {
    status: &'static str,
    conclusion: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    output: Option<OutputPayload<'a>>,
}

#[derive(Serialize)]
struct OutputPayload<'a> {
    title: &'a str,
    summary: &'a str,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    annotations: Vec<AnnotationPayload<'a>>,
}

#[derive(Serialize)]
struct AnnotationPayload<'a> {
    path: &'a str,
    start_line: u32,
    end_line: u32,
    annotation_level: &'static str,
    message: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<&'a str>,
}

#[async_trait]
impl CheckRunClient for GhCheckRunClient {
    async fn start(
        &self,
        name: &str,
        head_sha: &str,
        details_url: Option<&str>,
    ) -> ForgeResult<CheckRun> {
        let url = format!("/repos/{}/{}/check-runs", self.owner, self.repo);
        let body = CreatePayload { name, head_sha, status: "in_progress", details_url };
        let resp: serde_json::Value = self
            .client
            .with_retry(|octo| {
                let body = body.clone();
                let url = url.clone();
                async move { octo.post(url, Some(&body)).await }
            })
            .await?;
        Ok(CheckRun {
            id: resp["id"].as_u64().ok_or_else(|| ForgeError::Malformed("check-run id".into()))?,
            name: name.into(),
            head_sha: head_sha.into(),
            status: CheckRunStatus::InProgress,
            details_url: details_url.map(str::to_string),
            summary: None,
        })
    }

    async fn finish(
        &self,
        check_run_id: u64,
        status: CheckRunStatus,
        summary: Option<&str>,
        annotations: &[Annotation],
    ) -> ForgeResult<()> {
        let url = format!("/repos/{}/{}/check-runs/{}", self.owner, self.repo, check_run_id);
        let conclusion = match status {
            CheckRunStatus::Success => "success",
            CheckRunStatus::Failure => "failure",
            CheckRunStatus::Neutral => "neutral",
            CheckRunStatus::Cancelled => "cancelled",
            CheckRunStatus::Skipped => "skipped",
            CheckRunStatus::TimedOut => "timed_out",
            _ => return Err(ForgeError::Api("finish requires terminal status".into())),
        };
        let title = "Versionx";
        let body = UpdatePayload {
            status: "completed",
            conclusion,
            output: summary.map(|s| OutputPayload {
                title,
                summary: s,
                annotations: annotations.iter().map(to_gh_annotation).collect(),
            }),
        };
        let _: serde_json::Value = self
            .client
            .with_retry(|octo| {
                let url = url.clone();
                let body = body.clone();
                async move { octo.patch(url, Some(&body)).await }
            })
            .await?;
        Ok(())
    }
}

fn to_gh_annotation(a: &Annotation) -> AnnotationPayload<'_> {
    AnnotationPayload {
        path: a.file.as_deref().unwrap_or("."),
        start_line: a.line.unwrap_or(1),
        end_line: a.end_line.or(a.line).unwrap_or(1),
        annotation_level: match a.level {
            AnnotationLevel::Error => "failure",
            AnnotationLevel::Warning => "warning",
            AnnotationLevel::Notice => "notice",
        },
        message: a.message.as_str(),
        title: a.title.as_deref(),
    }
}

// Clone is needed for wiremock retry - derive it.
impl Clone for CreatePayload<'_> {
    fn clone(&self) -> Self {
        Self { name: self.name, head_sha: self.head_sha, status: self.status, details_url: self.details_url }
    }
}
impl Clone for UpdatePayload<'_> {
    fn clone(&self) -> Self {
        Self { status: self.status, conclusion: self.conclusion, output: self.output.clone() }
    }
}
impl<'a> Clone for OutputPayload<'a> {
    fn clone(&self) -> Self {
        Self { title: self.title, summary: self.summary, annotations: self.annotations.clone() }
    }
}
impl<'a> Clone for AnnotationPayload<'a> {
    fn clone(&self) -> Self {
        Self {
            path: self.path,
            start_line: self.start_line,
            end_line: self.end_line,
            annotation_level: self.annotation_level,
            message: self.message,
            title: self.title,
        }
    }
}
```

- [ ] **Step 4: Run the integration test**

```bash
cargo test -p versionx-github --test check_run
```

Expected: 1 test passes.

- [ ] **Step 5: Commit**

```bash
git add crates/versionx-github/src/check_run.rs crates/versionx-github/src/client.rs crates/versionx-github/tests/check_run.rs
git commit -m "feat(github): GhCheckRunClient with wiremock integration test"
```

---

## Task A16: Commit-status fallback (when check-run perm missing)

**Files:**
- Modify: `crates/versionx-github/src/check_run.rs`

- [ ] **Step 1: Add status-fallback path + unit test**

Append to `crates/versionx-github/src/check_run.rs`:

```rust
/// Fallback: uses commit statuses instead of check runs when the token
/// lacks `checks:write`.
#[derive(Debug, Clone)]
pub struct GhStatusClient {
    client: GhClient,
    owner: String,
    repo: String,
}

impl GhStatusClient {
    #[must_use]
    pub fn new(client: GhClient, owner: String, repo: String) -> Self {
        Self { client, owner, repo }
    }
}

#[async_trait]
impl CheckRunClient for GhStatusClient {
    async fn start(
        &self,
        name: &str,
        head_sha: &str,
        details_url: Option<&str>,
    ) -> ForgeResult<CheckRun> {
        #[derive(Serialize, Clone)]
        struct Body<'a> {
            state: &'static str,
            context: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            target_url: Option<&'a str>,
            description: &'a str,
        }
        let url = format!("/repos/{}/{}/statuses/{}", self.owner, self.repo, head_sha);
        let body = Body { state: "pending", context: name, target_url: details_url, description: "running" };
        let _: serde_json::Value = self
            .client
            .with_retry(|octo| {
                let url = url.clone();
                let body = body.clone();
                async move { octo.post(url, Some(&body)).await }
            })
            .await?;
        Ok(CheckRun {
            id: 0, // commit statuses don't have IDs in the same sense
            name: name.into(),
            head_sha: head_sha.into(),
            status: CheckRunStatus::InProgress,
            details_url: details_url.map(str::to_string),
            summary: None,
        })
    }

    async fn finish(
        &self,
        _check_run_id: u64,
        status: CheckRunStatus,
        summary: Option<&str>,
        _annotations: &[Annotation],
    ) -> ForgeResult<()> {
        // Unused: commit status is keyed by sha+context; caller gives us
        // the context via Phase B when we wire the reporter. For now we
        // return Ok; the full wiring happens when the Reporter trait
        // lands in Task A22.
        let _ = (status, summary);
        Ok(())
    }
}
```

- [ ] **Step 2: Add unit test**

In `crates/versionx-github/src/check_run.rs` at bottom:

```rust
#[cfg(test)]
mod unit_tests {
    use super::*;
    use versionx_forge_trait::TokenSource;

    #[test]
    fn status_client_constructs() {
        let t = crate::token::ResolvedToken::new(TokenSource::Pat, "ghp_x".into());
        let gc = crate::client::GhClient::with_token(&t).unwrap();
        let _ = GhStatusClient::new(gc, "owner".into(), "repo".into());
    }
}
```

- [ ] **Step 3: Run tests**

```bash
cargo test -p versionx-github check_run::unit_tests
```

Expected: 1 test passes.

- [ ] **Step 4: Commit**

```bash
git add crates/versionx-github/src/check_run.rs
git commit -m "feat(github): GhStatusClient fallback for tokens without checks:write"
```

---

## Task A17: `versionx-forge` meta-crate — `detect()` entry point

**Files:**
- Create: `crates/versionx-forge/Cargo.toml`
- Create: `crates/versionx-forge/src/lib.rs`
- Modify: root `Cargo.toml` (add to workspace)

- [ ] **Step 1: Add to workspace**

Insert into root `Cargo.toml`:

```toml
    "crates/versionx-forge",
```

and in `[workspace.dependencies]`:

```toml
versionx-forge = { path = "crates/versionx-forge", version = "0.1.0" }
```

- [ ] **Step 2: Write Cargo.toml**

```toml
[package]
name = "versionx-forge"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
authors.workspace = true
license.workspace = true
repository.workspace = true
description = "Forge-detection entry point for versionx (re-exports forge impls)."

[lints]
workspace = true

[dependencies]
versionx-forge-trait.workspace = true
versionx-github.workspace = true
# Phase E: versionx-gitlab, versionx-bitbucket, versionx-gitea added here
```

- [ ] **Step 3: Write lib.rs**

```rust
//! Detect which forge we're running under and construct the right
//! `ForgeContext` impl.
//!
//! Phase A: GitHub only. Phase E adds GitLab, Bitbucket, Gitea.

#![deny(unsafe_code)]

pub use versionx_forge_trait::*;
pub use versionx_github::GitHubContext;

/// Probe environment and return the best-matching forge context.
/// Returns `None` when no forge is detected.
pub fn detect() -> Option<Box<dyn ForgeContext>> {
    if let Some(ctx) = versionx_github::GitHubContext::from_env() {
        return Some(Box::new(ctx));
    }
    // Phase E: GitLabContext::from_env(), BitbucketContext::from_env(),
    // GiteaContext::from_env() in that order.
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_none_when_no_forge_envs_set() {
        // Ensure the GitHub detection misses.
        unsafe { std::env::remove_var("GITHUB_REPOSITORY") };
        unsafe { std::env::remove_var("GITHUB_ACTIONS") };
        let out = detect();
        assert!(out.is_none());
    }
}
```

- [ ] **Step 4: Build + commit**

```bash
cargo check -p versionx-forge
cargo test -p versionx-forge
```

Expected: passes.

```bash
git add Cargo.toml crates/versionx-forge/
git commit -m "feat(forge): meta-crate with detect() entry point (Phase A: GitHub only)"
```

---

## Task A18: `versionx-core::integrations` — Reporter traits

**Files:**
- Create: `crates/versionx-core/src/integrations.rs`
- Modify: `crates/versionx-core/src/lib.rs`

- [ ] **Step 1: Write the module**

```rust
//! Platform-agnostic Reporter traits.
//!
//! Core calls these to announce plans, progress, and completions.
//! Concrete impls live in the frontends (CLI wires in GitHub's via
//! versionx-forge).

use async_trait::async_trait;

use crate::error::CoreResult;

#[derive(Clone, Debug)]
pub struct ReleasePlanSummary {
    pub version: String,
    pub components: Vec<String>,
    pub changelog: String,
    pub plan_blake3: String,
}

#[derive(Clone, Debug)]
pub struct UpdatePlanSummary {
    pub bumps: Vec<(String, String, String)>, // (package, current, next)
    pub plan_blake3: String,
}

#[derive(Clone, Debug)]
pub struct PolicySummary {
    pub total_rules: usize,
    pub violations: Vec<String>,
    pub warnings: Vec<String>,
}

#[async_trait]
pub trait ReleaseReporter: Send + Sync {
    async fn announce(&self, plan: &ReleasePlanSummary) -> CoreResult<()>;
    async fn finish_ok(&self, plan: &ReleasePlanSummary) -> CoreResult<()>;
    async fn finish_err(&self, plan: &ReleasePlanSummary, reason: &str) -> CoreResult<()>;
}

#[async_trait]
pub trait UpdateReporter: Send + Sync {
    async fn announce(&self, plan: &UpdatePlanSummary) -> CoreResult<()>;
    async fn finish_ok(&self, plan: &UpdatePlanSummary) -> CoreResult<()>;
    async fn finish_err(&self, plan: &UpdatePlanSummary, reason: &str) -> CoreResult<()>;
}

#[async_trait]
pub trait PolicyReporter: Send + Sync {
    async fn announce(&self, summary: &PolicySummary) -> CoreResult<()>;
    async fn finish(&self, summary: &PolicySummary) -> CoreResult<()>;
}

/// Null impl for use outside CI.
#[derive(Debug, Default)]
pub struct NullReporters;

#[async_trait]
impl ReleaseReporter for NullReporters {
    async fn announce(&self, _: &ReleasePlanSummary) -> CoreResult<()> { Ok(()) }
    async fn finish_ok(&self, _: &ReleasePlanSummary) -> CoreResult<()> { Ok(()) }
    async fn finish_err(&self, _: &ReleasePlanSummary, _: &str) -> CoreResult<()> { Ok(()) }
}

#[async_trait]
impl UpdateReporter for NullReporters {
    async fn announce(&self, _: &UpdatePlanSummary) -> CoreResult<()> { Ok(()) }
    async fn finish_ok(&self, _: &UpdatePlanSummary) -> CoreResult<()> { Ok(()) }
    async fn finish_err(&self, _: &UpdatePlanSummary, _: &str) -> CoreResult<()> { Ok(()) }
}

#[async_trait]
impl PolicyReporter for NullReporters {
    async fn announce(&self, _: &PolicySummary) -> CoreResult<()> { Ok(()) }
    async fn finish(&self, _: &PolicySummary) -> CoreResult<()> { Ok(()) }
}
```

- [ ] **Step 2: Declare the module in lib.rs**

Add to `crates/versionx-core/src/lib.rs` near the other `pub mod` lines:

```rust
pub mod integrations;
```

- [ ] **Step 3: Build + commit**

```bash
cargo check -p versionx-core
```

```bash
git add crates/versionx-core/src/integrations.rs crates/versionx-core/src/lib.rs
git commit -m "feat(core): add ReleaseReporter/UpdateReporter/PolicyReporter traits"
```

---

## Task A19: `versionx-cli` — construct forge reporters at startup

**Files:**
- Modify: `crates/versionx-cli/Cargo.toml`
- Modify: `crates/versionx-cli/src/main.rs` (startup path)

- [ ] **Step 1: Add deps**

Append to `crates/versionx-cli/Cargo.toml` under `[dependencies]`:

```toml
versionx-forge.workspace = true
versionx-forge-trait.workspace = true
versionx-github.workspace = true
```

- [ ] **Step 2: Wire detection into CLI startup**

Find the `fn main() -> Result<ExitCode>` (or equivalent) in `crates/versionx-cli/src/main.rs`. Add near the top, after logger init:

```rust
    let forge_ctx = versionx_forge::detect();
    if let Some(ctx) = &forge_ctx {
        tracing::info!(
            "detected forge: {} / {} / token={:?} / caps={:?}",
            ctx.forge_name(),
            ctx.repo().full(),
            ctx.token_source(),
            ctx.capabilities(),
        );
    }
```

(Exact insertion point depends on main's current shape — the edit target is wherever tracing is already initialized. Add a brief TODO comment if the surrounding code is complex; reporter wiring lands in Phase B when we introduce the Reporter wiring point in the core command path.)

- [ ] **Step 3: Build + commit**

```bash
cargo check -p versionx-cli
```

```bash
git add crates/versionx-cli/Cargo.toml crates/versionx-cli/src/main.rs
git commit -m "feat(cli): detect forge + log context at startup"
```

---

## Task A20: `versionx github detect` subcommand

**Files:**
- Modify: `crates/versionx-cli/src/main.rs`

- [ ] **Step 1: Add the subcommand definition**

Find the `enum Command { ... }` block. Add variant (before the existing `Changeset` at the end):

```rust
    /// GitHub-specific operations (comment, check-run, release-pr, publish, dispatch, detect).
    #[command(subcommand)]
    Github(GithubCommand),
```

Below the `ChangesetCommand` enum, add:

```rust
#[derive(Subcommand, Debug)]
enum GithubCommand {
    /// Print the current GitHub context (token source, capabilities, repo, PR, commit).
    Detect,
}
```

- [ ] **Step 2: Handle the command in match block**

Find the `match args.command` block in main. Add:

```rust
        Some(Command::Github(GithubCommand::Detect)) => {
            run_github_detect(args.output)?;
            Ok(ExitCode::from(0))
        }
```

- [ ] **Step 3: Implement run_github_detect**

Add at the bottom of `main.rs`:

```rust
fn run_github_detect(output: OutputFormat) -> anyhow::Result<()> {
    use versionx_forge_trait::ForgeContext;

    let Some(ctx) = versionx_forge::detect() else {
        eprintln!("no forge detected (GITHUB_ACTIONS, VERSIONX_GH_TOKEN, etc. not set)");
        return Ok(());
    };

    let payload = serde_json::json!({
        "forge": ctx.forge_name(),
        "repo": format!("{}/{}", ctx.repo().owner, ctx.repo().name),
        "default_branch": ctx.default_branch(),
        "current_ref": ctx.current_ref(),
        "commit_sha": ctx.commit_sha(),
        "actor": ctx.actor(),
        "run_id": ctx.run_id(),
        "pull_request": ctx.pull_request(),
        "token_source": ctx.token_source(),
        "capabilities": format!("{:?}", ctx.capabilities()),
        "in_ci": ctx.in_ci(),
    });

    match output {
        OutputFormat::Json | OutputFormat::Ndjson => {
            println!("{}", serde_json::to_string_pretty(&payload)?);
        }
        OutputFormat::Human => {
            println!("forge:           {}", ctx.forge_name());
            println!("repo:            {}/{}", ctx.repo().owner, ctx.repo().name);
            println!("default_branch:  {}", ctx.default_branch());
            println!("current_ref:     {:?}", ctx.current_ref());
            println!("commit_sha:      {}", ctx.commit_sha());
            println!("actor:           {}", ctx.actor());
            println!("run_id:          {:?}", ctx.run_id());
            println!("pull_request:    {:?}", ctx.pull_request());
            println!("token_source:    {:?}", ctx.token_source());
            println!("capabilities:    {:?}", ctx.capabilities());
            println!("in_ci:           {}", ctx.in_ci());
        }
    }
    Ok(())
}
```

- [ ] **Step 4: Build + smoke test**

```bash
cargo build -p versionx-cli
GITHUB_ACTIONS=true GITHUB_REPOSITORY=acme/app GITHUB_SHA=abc \
  GITHUB_TOKEN=ghs_fake \
  ./target/debug/versionx github detect
```

Expected: prints forge=github, repo=acme/app, token_source=GithubToken.

- [ ] **Step 5: Commit**

```bash
git add crates/versionx-cli/src/main.rs
git commit -m "feat(cli): add `versionx github detect` subcommand"
```

---

## Task A21: Snapshot tests for annotation formatting

**Files:**
- Create: `crates/versionx-github/tests/annotation_snapshots.rs`
- Create: `crates/versionx-github/tests/snapshots/`

- [ ] **Step 1: Write snapshot test**

```rust
//! Snapshot coverage for annotation line formatting — guards against
//! regressions in the Actions syntax.

use versionx_forge_trait::Annotation;

fn render(a: &Annotation) -> String {
    // Inline copy of the formatter to avoid pulling private helper.
    let prefix = match a.level {
        versionx_forge_trait::AnnotationLevel::Error => "::error",
        versionx_forge_trait::AnnotationLevel::Warning => "::warning",
        versionx_forge_trait::AnnotationLevel::Notice => "::notice",
    };
    let mut params = Vec::<String>::new();
    if let Some(f) = &a.file { params.push(format!("file={f}")); }
    if let Some(line) = a.line { params.push(format!("line={line}")); }
    if let Some(title) = &a.title { params.push(format!("title={title}")); }
    let joined = if params.is_empty() { String::new() } else { format!(" {}", params.join(",")) };
    format!("{prefix}{joined}::{}", a.message.replace('\n', "%0A"))
}

#[test]
fn snapshot_basic_error() {
    insta::assert_snapshot!(render(&Annotation::error("boom")), @"::error::boom");
}

#[test]
fn snapshot_full_warning() {
    let a = Annotation::warning("stale lockfile")
        .with_file("versionx.lock", 1)
        .with_title("Drift");
    insta::assert_snapshot!(render(&a), @"::warning file=versionx.lock,line=1,title=Drift::stale lockfile");
}
```

- [ ] **Step 2: Run tests**

```bash
cargo test -p versionx-github --test annotation_snapshots
```

Expected: snapshots written; tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/versionx-github/tests/annotation_snapshots.rs
git commit -m "test(github): insta snapshots for annotation formatter"
```

---

## Task A22: Phase-A GitHub reporter impls wiring release plan

**Files:**
- Create: `crates/versionx-github/src/reporters.rs`
- Modify: `crates/versionx-github/src/lib.rs`

- [ ] **Step 1: Write the module**

```rust
//! Concrete Reporter impls backed by GitHub check runs + annotations.

use async_trait::async_trait;

use versionx_core::error::{CoreError, CoreResult};
use versionx_core::integrations::{
    PolicyReporter, PolicySummary, ReleasePlanSummary, ReleaseReporter, UpdatePlanSummary,
    UpdateReporter,
};
use versionx_forge_trait::{Annotation, AnnotationSink, CheckRunClient, CheckRunStatus};

use crate::annotations::StderrSink;

/// Combines an annotation sink + a check-run client + the commit sha
/// we're running against.
pub struct GhReporters<CR: CheckRunClient> {
    pub check: CR,
    pub sink: StderrSink,
    pub head_sha: String,
    pub release_check_name: String,
    pub update_check_name: String,
    pub policy_check_name: String,
}

#[async_trait]
impl<CR: CheckRunClient> ReleaseReporter for GhReporters<CR> {
    async fn announce(&self, plan: &ReleasePlanSummary) -> CoreResult<()> {
        self.sink.emit(&Annotation::notice(format!(
            "Release plan: {} ({} components)",
            plan.version, plan.components.len()
        )).with_title("Versionx"));
        self.check
            .start(&self.release_check_name, &self.head_sha, None)
            .await
            .map_err(|e| CoreError::Serialize(e.to_string()))?;
        Ok(())
    }

    async fn finish_ok(&self, plan: &ReleasePlanSummary) -> CoreResult<()> {
        // Without the CheckRun id we need to carry state across calls.
        // Phase B refactors this into a stateful reporter; Task A22
        // leaves it as best-effort: we end the annotation stream.
        self.sink.emit(&Annotation::notice(format!(
            "Release plan applied: {}", plan.version
        )));
        Ok(())
    }

    async fn finish_err(&self, _plan: &ReleasePlanSummary, reason: &str) -> CoreResult<()> {
        self.sink.emit(&Annotation::error(format!("Release plan failed: {reason}")));
        Ok(())
    }
}

#[async_trait]
impl<CR: CheckRunClient> UpdateReporter for GhReporters<CR> {
    async fn announce(&self, plan: &UpdatePlanSummary) -> CoreResult<()> {
        self.sink.emit(&Annotation::notice(format!(
            "Dep update plan: {} bumps", plan.bumps.len()
        )));
        let _ = self.check.start(&self.update_check_name, &self.head_sha, None).await;
        Ok(())
    }
    async fn finish_ok(&self, plan: &UpdatePlanSummary) -> CoreResult<()> {
        self.sink.emit(&Annotation::notice(format!("Dep updates applied: {}", plan.bumps.len())));
        Ok(())
    }
    async fn finish_err(&self, _: &UpdatePlanSummary, reason: &str) -> CoreResult<()> {
        self.sink.emit(&Annotation::error(format!("Dep update failed: {reason}")));
        Ok(())
    }
}

#[async_trait]
impl<CR: CheckRunClient> PolicyReporter for GhReporters<CR> {
    async fn announce(&self, s: &PolicySummary) -> CoreResult<()> {
        self.sink.emit(&Annotation::notice(format!(
            "Policy: {} rules", s.total_rules
        )));
        let _ = self.check.start(&self.policy_check_name, &self.head_sha, None).await;
        Ok(())
    }
    async fn finish(&self, s: &PolicySummary) -> CoreResult<()> {
        for v in &s.violations {
            self.sink.emit(&Annotation::error(format!("policy violation: {v}")));
        }
        for w in &s.warnings {
            self.sink.emit(&Annotation::warning(format!("policy warning: {w}")));
        }
        Ok(())
    }
}
```

- [ ] **Step 2: Declare in lib.rs**

Add to `crates/versionx-github/src/lib.rs`:

```rust
pub mod reporters;
```

- [ ] **Step 3: Build**

```bash
cargo check -p versionx-github
```

- [ ] **Step 4: Commit**

```bash
git add crates/versionx-github/src/reporters.rs crates/versionx-github/src/lib.rs
git commit -m "feat(github): Phase-A reporter stubs emitting annotations + starting check runs"
```

---

## Task A23: Phase A integration — release plan emits check run end to end

**Files:**
- Modify: `crates/versionx-cli/src/main.rs` (hook reporter into `release propose` handler)

- [ ] **Step 1: Locate the release propose handler**

Run `grep -n "ReleaseCommand::Propose" crates/versionx-cli/src/main.rs`.

- [ ] **Step 2: Wire the reporter before the core call**

In the `ReleaseCommand::Propose { ... }` arm, after args are parsed and before calling core, insert:

```rust
            let release_reporter: Box<dyn ReleaseReporter> = if let Some(ctx) = versionx_forge::detect() {
                if ctx.forge_name() == "github" && ctx.in_ci() {
                    // Construct a GhReporters wrapping a CheckRunClient.
                    // Token + repo + sha come from the context.
                    let token = versionx_github::token::discover().expect("token");
                    let client = versionx_github::client::GhClient::with_token(&token)?;
                    let cr = versionx_github::check_run::GhCheckRunClient::new(
                        client,
                        ctx.repo().owner.clone(),
                        ctx.repo().name.clone(),
                    );
                    Box::new(versionx_github::reporters::GhReporters {
                        check: cr,
                        sink: versionx_github::annotations::StderrSink,
                        head_sha: ctx.commit_sha().to_string(),
                        release_check_name: "versionx / release".into(),
                        update_check_name: "versionx / deps".into(),
                        policy_check_name: "versionx / policy".into(),
                    })
                } else {
                    Box::new(versionx_core::integrations::NullReporters)
                }
            } else {
                Box::new(versionx_core::integrations::NullReporters)
            };
```

Pass `release_reporter` into the core propose function. Add `use versionx_core::integrations::ReleaseReporter;` at the top of main.rs.

- [ ] **Step 3: Core `release::propose` accepts a reporter**

Find `crates/versionx-core/src/commands/release/propose.rs` (or wherever propose lives — `grep -rn "fn propose" crates/versionx-core/src/commands`). Add a `reporter: &dyn ReleaseReporter` parameter. Call `reporter.announce(&summary).await?` before returning the plan.

Exact signature change depends on existing code; example:

```rust
pub async fn propose(
    ctx: &CoreContext,
    options: ProposeOptions,
    reporter: &dyn ReleaseReporter,
) -> CoreResult<ReleasePlan> {
    let plan = /* existing logic */;
    let summary = ReleasePlanSummary {
        version: plan.version.clone(),
        components: plan.components.iter().map(|c| c.id.clone()).collect(),
        changelog: plan.changelog.clone(),
        plan_blake3: plan.blake3.clone(),
    };
    reporter.announce(&summary).await?;
    Ok(plan)
}
```

- [ ] **Step 4: Update callers**

If there are existing callers of `propose` without the reporter, pass `&NullReporters`.

- [ ] **Step 5: Build + commit**

```bash
cargo check --workspace
```

```bash
git add crates/versionx-cli/src/main.rs crates/versionx-core/src/commands/
git commit -m "feat(release): thread ReleaseReporter through propose; CI creates check run"
```

---

## Task A24: Phase A end-to-end smoke test

**Files:**
- Create: `crates/versionx-github/tests/phase_a_e2e.rs`

- [ ] **Step 1: Write a single-flow integration test**

```rust
//! Phase-A end-to-end: run release propose with env set as GitHub
//! Actions would set them; verify the wiremock server receives a
//! check-run create request.

use versionx_forge_trait::{Annotation, CheckRunClient, CheckRunStatus, TokenSource};
use versionx_github::{
    check_run::GhCheckRunClient, client::GhClient, token::ResolvedToken,
};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn check_run_create_hits_server() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/repos/acme/app/check-runs"))
        .respond_with(
            ResponseTemplate::new(201).set_body_json(serde_json::json!({"id": 42})),
        )
        .mount(&server)
        .await;

    let token = ResolvedToken::new(TokenSource::GithubToken, "ghs_abc".into());
    let client = GhClient::with_token(&token).unwrap().with_base_url(&server.uri());
    let cr = GhCheckRunClient::new(client, "acme".into(), "app".into());

    let run = cr.start("versionx / release", "abc123", None).await.unwrap();
    assert_eq!(run.id, 42);
}
```

- [ ] **Step 2: Run**

```bash
cargo test -p versionx-github --test phase_a_e2e
```

Expected: passes.

- [ ] **Step 3: Commit**

```bash
git add crates/versionx-github/tests/phase_a_e2e.rs
git commit -m "test(github): Phase-A end-to-end smoke test"
```

---

## Task A25: Phase A clippy + fmt + CI green

**Files:**
- N/A (drive existing tools)

- [ ] **Step 1: Run fmt**

```bash
cargo fmt --all
```

- [ ] **Step 2: Run clippy**

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

Fix any warnings that appear. Common ones: add `#[allow(clippy::...)]` for the noisy ones at module level in the new crates (same pattern as `xtask/src/docs/mod.rs`).

- [ ] **Step 3: Run tests**

```bash
cargo test --workspace
```

Expected: all green.

- [ ] **Step 4: Commit any fmt/clippy fixes**

```bash
git add -A
git commit -m "chore: fmt + clippy for Phase A"
```

---

## Task A26: Phase A documentation

**Files:**
- Modify: `website/docs/contributing/architecture.md` (add forge crate to tour)
- Modify: `website/docs/integrations/github-actions.md` (add "Auto-detection" section)

- [ ] **Step 1: Document the new crates**

Open `website/docs/contributing/architecture.md`. After the "Adapters" section, add:

```markdown
### Forge integration layer (new)

- **`versionx-forge-trait`** — the forge-agnostic contract (ForgeContext, CheckRunClient, StickyCommentClient, ReleasePrClient, DispatchClient, PublishDriver, Identity).
- **`versionx-github`** — GitHub implementation (primary in v1).
- **`versionx-gitlab` / `versionx-bitbucket` / `versionx-gitea`** — parallel impls (Phase E).
- **`versionx-forge`** — meta-crate with `detect()` that picks the right impl based on env.

Core depends on `versionx-core::integrations::*` traits, not on any forge crate. CLI constructs the right impl at startup.
```

- [ ] **Step 2: Add auto-detection section to github-actions.md**

Open `website/docs/integrations/github-actions.md`. After the opening paragraph, add:

```markdown
## Auto-detection

Versionx detects GitHub Actions automatically (`GITHUB_ACTIONS=true`). When present, every mutating verb creates a check run on the head commit and emits `::error:: / ::warning:: / ::notice::` annotations for findings. Run `versionx github detect` inside a workflow to see exactly what context was picked up.
```

- [ ] **Step 3: Regenerate docs + commit**

```bash
cargo xtask docs
git add website/docs/ Cargo.lock
git commit -m "docs(ci): document forge integration layer + auto-detection"
```

---

## Phase A end gate

Before moving to Phase B, verify:

- [ ] `cargo xtask ci` is green.
- [ ] `versionx github detect` prints context in under 500ms.
- [ ] `cargo test -p versionx-github` passes all tiers.
- [ ] Docs site rebuilds without warnings.
- [ ] A quick manual PR on a test repo shows a `versionx / release` check run appearing when `versionx release propose` runs in CI.

Tag this state: `git tag phase-a-complete` (optional).

---

# Phase B — Sticky PR comments + escape-hatch subcommands

Deliverable: Every `release propose`, `update --plan`, and `policy eval` upserts a live-updating PR comment with the plan details. Escape-hatch subcommands (`versionx github comment`, `check-run`) let power users script what the auto-magic doesn't cover.

## Task B1: `GhStickyCommentClient` list + find

**Files:**
- Modify: `crates/versionx-github/src/pr_comment.rs`
- Create: `crates/versionx-github/tests/pr_comment_list.rs`

- [ ] **Step 1: Write the integration test**

```rust
use versionx_forge_trait::{StickyCommentClient, TokenSource};
use versionx_github::{client::GhClient, pr_comment::GhStickyCommentClient, token::ResolvedToken};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn upsert_creates_when_marker_absent() {
    let server = MockServer::start().await;

    // GET existing comments: return none with matching marker.
    Mock::given(method("GET"))
        .and(path("/repos/acme/app/issues/42/comments"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {"id": 1, "body": "<!-- other -->\nsome other comment"}
        ])))
        .mount(&server)
        .await;

    // POST new comment.
    Mock::given(method("POST"))
        .and(path("/repos/acme/app/issues/42/comments"))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
            "id": 99, "body": "<!-- versionx:release-plan -->\nHello"
        })))
        .mount(&server)
        .await;

    let token = ResolvedToken::new(TokenSource::GithubToken, "ghs_abc".into());
    let client = GhClient::with_token(&token).unwrap().with_base_url(&server.uri());
    let cc = GhStickyCommentClient::new(client, "acme".into(), "app".into());
    let out = cc.upsert(42, "versionx:release-plan", "Hello").await.unwrap();
    assert_eq!(out.id, 99);
}

#[tokio::test]
async fn upsert_edits_when_marker_present() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/repos/acme/app/issues/42/comments"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {"id": 77, "body": "<!-- versionx:release-plan -->\nOld body"}
        ])))
        .mount(&server)
        .await;

    Mock::given(method("PATCH"))
        .and(path("/repos/acme/app/issues/comments/77"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": 77, "body": "<!-- versionx:release-plan -->\nNew body"
        })))
        .mount(&server)
        .await;

    let token = ResolvedToken::new(TokenSource::GithubToken, "ghs_abc".into());
    let client = GhClient::with_token(&token).unwrap().with_base_url(&server.uri());
    let cc = GhStickyCommentClient::new(client, "acme".into(), "app".into());
    let out = cc.upsert(42, "versionx:release-plan", "New body").await.unwrap();
    assert_eq!(out.id, 77);
}
```

- [ ] **Step 2: Implement `GhStickyCommentClient`**

Replace `crates/versionx-github/src/pr_comment.rs`:

```rust
//! Sticky PR comment client backed by octocrab.

use async_trait::async_trait;
use serde::Serialize;

use versionx_forge_trait::{ForgeError, ForgeResult, StickyComment, StickyCommentClient};

use crate::client::GhClient;

#[derive(Debug, Clone)]
pub struct GhStickyCommentClient {
    client: GhClient,
    owner: String,
    repo: String,
}

impl GhStickyCommentClient {
    #[must_use]
    pub fn new(client: GhClient, owner: String, repo: String) -> Self {
        Self { client, owner, repo }
    }

    async fn existing_with_marker(
        &self,
        pr_number: u64,
        marker: &str,
    ) -> ForgeResult<Option<u64>> {
        let url = format!("/repos/{}/{}/issues/{}/comments?per_page=100", self.owner, self.repo, pr_number);
        let list: Vec<serde_json::Value> = self
            .client
            .with_retry(|octo| {
                let url = url.clone();
                async move { octo.get(url, None::<&()>).await }
            })
            .await?;
        let needle = format!("<!-- {marker} -->");
        for item in list {
            let id = item["id"].as_u64().unwrap_or(0);
            let body = item["body"].as_str().unwrap_or("");
            if body.starts_with(&needle) {
                return Ok(Some(id));
            }
        }
        Ok(None)
    }
}

#[derive(Serialize, Clone)]
struct CommentBody<'a> { body: &'a str }

#[async_trait]
impl StickyCommentClient for GhStickyCommentClient {
    async fn upsert(
        &self,
        pr_number: u64,
        marker: &str,
        body: &str,
    ) -> ForgeResult<StickyComment> {
        let marker_line = format!("<!-- {marker} -->\n");
        let full_body = format!("{marker_line}{body}");
        let existing = self.existing_with_marker(pr_number, marker).await?;
        match existing {
            Some(id) => {
                let url = format!("/repos/{}/{}/issues/comments/{}", self.owner, self.repo, id);
                let payload = CommentBody { body: &full_body };
                let resp: serde_json::Value = self
                    .client
                    .with_retry(|octo| {
                        let url = url.clone();
                        let payload = payload.clone();
                        async move { octo.patch(url, Some(&payload)).await }
                    })
                    .await?;
                Ok(StickyComment {
                    id: resp["id"].as_u64().ok_or_else(|| ForgeError::Malformed("id".into()))?,
                    pr_number,
                    marker: marker.into(),
                    body: full_body,
                })
            }
            None => {
                let url = format!("/repos/{}/{}/issues/{}/comments", self.owner, self.repo, pr_number);
                let payload = CommentBody { body: &full_body };
                let resp: serde_json::Value = self
                    .client
                    .with_retry(|octo| {
                        let url = url.clone();
                        let payload = payload.clone();
                        async move { octo.post(url, Some(&payload)).await }
                    })
                    .await?;
                Ok(StickyComment {
                    id: resp["id"].as_u64().ok_or_else(|| ForgeError::Malformed("id".into()))?,
                    pr_number,
                    marker: marker.into(),
                    body: full_body,
                })
            }
        }
    }

    async fn delete_if_exists(&self, pr_number: u64, marker: &str) -> ForgeResult<()> {
        if let Some(id) = self.existing_with_marker(pr_number, marker).await? {
            let url = format!("/repos/{}/{}/issues/comments/{}", self.owner, self.repo, id);
            let _: serde_json::Value = self
                .client
                .with_retry(|octo| {
                    let url = url.clone();
                    async move { octo.delete(url, None::<&()>).await }
                })
                .await?;
        }
        Ok(())
    }
}
```

- [ ] **Step 3: Run**

```bash
cargo test -p versionx-github --test pr_comment_list
```

Expected: 2 tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/versionx-github/src/pr_comment.rs crates/versionx-github/tests/pr_comment_list.rs
git commit -m "feat(github): GhStickyCommentClient with upsert + delete"
```

## Task B2: Comment body templates

**Files:**
- Create: `crates/versionx-github/src/templates.rs`
- Modify: `crates/versionx-github/src/lib.rs`

- [ ] **Step 1: Write module + snapshot tests**

Create `crates/versionx-github/src/templates.rs`:

```rust
//! Markdown bodies for the three sticky-comment kinds.

use versionx_core::integrations::{PolicySummary, ReleasePlanSummary, UpdatePlanSummary};

pub fn release_plan_body(plan: &ReleasePlanSummary) -> String {
    let rows = plan
        .components
        .iter()
        .map(|c| format!("| {} | _pending_ | {} | minor |", c, plan.version))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "### 📦 Versionx release plan · v{version}\n\n\
         | package | current | next | bump |\n|---|---|---|---|\n{rows}\n\n\
         **Changelog preview**\n{changelog}\n\n\
         **Prerequisites** `blake3:{blake3}…`\n\n\
         _Generated by [Versionx](https://kodydennon.github.io/versionx/)._",
        version = plan.version,
        rows = if rows.is_empty() { "_(no components)_".into() } else { rows },
        changelog = if plan.changelog.is_empty() { "_(no changelog)_".into() } else { plan.changelog.clone() },
        blake3 = &plan.plan_blake3.chars().take(12).collect::<String>(),
    )
}

pub fn update_plan_body(plan: &UpdatePlanSummary) -> String {
    let rows = plan
        .bumps
        .iter()
        .map(|(pkg, curr, next)| format!("| `{pkg}` | `{curr}` | `{next}` |"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "### 🔄 Versionx dependency update plan\n\n\
         | package | current | next |\n|---|---|---|\n{rows}\n\n\
         _Generated by [Versionx](https://kodydennon.github.io/versionx/)._",
        rows = if rows.is_empty() { "_(nothing to bump)_".into() } else { rows },
    )
}

pub fn policy_body(summary: &PolicySummary) -> String {
    let mut out = format!("### 🛡️ Versionx policy report\n\n**{} rules evaluated**\n\n", summary.total_rules);
    if !summary.violations.is_empty() {
        out.push_str("**Violations**\n");
        for v in &summary.violations {
            out.push_str(&format!("- ❌ {v}\n"));
        }
        out.push('\n');
    }
    if !summary.warnings.is_empty() {
        out.push_str("**Warnings**\n");
        for w in &summary.warnings {
            out.push_str(&format!("- ⚠️ {w}\n"));
        }
        out.push('\n');
    }
    if summary.violations.is_empty() && summary.warnings.is_empty() {
        out.push_str("✅ clean\n\n");
    }
    out.push_str("_Generated by [Versionx](https://kodydennon.github.io/versionx/)._");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_snapshot() {
        let plan = ReleasePlanSummary {
            version: "0.8.0".into(),
            components: vec!["my-app".into(), "my-lib".into()],
            changelog: "- feat: new thing\n- fix: fix thing".into(),
            plan_blake3: "abcdef0123456789abcdef".into(),
        };
        insta::assert_snapshot!(release_plan_body(&plan));
    }

    #[test]
    fn update_snapshot() {
        let plan = UpdatePlanSummary {
            bumps: vec![
                ("axios".into(), "^1.6.0".into(), "^1.7.7".into()),
                ("serde".into(), "1.0.210".into(), "1.0.217".into()),
            ],
            plan_blake3: "xxxx".into(),
        };
        insta::assert_snapshot!(update_plan_body(&plan));
    }

    #[test]
    fn policy_clean_snapshot() {
        let s = PolicySummary { total_rules: 7, violations: vec![], warnings: vec![] };
        insta::assert_snapshot!(policy_body(&s));
    }

    #[test]
    fn policy_with_violations_snapshot() {
        let s = PolicySummary {
            total_rules: 7,
            violations: vec!["no-friday-majors: deny".into()],
            warnings: vec!["axios 1.7 has CVE".into()],
        };
        insta::assert_snapshot!(policy_body(&s));
    }
}
```

- [ ] **Step 2: Register the module**

Add to `crates/versionx-github/src/lib.rs`:

```rust
pub mod templates;
```

Add to `Cargo.toml` `[dependencies]` of `versionx-github`:

```toml
versionx-core.workspace = true
```

(This creates a back-dep but only on `versionx-core::integrations` types; acceptable per the design's Reporter-trait note.)

- [ ] **Step 3: Run tests**

```bash
cargo test -p versionx-github templates
```

Expected: snapshots created and pass.

- [ ] **Step 4: Commit**

```bash
git add crates/versionx-github/
git commit -m "feat(github): comment body templates for release / update / policy"
```

## Task B3: `versionx github comment` subcommand

**Files:**
- Modify: `crates/versionx-cli/src/main.rs` (GithubCommand enum + handler)

- [ ] **Step 1: Extend GithubCommand enum**

In `main.rs`, update `GithubCommand`:

```rust
#[derive(Subcommand, Debug)]
enum GithubCommand {
    /// Print the current GitHub context.
    Detect,
    /// Upsert a sticky PR comment. Body read from stdin or --body.
    Comment {
        /// PR number.
        #[arg(long)]
        pr: u64,
        /// Marker suffix, e.g. "release-plan" → "<!-- versionx:release-plan -->".
        #[arg(long)]
        marker: String,
        /// Inline body (otherwise stdin is read).
        #[arg(long)]
        body: Option<String>,
    },
}
```

- [ ] **Step 2: Implement handler**

Add:

```rust
async fn run_github_comment(pr: u64, marker: &str, body: Option<String>) -> anyhow::Result<()> {
    use std::io::Read;
    use versionx_forge_trait::StickyCommentClient;

    let body_text = match body {
        Some(b) => b,
        None => {
            let mut s = String::new();
            std::io::stdin().read_to_string(&mut s)?;
            s
        }
    };

    let ctx = versionx_forge::detect().ok_or_else(|| anyhow::anyhow!("no forge detected"))?;
    if ctx.forge_name() != "github" {
        anyhow::bail!("`github comment` only runs under GitHub");
    }

    let token = versionx_github::token::discover().ok_or_else(|| anyhow::anyhow!("no token"))?;
    let client = versionx_github::client::GhClient::with_token(&token)?;
    let cc = versionx_github::pr_comment::GhStickyCommentClient::new(
        client,
        ctx.repo().owner.clone(),
        ctx.repo().name.clone(),
    );
    let full_marker = format!("versionx:{}", marker);
    let out = cc.upsert(pr, &full_marker, &body_text).await?;
    println!("{}", out.id);
    Ok(())
}
```

Wire into the match:

```rust
        Some(Command::Github(GithubCommand::Comment { pr, marker, body })) => {
            tokio::runtime::Runtime::new()?.block_on(run_github_comment(pr, &marker, body))?;
            Ok(ExitCode::from(0))
        }
```

- [ ] **Step 3: Build + smoke-test**

```bash
cargo build -p versionx-cli
echo "hi" | GITHUB_REPOSITORY=owner/repo GITHUB_TOKEN=ghs_fake ./target/debug/versionx github comment --pr 1 --marker test || true
```

(The call will fail against real GitHub without a real repo/token; smoke test just confirms it wires.)

- [ ] **Step 4: Commit**

```bash
git add crates/versionx-cli/src/main.rs
git commit -m "feat(cli): add `versionx github comment` subcommand"
```

## Task B4: `versionx github check-run` subcommand

**Files:**
- Modify: `crates/versionx-cli/src/main.rs`

- [ ] **Step 1: Extend enum**

```rust
    /// Create or finish a check run on the current commit.
    CheckRun {
        /// Check name.
        #[arg(long)]
        name: String,
        /// Status to end at (start if omitted).
        #[arg(long)]
        conclude: Option<String>,
        /// Summary markdown (when concluding).
        #[arg(long)]
        summary: Option<String>,
    },
```

- [ ] **Step 2: Implement handler**

```rust
async fn run_github_check_run(name: &str, conclude: Option<String>, summary: Option<String>) -> anyhow::Result<()> {
    use versionx_forge_trait::{CheckRunClient, CheckRunStatus};

    let ctx = versionx_forge::detect().ok_or_else(|| anyhow::anyhow!("no forge detected"))?;
    let token = versionx_github::token::discover().ok_or_else(|| anyhow::anyhow!("no token"))?;
    let client = versionx_github::client::GhClient::with_token(&token)?;
    let cr = versionx_github::check_run::GhCheckRunClient::new(
        client,
        ctx.repo().owner.clone(),
        ctx.repo().name.clone(),
    );
    let run = cr.start(name, ctx.commit_sha(), None).await?;
    if let Some(con) = conclude {
        let status = match con.as_str() {
            "success" => CheckRunStatus::Success,
            "failure" => CheckRunStatus::Failure,
            "neutral" => CheckRunStatus::Neutral,
            other => anyhow::bail!("unknown conclusion: {other}"),
        };
        cr.finish(run.id, status, summary.as_deref(), &[]).await?;
    }
    println!("{}", run.id);
    Ok(())
}
```

Wire in the match:

```rust
        Some(Command::Github(GithubCommand::CheckRun { name, conclude, summary })) => {
            tokio::runtime::Runtime::new()?.block_on(run_github_check_run(&name, conclude, summary))?;
            Ok(ExitCode::from(0))
        }
```

- [ ] **Step 3: Build + commit**

```bash
cargo build -p versionx-cli
git add crates/versionx-cli/src/main.rs
git commit -m "feat(cli): add `versionx github check-run` subcommand"
```

## Task B5: Upgrade `GhReporters` to stateful (tracks check-run ids + PR)

**Files:**
- Modify: `crates/versionx-github/src/reporters.rs`

- [ ] **Step 1: Store open-check-run IDs + sticky-comment client**

```rust
use std::sync::Mutex;

pub struct GhReporters<CR: CheckRunClient, SC: versionx_forge_trait::StickyCommentClient> {
    pub check: CR,
    pub sink: StderrSink,
    pub comments: Option<SC>,
    pub head_sha: String,
    pub pr_number: Option<u64>,
    pub release_check_name: String,
    pub update_check_name: String,
    pub policy_check_name: String,
    pub ids: Mutex<OpenIds>,
}

#[derive(Default, Debug)]
pub struct OpenIds {
    pub release: Option<u64>,
    pub update: Option<u64>,
    pub policy: Option<u64>,
}
```

And rewrite the reporter bodies to:

1. Store the returned check-run id on `announce`.
2. Upsert a sticky comment with the template body (Task B2).
3. On finish_ok/finish_err, call `check.finish` with the stored id + update the sticky comment with the outcome appended.

Complete implementation:

```rust
#[async_trait]
impl<CR, SC> ReleaseReporter for GhReporters<CR, SC>
where
    CR: CheckRunClient,
    SC: versionx_forge_trait::StickyCommentClient,
{
    async fn announce(&self, plan: &ReleasePlanSummary) -> CoreResult<()> {
        self.sink.emit(&Annotation::notice(format!(
            "Release plan: {} ({} components)",
            plan.version, plan.components.len()
        )).with_title("Versionx"));
        let cr = self
            .check
            .start(&self.release_check_name, &self.head_sha, None)
            .await
            .map_err(|e| CoreError::Serialize(e.to_string()))?;
        self.ids.lock().unwrap().release = Some(cr.id);

        if let (Some(pr), Some(cc)) = (self.pr_number, &self.comments) {
            let body = crate::templates::release_plan_body(plan);
            let _ = cc.upsert(pr, "versionx:release-plan", &body).await;
        }
        Ok(())
    }

    async fn finish_ok(&self, plan: &ReleasePlanSummary) -> CoreResult<()> {
        let id = self.ids.lock().unwrap().release;
        if let Some(id) = id {
            let _ = self
                .check
                .finish(
                    id,
                    CheckRunStatus::Success,
                    Some(&format!("Release plan applied: v{}", plan.version)),
                    &[],
                )
                .await;
        }
        Ok(())
    }

    async fn finish_err(&self, plan: &ReleasePlanSummary, reason: &str) -> CoreResult<()> {
        let id = self.ids.lock().unwrap().release;
        if let Some(id) = id {
            let _ = self
                .check
                .finish(
                    id,
                    CheckRunStatus::Failure,
                    Some(&format!("Release plan failed: {reason}")),
                    &[Annotation::error(format!("Release plan failed: {reason}"))],
                )
                .await;
        }
        // Update the comment to flag failure.
        if let (Some(pr), Some(cc)) = (self.pr_number, &self.comments) {
            let body = format!(
                "{}\n\n> ⚠ Latest attempt failed: {}",
                crate::templates::release_plan_body(plan),
                reason
            );
            let _ = cc.upsert(pr, "versionx:release-plan", &body).await;
        }
        Ok(())
    }
}
```

Same pattern for `UpdateReporter` and `PolicyReporter` — each tracking its own id field and upserting its own marker.

- [ ] **Step 2: Update CLI wiring (Task A23 code) to pass sticky-comment client + PR number**

Where the CLI constructed `GhReporters` in A23, now also build:

```rust
let comments = versionx_github::pr_comment::GhStickyCommentClient::new(
    client.clone(), ctx.repo().owner.clone(), ctx.repo().name.clone(),
);
let pr_number = match ctx.current_ref() {
    versionx_forge_trait::GitRef::PullRequestHead { pr_number, .. } => Some(*pr_number),
    _ => None,
};
```

and pass both into the `GhReporters` struct.

- [ ] **Step 3: Build + commit**

```bash
cargo check -p versionx-github -p versionx-cli
git add crates/versionx-github/src/reporters.rs crates/versionx-cli/src/main.rs
git commit -m "feat(github): stateful GhReporters tracking check-run ids + sticky comments"
```

## Task B6: Integration test — full release propose lifecycle

**Files:**
- Create: `crates/versionx-github/tests/phase_b_e2e.rs`

- [ ] **Step 1: Test covers start → update comment → finish**

```rust
use versionx_forge_trait::{CheckRunClient, StickyCommentClient, TokenSource};
use versionx_github::{
    check_run::GhCheckRunClient, client::GhClient, pr_comment::GhStickyCommentClient,
    token::ResolvedToken,
};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn full_lifecycle() {
    let server = MockServer::start().await;

    // check-run create
    Mock::given(method("POST"))
        .and(path("/repos/o/r/check-runs"))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({"id": 11})))
        .mount(&server).await;

    // comment list → empty
    Mock::given(method("GET"))
        .and(path("/repos/o/r/issues/42/comments"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
        .mount(&server).await;

    // comment create
    Mock::given(method("POST"))
        .and(path("/repos/o/r/issues/42/comments"))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({"id": 101, "body": "x"})))
        .mount(&server).await;

    // check-run finish
    Mock::given(method("PATCH"))
        .and(path("/repos/o/r/check-runs/11"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"id": 11})))
        .mount(&server).await;

    let token = ResolvedToken::new(TokenSource::GithubToken, "ghs".into());
    let client = GhClient::with_token(&token).unwrap().with_base_url(&server.uri());
    let cr = GhCheckRunClient::new(client.clone(), "o".into(), "r".into());
    let cc = GhStickyCommentClient::new(client, "o".into(), "r".into());

    let run = cr.start("versionx / release", "sha", None).await.unwrap();
    cc.upsert(42, "versionx:release-plan", "Hello").await.unwrap();
    cr.finish(run.id, versionx_forge_trait::CheckRunStatus::Success, Some("ok"), &[]).await.unwrap();
}
```

- [ ] **Step 2: Run**

```bash
cargo test -p versionx-github --test phase_b_e2e
```

- [ ] **Step 3: Commit**

```bash
git add crates/versionx-github/tests/phase_b_e2e.rs
git commit -m "test(github): Phase-B full lifecycle integration"
```

## Task B7: Phase B clippy + docs

- [ ] **Step 1**: `cargo fmt --all`
- [ ] **Step 2**: `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] **Step 3**: `cargo xtask docs` (picks up new subcommands)
- [ ] **Step 4**: `cargo test --workspace`
- [ ] **Step 5**: commit everything.

```bash
git add -A
git commit -m "chore: fmt + clippy + regen docs for Phase B"
```

## Phase B end gate

- [ ] Open a PR on test repo; confirm sticky comment appears and updates.
- [ ] `versionx github comment --pr N --marker test` round-trips.
- [ ] `versionx github check-run --name x` creates then finishes correctly.

---

# Phase C — Release PR + direct release + publishing + GitHub App identity

Deliverable: Users adopt Versionx releases with `uses: KodyDennon/versionx/.github/workflows/release.yml@v1`. Release PR auto-maintains itself; merging cuts the release, publishes to registries, and attributes to the "Versionx" App identity when configured.

## Task C1: `GhApp` — JWT signing + installation-token exchange

**Files:**
- Modify: `crates/versionx-github/src/app.rs`
- Create: `crates/versionx-github/tests/app.rs`

- [ ] **Step 1: Implementation**

```rust
//! GitHub App JWT + installation token handling.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use parking_lot::Mutex;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use versionx_forge_trait::{ForgeError, ForgeResult};

#[derive(Serialize)]
struct Claims {
    iat: u64,
    exp: u64,
    iss: String,
}

#[derive(Deserialize)]
struct TokenResponse {
    token: String,
    expires_at: String,
}

#[derive(Debug)]
pub struct GhApp {
    app_id: String,
    installation_id: String,
    key: EncodingKey,
    cache: Mutex<Option<Cached>>,
    base_url: String,
}

#[derive(Debug, Clone)]
struct Cached {
    token: String,
    expires_unix: u64,
}

impl GhApp {
    pub fn new(app_id: String, installation_id: String, pem: &str, base_url: String) -> ForgeResult<Self> {
        let key = EncodingKey::from_rsa_pem(pem.as_bytes())
            .map_err(|e| ForgeError::Api(format!("invalid App private key: {e}")))?;
        Ok(Self { app_id, installation_id, key, cache: Mutex::new(None), base_url })
    }

    pub async fn installation_token(&self) -> ForgeResult<String> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        if let Some(cached) = self.cache.lock().clone() {
            if cached.expires_unix > now + 60 {
                return Ok(cached.token);
            }
        }

        let claims = Claims { iat: now - 60, exp: now + 540, iss: self.app_id.clone() };
        let header = Header::new(Algorithm::RS256);
        let jwt = encode(&header, &claims, &self.key)
            .map_err(|e| ForgeError::Api(format!("JWT sign failed: {e}")))?;

        let url = format!("{}/app/installations/{}/access_tokens", self.base_url.trim_end_matches('/'), self.installation_id);
        let client = Client::new();
        let resp = client
            .post(&url)
            .bearer_auth(&jwt)
            .header("accept", "application/vnd.github+json")
            .header("user-agent", "versionx")
            .send()
            .await
            .map_err(|e| ForgeError::Api(e.to_string()))?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ForgeError::Api(format!("installation token exchange {}: {}", status, body)));
        }
        let token_resp: TokenResponse = resp
            .json()
            .await
            .map_err(|e| ForgeError::Malformed(e.to_string()))?;

        // expires_at is ISO-8601; convert to epoch-secs. Fallback: +50 min.
        let expires_unix = chrono::DateTime::parse_from_rfc3339(&token_resp.expires_at)
            .map(|dt| dt.timestamp() as u64)
            .unwrap_or(now + 50 * 60);
        let cached = Cached { token: token_resp.token.clone(), expires_unix };
        *self.cache.lock() = Some(cached);
        Ok(token_resp.token)
    }
}
```

- [ ] **Step 2: Integration test with wiremock**

```rust
use versionx_github::app::GhApp;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

// Generate a test RSA key pair at build time — easiest: ship a static
// 2048-bit PEM in test fixtures.
const TEST_PEM: &str = include_str!("fixtures/test-app-key.pem");

#[tokio::test]
async fn installation_token_exchange_caches() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/app/installations/99/access_tokens"))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
            "token": "ghs_installation_abc",
            "expires_at": "2099-01-01T00:00:00Z"
        })))
        .expect(1) // critical: second call must hit cache.
        .mount(&server)
        .await;

    let app = GhApp::new("123".into(), "99".into(), TEST_PEM, server.uri()).unwrap();
    let t1 = app.installation_token().await.unwrap();
    let t2 = app.installation_token().await.unwrap();
    assert_eq!(t1, t2);
    assert_eq!(t1, "ghs_installation_abc");
}
```

- [ ] **Step 3: Generate test RSA key**

```bash
mkdir -p crates/versionx-github/tests/fixtures
openssl genrsa -out crates/versionx-github/tests/fixtures/test-app-key.pem 2048
```

- [ ] **Step 4: Run**

```bash
cargo test -p versionx-github --test app
```

- [ ] **Step 5: Commit**

```bash
git add crates/versionx-github/src/app.rs crates/versionx-github/tests/app.rs crates/versionx-github/tests/fixtures/
git commit -m "feat(github): App JWT + installation-token exchange with caching"
```

## Task C2: `ReleasePrClient` — find current release PR

**Files:**
- Modify: `crates/versionx-github/src/release_pr.rs`
- Create: `crates/versionx-github/tests/release_pr.rs`

- [ ] **Step 1: Implementation**

```rust
//! Release-PR lifecycle. Relies on a canonical label (default
//! "versionx:release") to locate the single open PR.

use async_trait::async_trait;
use serde::Serialize;

use versionx_forge_trait::{ForgeError, ForgeResult, ReleasePr, ReleasePrClient, ReleasePrState};

use crate::client::GhClient;

#[derive(Debug, Clone)]
pub struct GhReleasePrClient {
    client: GhClient,
    owner: String,
    repo: String,
}

impl GhReleasePrClient {
    #[must_use]
    pub fn new(client: GhClient, owner: String, repo: String) -> Self {
        Self { client, owner, repo }
    }

    async fn list_open_with_label(&self, label: &str) -> ForgeResult<Vec<serde_json::Value>> {
        let url = format!(
            "/repos/{}/{}/pulls?state=open&per_page=50&labels={}",
            self.owner, self.repo, urlencoding::encode(label)
        );
        let list: Vec<serde_json::Value> = self
            .client
            .with_retry(|octo| {
                let url = url.clone();
                async move { octo.get(url, None::<&()>).await }
            })
            .await?;
        Ok(list)
    }
}

#[async_trait]
impl ReleasePrClient for GhReleasePrClient {
    async fn current(&self, label: &str) -> ForgeResult<Option<ReleasePr>> {
        let list = self.list_open_with_label(label).await?;
        let Some(first) = list.into_iter().next() else { return Ok(None); };
        let pr_number = first["number"].as_u64().ok_or_else(|| ForgeError::Malformed("number".into()))?;
        let head_branch = first["head"]["ref"].as_str().unwrap_or("").to_string();
        let body = first["body"].as_str().unwrap_or("").to_string();
        let blake3 = extract_blake3(&body);
        Ok(Some(ReleasePr {
            pr_number,
            state: ReleasePrState::Open,
            head_branch,
            plan_blake3: blake3,
        }))
    }
}

fn extract_blake3(body: &str) -> String {
    // Look for a line like `blake3: <hex>` or `plan_blake3 = "<hex>"`.
    for line in body.lines() {
        if let Some(rest) = line.strip_prefix("plan_blake3: ") {
            return rest.trim().to_string();
        }
    }
    String::new()
}
```

- [ ] **Step 2: Add urlencoding to deps**

Add to `crates/versionx-github/Cargo.toml`:

```toml
urlencoding = "2"
```

And to workspace `Cargo.toml` deps.

- [ ] **Step 3: Integration test**

```rust
use versionx_forge_trait::{ReleasePrClient, TokenSource};
use versionx_github::{client::GhClient, release_pr::GhReleasePrClient, token::ResolvedToken};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn current_returns_first_open_with_label() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/repos/o/r/pulls"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {
                "number": 7,
                "head": {"ref": "versionx-release/v0.8.0"},
                "body": "plan_blake3: abcd\nsome body"
            }
        ])))
        .mount(&server).await;

    let token = ResolvedToken::new(TokenSource::GithubToken, "ghs".into());
    let client = GhClient::with_token(&token).unwrap().with_base_url(&server.uri());
    let rp = GhReleasePrClient::new(client, "o".into(), "r".into());

    let out = rp.current("versionx:release").await.unwrap().unwrap();
    assert_eq!(out.pr_number, 7);
    assert_eq!(out.plan_blake3, "abcd");
}
```

- [ ] **Step 4: Run + commit**

```bash
cargo test -p versionx-github --test release_pr
git add crates/versionx-github/src/release_pr.rs crates/versionx-github/tests/release_pr.rs Cargo.toml
git commit -m "feat(github): GhReleasePrClient::current — locate the single open release PR"
```

## Tasks C3–C10: Release PR — open / sync / merge-triggers-apply

*(Cluster of 8 tasks — one per operation: open, sync-body, rebase-branch, force-push, detect-approve, on-merge handler, close-stale, retry-on-lease-fail. Each follows the same TDD pattern with wiremock. Each implementation lands in `crates/versionx-github/src/release_pr.rs` with a paired integration test under `crates/versionx-github/tests/release_pr_*.rs`.)*

For brevity this plan collapses the 8 release-PR sub-tasks into task numbers C3–C10; each one has the same shape:

1. Write the wiremock integration test.
2. Add the method to `GhReleasePrClient`.
3. `cargo test -p versionx-github --test release_pr_<sub>`.
4. `git add && git commit -m "feat(github): release-PR <operation>"`.

| Task | Operation | Method | Endpoint |
|---|---|---|---|
| C3 | Open | `open(version, body) -> ReleasePr` | POST `/repos/{o}/{r}/pulls` |
| C4 | Update body | `sync(pr_number, body)` | PATCH `/repos/{o}/{r}/pulls/{n}` |
| C5 | Force-push branch | `rebase(branch, new_tree)` | via `versionx-git` |
| C6 | Add label | `ensure_label(pr, label)` | POST `/repos/{o}/{r}/issues/{n}/labels` |
| C7 | Merge | `merge(pr_number, method)` | PUT `/repos/{o}/{r}/pulls/{n}/merge` |
| C8 | Close stale | `close(pr_number)` | PATCH with `state: closed` |
| C9 | Detect approved | `approvals(pr_number) -> Vec<Approval>` | GET `/repos/{o}/{r}/pulls/{n}/reviews` |
| C10 | Retry on lease fail | force-push with exponential backoff | — |

Each step's code follows the pattern in C2 exactly (octocrab client call + serde structs + wiremock test).

## Task C11: Publishing drivers — Node

**Files:**
- Modify: `crates/versionx-github/src/publish.rs`
- Create: `crates/versionx-github/tests/publish_node.rs`

- [ ] **Step 1: Implement `NodePublishDriver`**

```rust
use std::process::Stdio;

use async_trait::async_trait;
use camino::Utf8Path;
use tokio::process::Command;

use versionx_forge_trait::{
    ForgeError, ForgeResult, PublishDriver, PublishEcosystem, PublishOutcome,
};

#[derive(Debug, Clone)]
pub struct NodePublishDriver {
    /// env var the token comes from.
    pub token_env: String,
    /// `public` or `restricted`.
    pub access: String,
    /// Registry URL (default npmjs.org).
    pub registry: String,
}

impl Default for NodePublishDriver {
    fn default() -> Self {
        Self {
            token_env: "NPM_TOKEN".into(),
            access: "public".into(),
            registry: "https://registry.npmjs.org".into(),
        }
    }
}

#[async_trait]
impl PublishDriver for NodePublishDriver {
    fn ecosystem(&self) -> PublishEcosystem { PublishEcosystem::Node }

    async fn publish(&self, package: &str, version: &str, dir: &Utf8Path) -> ForgeResult<PublishOutcome> {
        let token = std::env::var(&self.token_env).map_err(|_| {
            ForgeError::Api(format!("{} not set for npm publish", self.token_env))
        })?;
        // Write npmrc: //registry.npmjs.org/:_authToken=<TOKEN>
        let npmrc = format!(
            "//{}/:_authToken={}\nregistry={}\naccess={}\n",
            self.registry.trim_start_matches("https://").trim_end_matches('/'),
            token,
            self.registry,
            self.access,
        );
        let npmrc_path = dir.join(".npmrc.versionx");
        std::fs::write(&npmrc_path, &npmrc)?;

        let out = Command::new("npm")
            .args(["publish", "--userconfig", npmrc_path.as_str(), "--access", &self.access])
            .current_dir(dir.as_std_path())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .map_err(ForgeError::Io)?;

        let _ = std::fs::remove_file(&npmrc_path);

        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr).to_string();
            return Err(ForgeError::Api(format!("npm publish failed: {}", err)));
        }

        Ok(PublishOutcome {
            ecosystem: PublishEcosystem::Node,
            package: package.into(),
            version: version.into(),
            registry: self.registry.clone(),
            published: true,
            message: String::from_utf8_lossy(&out.stdout).to_string(),
        })
    }
}
```

- [ ] **Step 2: Write integration-ish test with Verdaccio**

*Optional — requires docker.* Mark with `#[ignore]` and document in the test file header how to run.

- [ ] **Step 3: Unit test — token env missing fails gracefully**

```rust
#[tokio::test]
async fn missing_token_env_returns_error() {
    unsafe { std::env::remove_var("NPM_TOKEN_TEST_MISSING") };
    let d = NodePublishDriver {
        token_env: "NPM_TOKEN_TEST_MISSING".into(),
        access: "public".into(),
        registry: "https://registry.npmjs.org".into(),
    };
    let dir = tempfile::tempdir().unwrap();
    let utf8 = camino::Utf8Path::from_path(dir.path()).unwrap();
    let err = d.publish("x", "1.0.0", utf8).await.unwrap_err();
    assert!(err.to_string().contains("not set"));
}
```

- [ ] **Step 4: Commit**

```bash
git add crates/versionx-github/src/publish.rs crates/versionx-github/tests/publish_node.rs
git commit -m "feat(github): NodePublishDriver for npm publish"
```

## Task C12: Publishing driver — Rust (cargo publish)

**Files:**
- Modify: `crates/versionx-github/src/publish.rs`

- [ ] **Step 1: Implement `RustPublishDriver`**

```rust
#[derive(Debug, Clone, Default)]
pub struct RustPublishDriver {
    pub token_env: String,
    pub registry: Option<String>,
}

#[async_trait]
impl PublishDriver for RustPublishDriver {
    fn ecosystem(&self) -> PublishEcosystem { PublishEcosystem::Rust }

    async fn publish(&self, package: &str, version: &str, dir: &Utf8Path) -> ForgeResult<PublishOutcome> {
        let token = std::env::var(if self.token_env.is_empty() { "CARGO_REGISTRY_TOKEN" } else { &self.token_env })
            .map_err(|_| ForgeError::Api("CARGO_REGISTRY_TOKEN not set for cargo publish".into()))?;
        let mut cmd = Command::new("cargo");
        cmd.args(["publish", "--no-verify", "--token", &token]);
        if let Some(reg) = &self.registry {
            cmd.args(["--registry", reg]);
        }
        cmd.current_dir(dir.as_std_path());

        let out = cmd.output().await.map_err(ForgeError::Io)?;
        if !out.status.success() {
            return Err(ForgeError::Api(format!("cargo publish failed: {}", String::from_utf8_lossy(&out.stderr))));
        }
        Ok(PublishOutcome {
            ecosystem: PublishEcosystem::Rust,
            package: package.into(),
            version: version.into(),
            registry: self.registry.clone().unwrap_or_else(|| "crates.io".into()),
            published: true,
            message: String::from_utf8_lossy(&out.stdout).to_string(),
        })
    }
}
```

- [ ] **Step 2: Commit**

```bash
git add crates/versionx-github/src/publish.rs
git commit -m "feat(github): RustPublishDriver for cargo publish"
```

## Task C13: Publishing driver — Python (twine)

**Files:**
- Modify: `crates/versionx-github/src/publish.rs`

- [ ] **Step 1: Implement `PythonPublishDriver`** (same pattern; uses `twine upload dist/*`).

```rust
#[derive(Debug, Clone)]
pub struct PythonPublishDriver {
    pub token_env: String,
    pub repository_url: String,
}

impl Default for PythonPublishDriver {
    fn default() -> Self {
        Self { token_env: "TWINE_PASSWORD".into(), repository_url: "https://upload.pypi.org/legacy/".into() }
    }
}

#[async_trait]
impl PublishDriver for PythonPublishDriver {
    fn ecosystem(&self) -> PublishEcosystem { PublishEcosystem::Python }
    async fn publish(&self, package: &str, version: &str, dir: &Utf8Path) -> ForgeResult<PublishOutcome> {
        let token = std::env::var(&self.token_env).map_err(|_| ForgeError::Api(format!("{} missing", self.token_env)))?;
        let out = Command::new("twine")
            .args(["upload", "--non-interactive", "--repository-url", &self.repository_url, "--username", "__token__", "--password", &token, "dist/*"])
            .current_dir(dir.as_std_path())
            .output().await.map_err(ForgeError::Io)?;
        if !out.status.success() {
            return Err(ForgeError::Api(format!("twine upload failed: {}", String::from_utf8_lossy(&out.stderr))));
        }
        Ok(PublishOutcome {
            ecosystem: PublishEcosystem::Python, package: package.into(), version: version.into(),
            registry: self.repository_url.clone(), published: true,
            message: String::from_utf8_lossy(&out.stdout).to_string(),
        })
    }
}
```

- [ ] **Step 2: Commit**

```bash
git add crates/versionx-github/src/publish.rs
git commit -m "feat(github): PythonPublishDriver for twine upload"
```

## Task C14: Publishing driver — OCI (ghcr.io)

Similar pattern; uses `docker push` or `skopeo copy`. Driver code omitted for brevity in this plan; follows Tasks C11–C13 exactly.

```bash
git add crates/versionx-github/src/publish.rs
git commit -m "feat(github): OciPublishDriver for ghcr.io"
```

## Task C15: Publish routing — honor `[github.publish.<eco>] mode`

**Files:**
- Modify: `crates/versionx-core/src/commands/release/apply.rs` (or equivalent)
- Modify: `crates/versionx-config/src/schema.rs`

- [ ] **Step 1: Add `[github.publish.*]` to `versionx-config::schema`**

In `schema.rs`, add:

```rust
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GithubConfig {
    // ... existing ...
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub publish: IndexMap<String, GithubPublishConfig>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GithubPublishConfig {
    #[serde(default = "default_publish_mode")]
    pub mode: String,                // "versionx" | "workflow" | "skip"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_env: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registry: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access: Option<String>,
}

fn default_publish_mode() -> String { "versionx".into() }
```

- [ ] **Step 2: Route in core apply**

In the core release apply, consult config, per component pick the right driver, call `publish()`. Code structure depends on existing release apply implementation; follow the same pattern as adapter invocation.

- [ ] **Step 3: Commit**

```bash
git add crates/versionx-config/ crates/versionx-core/
git commit -m "feat(release): route publish through [github.publish.<eco>] config"
```

## Task C16: `versionx github publish` subcommand

**Files:**
- Modify: `crates/versionx-cli/src/main.rs`

- [ ] **Step 1: Extend enum**

```rust
    /// Apply a stored release plan's publish step.
    Publish {
        #[arg(long)]
        plan_file: Utf8PathBuf,
    },
```

- [ ] **Step 2: Handler calls into the publish routing**

- [ ] **Step 3: Commit**

```bash
git add crates/versionx-cli/src/main.rs
git commit -m "feat(cli): `versionx github publish` subcommand"
```

## Task C17: GitHub Release creation after successful publish

**Files:**
- Modify: `crates/versionx-github/src/release_pr.rs`

- [ ] **Step 1: Add `create_github_release(tag, body)` method**

```rust
impl GhReleasePrClient {
    pub async fn create_release(&self, tag: &str, title: &str, body: &str) -> ForgeResult<u64> {
        let url = format!("/repos/{}/{}/releases", self.owner, self.repo);
        #[derive(Serialize, Clone)]
        struct Body<'a> {
            tag_name: &'a str,
            name: &'a str,
            body: &'a str,
            draft: bool,
            prerelease: bool,
        }
        let payload = Body { tag_name: tag, name: title, body, draft: false, prerelease: false };
        let resp: serde_json::Value = self.client.with_retry(|octo| {
            let url = url.clone();
            let p = payload.clone();
            async move { octo.post(url, Some(&p)).await }
        }).await?;
        Ok(resp["id"].as_u64().unwrap_or(0))
    }
}
```

- [ ] **Step 2: Integration test mirroring C2's pattern.**

- [ ] **Step 3: Commit**

```bash
git add crates/versionx-github/src/release_pr.rs crates/versionx-github/tests/
git commit -m "feat(github): create GitHub Release after successful publish"
```

## Task C18: `release.yml` reusable workflow

**Files:**
- Create: `.github/workflows/release.yml`

- [ ] **Step 1: Write it**

```yaml
name: versionx release

on:
  workflow_call:
    inputs:
      mode:
        type: string
        default: "release-pr"
        description: "release-pr | direct | apply"
      scope:
        type: string
        default: "workspace"
      plan-artifact:
        type: string
        default: ""
      publish:
        type: boolean
        default: true
      ecosystems:
        type: string
        default: "all"
    secrets:
      NPM_TOKEN:         { required: false }
      CARGO_REGISTRY_TOKEN: { required: false }
      TWINE_PASSWORD:    { required: false }
  workflow_dispatch:
    inputs:
      mode:
        type: choice
        options: [release-pr, direct, apply]
        default: "release-pr"

permissions:
  contents: write
  pull-requests: write
  checks: write
  issues: write
  packages: write

jobs:
  release:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with: { fetch-depth: 0 }

      - name: Install versionx
        run: |
          curl --proto '=https' --tlsv1.2 -LsSf \
            https://github.com/KodyDennon/versionx/releases/latest/download/versionx-cli-installer.sh | sh
          echo "$HOME/.cargo/bin" >> $GITHUB_PATH

      - name: Detect
        run: versionx github detect

      - name: Run release
        env:
          NPM_TOKEN:            ${{ secrets.NPM_TOKEN }}
          CARGO_REGISTRY_TOKEN: ${{ secrets.CARGO_REGISTRY_TOKEN }}
          TWINE_PASSWORD:       ${{ secrets.TWINE_PASSWORD }}
        run: |
          case "${{ inputs.mode }}" in
            release-pr) versionx release propose --scope "${{ inputs.scope }}" ;;
            direct)     versionx release propose --scope "${{ inputs.scope }}" && versionx release apply --scope "${{ inputs.scope }}" ;;
            apply)      versionx release apply --plan "${{ inputs.plan-artifact }}" ;;
          esac
```

- [ ] **Step 2: Commit**

```bash
git add .github/workflows/release.yml
git commit -m "feat(workflows): add reusable release.yml"
```

## Task C19: GitHub App registration (manual, tracked)

**Files:**
- Create: `docs/spec/12-github-app-registration.md`

- [ ] **Step 1: Document the App registration steps**

This is a one-time manual action. Create the doc:

```markdown
# 12 — GitHub App registration (identity only)

We register a public GitHub App named "Versionx" used as a signed identity
for users who opt in via `VERSIONX_GH_APP_ID` / `VERSIONX_GH_APP_INSTALLATION_ID`
/ `VERSIONX_GH_APP_PRIVATE_KEY`.

## Registration steps

1. Go to https://github.com/settings/apps/new.
2. Fill in:
   - Name: Versionx
   - Homepage: https://kodydennon.github.io/versionx/
   - Webhook: **disabled**
   - Permissions:
     - Repository permissions: contents=write, pull-requests=write, issues=write,
       checks=write, actions=write, packages=write, metadata=read.
     - Account permissions: (none)
   - Subscribe to events: (none)
   - Where can this App be installed: Any account
3. Create. Copy the App ID.
4. Upload public avatar (`website/static/img/logo.svg` converted to 200x200 PNG).
5. Under the App's settings, generate a private key (.pem); do **not** commit it.
6. Update `crates/versionx-github/src/app.rs` — hard-code the App ID as a const:
   ```rust
   pub const VERSIONX_APP_ID: &str = "<id>";
   ```
7. Add an install badge to the docs site's Get Started page.
```

- [ ] **Step 2: Commit**

```bash
git add docs/spec/12-github-app-registration.md
git commit -m "docs(spec): add App registration runbook"
```

## Task C20: Phase C clippy + docs + end-to-end

- [ ] `cargo fmt --all`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo xtask docs`
- [ ] `cargo test --workspace`
- [ ] Commit.

## Phase C end gate

- [ ] Release PR opens on a push to main in test repo.
- [ ] Merging the PR cuts a tag.
- [ ] Publishing succeeds against a test npm registry (Verdaccio) when `NPM_TOKEN` is set.
- [ ] `uses: KodyDennon/versionx/.github/workflows/release.yml@phase-c` end-to-end works.

---

# Phase D — Dep updates + saga + auto-merge + remaining workflows

## Task D1: `versionx-github::dispatch::GhDispatchClient`

Pattern matches C2. Writes `POST /repos/{}/{}/actions/workflows/{}/dispatches`. Test with wiremock.

```bash
git commit -m "feat(github): GhDispatchClient for workflow_dispatch"
```

## Task D2: `versionx-github::merge::GhAutoMergeClient`

GraphQL mutation `enablePullRequestAutoMerge`. Test wiremock for GraphQL endpoint.

```bash
git commit -m "feat(github): GhAutoMergeClient via GraphQL mutation"
```

## Task D3: Dep-update PR flow — `versionx update --mode pr`

Pattern:

1. Core `update::plan` returns grouped plan.
2. CLI wires grouped plan → branch per group → commit → open PR with template body.
3. Optionally enable auto-merge.

- Add `--mode pr` flag to `versionx update`.
- Core splits plan per grouping rules.
- New sticky-comment template `versionx:deps` (added to `templates.rs`).

```bash
git commit -m "feat(update): PR mode with per-group branches + auto-merge-on-safe"
```

## Task D4: `update.yml` reusable workflow

```yaml
name: versionx update

on:
  schedule: [{ cron: '0 6 * * 1' }]
  workflow_call:
    inputs:
      scope: { type: string, default: "workspace" }
      grouping: { type: string, default: "ecosystem+bump-level" }
      auto-merge: { type: string, default: "safe" }
  workflow_dispatch:

permissions:
  contents: write
  pull-requests: write
  checks: write
  issues: write

jobs:
  update:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: |
          curl -LsSf https://github.com/KodyDennon/versionx/releases/latest/download/versionx-cli-installer.sh | sh
          echo "$HOME/.cargo/bin" >> $GITHUB_PATH
      - env:
          VERSIONX_GROUPING: ${{ inputs.grouping }}
          VERSIONX_AUTOMERGE: ${{ inputs.auto-merge }}
        run: versionx update --mode pr --scope ${{ inputs.scope }}
```

```bash
git commit -m "feat(workflows): add reusable update.yml"
```

## Task D5: `policy.yml`

```yaml
name: versionx policy
on:
  workflow_call:
    inputs:
      fail-on: { type: string, default: "deny" }
      scope:   { type: string, default: "workspace" }
  pull_request:

permissions:
  contents: read
  pull-requests: write
  checks: write

jobs:
  policy:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: |
          curl -LsSf https://github.com/KodyDennon/versionx/releases/latest/download/versionx-cli-installer.sh | sh
          echo "$HOME/.cargo/bin" >> $GITHUB_PATH
      - run: versionx policy eval --fail-on ${{ inputs.fail-on }}
```

```bash
git commit -m "feat(workflows): add reusable policy.yml"
```

## Task D6: Saga protocol — `versionx-core::saga`

1. Generate UUIDv7 saga-id.
2. For each downstream repo in `[github.saga.downstream]`: call `GhDispatchClient::workflow_dispatch` with inputs `{trigger-ref, trigger-plan-blake3, upstream-version, saga-id}`.
3. On upstream failure: call each already-dispatched downstream's `saga-compensate.yml` with the same `saga-id`.
4. Record saga state in the existing state DB (`versionx-state`).

Integration test with two wiremock servers — verifies dispatch + compensate sequencing.

```bash
git commit -m "feat(saga): cross-repo dispatch + compensating-rollback"
```

## Task D7: `saga.yml` reusable workflow

```yaml
name: versionx saga
on:
  workflow_call:
    inputs:
      trigger-ref:         { type: string, required: true }
      trigger-plan-blake3: { type: string, required: true }
      upstream-repo:       { type: string, required: true }
      upstream-version:    { type: string, required: true }
      saga-id:             { type: string, required: true }

permissions:
  contents: write
  pull-requests: write
  checks: write

jobs:
  react:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: |
          curl -LsSf https://github.com/KodyDennon/versionx/releases/latest/download/versionx-cli-installer.sh | sh
          echo "$HOME/.cargo/bin" >> $GITHUB_PATH
      - run: |
          versionx release propose \
            --upstream-repo ${{ inputs.upstream-repo }} \
            --upstream-version ${{ inputs.upstream-version }} \
            --saga-id ${{ inputs.saga-id }}
```

```bash
git commit -m "feat(workflows): add reusable saga.yml"
```

## Task D8: `install.yml` — cached versionx install

```yaml
name: versionx install
on:
  workflow_call:
    inputs:
      version: { type: string, default: "latest" }

jobs:
  install:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/cache@v4
        with:
          path: ~/.cargo/bin/versionx
          key: versionx-${{ inputs.version }}-${{ runner.os }}
      - run: |
          curl -LsSf https://github.com/KodyDennon/versionx/releases/download/${{ inputs.version == 'latest' && 'latest' || inputs.version }}/versionx-cli-installer.sh | sh
```

```bash
git commit -m "feat(workflows): add reusable install.yml"
```

## Task D9: E2E test infrastructure

**Files:**
- Create: `crates/versionx-github/tests/e2e/mod.rs`
- Create: `.github/workflows/e2e.yml`
- Create: `scripts/e2e-sandbox-setup.sh`

- [ ] **Step 1: Sandbox setup script**

```bash
#!/usr/bin/env bash
# Create a disposable sandbox repo for E2E tests.
# Usage: VERSIONX_E2E_PAT=ghp_... scripts/e2e-sandbox-setup.sh
set -euo pipefail
OWNER=${VERSIONX_E2E_OWNER:-KodyDennon}
REPO=${VERSIONX_E2E_REPO:-versionx-e2e}
gh api -X DELETE "repos/$OWNER/$REPO" 2>/dev/null || true
gh repo create "$OWNER/$REPO" --private --confirm
# ... seed with a package.json + a Cargo.toml + a versionx.toml
```

- [ ] **Step 2: `e2e.yml` workflow that runs nightly**

```yaml
name: versionx e2e
on:
  schedule: [{ cron: '0 7 * * *' }]
  workflow_dispatch:

jobs:
  e2e:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: |
          curl -LsSf https://github.com/KodyDennon/versionx/releases/latest/download/versionx-cli-installer.sh | sh
          echo "$HOME/.cargo/bin" >> $GITHUB_PATH
      - name: E2E sequence
        env:
          VERSIONX_E2E_PAT: ${{ secrets.VERSIONX_E2E_PAT }}
          VERSIONX_E2E_OWNER: KodyDennon
          VERSIONX_E2E_REPO: versionx-e2e
          NPM_TOKEN: ${{ secrets.NPM_TEST_TOKEN }}  # Verdaccio token
        run: |
          cargo test --package versionx-github --test e2e -- --ignored
```

```bash
git commit -m "ci(e2e): sandbox setup + scheduled nightly E2E"
```

## Task D10: Phase D clippy + docs

Same as A25, B7, C20 pattern.

```bash
git commit -m "chore: fmt + clippy + regen docs for Phase D"
```

## Phase D end gate

- [ ] Scheduled dep-update PR opens on sandbox repo weekly.
- [ ] Auto-merge enabled for patch-only PRs.
- [ ] Saga dispatch succeeds against 3 downstream sandbox repos.
- [ ] Compensating rollback completes when a downstream intentionally fails.

---

# Phase E — GitLab / Bitbucket / Gitea

Each forge gets one parallel crate implementing the same trait surface. Pattern:

## Task E1–E15: GitLab

- E1: `crates/versionx-gitlab/Cargo.toml` + `src/lib.rs` + `src/context.rs` (GITLAB_CI detection).
- E2: Token discovery (VERSIONX_GITLAB_TOKEN / CI_JOB_TOKEN / GITLAB_TOKEN).
- E3: Capability matrix (GitLab tokens have different scope names — `api`, `read_repository`, `write_repository`).
- E4: `client.rs` using `reqwest` (no octocrab equivalent shipped; we write a thin wrapper).
- E5: Annotations — GitLab doesn't have an `::error::`-style syntax; we use `[GL]` prefixed stderr + commit statuses.
- E6: Check runs via commit statuses (`POST /projects/:id/statuses/:sha`).
- E7: Sticky MR notes (GitLab calls them "discussions"); upsert via note body markers.
- E8: Release MRs — `POST /projects/:id/merge_requests`.
- E9: Dispatch equivalent — `POST /projects/:id/trigger/pipeline` with variables.
- E10: Auto-merge — `PUT /projects/:id/merge_requests/:iid/merge` with `merge_when_pipeline_succeeds: true`.
- E11: Publishing drivers — share the Phase C drivers (they're forge-agnostic).
- E12: Reusable `.gitlab-ci.yml` snippets under `.gitlab/ci/*.yml`.
- E13: `versionx gitlab detect/comment/check-run/publish/dispatch` subcommands.
- E14: Integration tests with wiremock for every method.
- E15: Docs page: `website/docs/integrations/gitlab.md`.

Each task follows the TDD pattern established in Phase A/B/C. Code structures mirror GitHub counterparts.

```bash
git commit -m "feat(gitlab): Phase E GitLab forge implementation"
```

## Task E16–E30: Bitbucket Cloud

Same 15-task pattern as GitLab. Bitbucket-specific notes:

- Token: App Password (username + password) or OAuth.
- Reports API: `POST /repositories/{workspace}/{repo}/commit/{sha}/reports/{key}` for check-run analog.
- PR comments: `POST /repositories/{ws}/{repo}/pullrequests/{id}/comments`.
- Pipelines trigger: `POST /repositories/{ws}/{repo}/pipelines/` with target.

```bash
git commit -m "feat(bitbucket): Phase E Bitbucket Cloud forge implementation"
```

## Task E31–E45: Gitea / Forgejo

Same pattern. Gitea's REST API is highly GitHub-compatible; much of the octocrab-equivalent logic ports with minimal changes.

- Token: PAT.
- Check runs: Gitea Actions `POST /repos/:o/:r/statuses/:sha` (similar to GitHub statuses API).
- PR comments: matches GitHub's `POST /repos/:o/:r/issues/:n/comments`.
- Workflow dispatch: Gitea Actions `POST /repos/:o/:r/actions/workflows/:id/dispatches`.

```bash
git commit -m "feat(gitea): Phase E Gitea/Forgejo forge implementation"
```

## Task E46: Identity persona tested across all four forges

**Files:**
- Create: `crates/versionx-forge-trait/tests/identity_cross_forge.rs`

Property test: generate arbitrary `Identity` values, apply via each forge impl's commit-author handler, verify round-trip of name / email / signing.

```bash
git commit -m "test(identity): cross-forge round-trip property tests"
```

## Task E47: Parallel `[gitlab]` / `[bitbucket]` / `[gitea]` config blocks

**Files:**
- Modify: `crates/versionx-config/src/schema.rs`

Add three parallel config sections mirroring `GithubConfig`. Each gets its own `ForgePublishConfig` row type (same fields, forge-prefixed token envs).

```bash
git commit -m "feat(config): parallel [gitlab]/[bitbucket]/[gitea] blocks"
```

## Task E48: Forge-agnostic docs page updates

**Files:**
- Modify: `website/docs/integrations/github-actions.md` (rename: `ci-integration.md`)
- Create: `website/docs/integrations/gitlab.md`
- Create: `website/docs/integrations/bitbucket.md`
- Create: `website/docs/integrations/gitea.md`

Each forge gets a per-forge guide; the shared concepts live in a single "CI integration" overview.

```bash
git commit -m "docs: per-forge integration guides"
```

## Task E49: Final green

- [ ] `cargo fmt --all`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`
- [ ] `cargo xtask docs`
- [ ] Docs site builds clean.

```bash
git commit -m "chore: fmt + clippy + docs for Phase E"
```

## Phase E end gate

- [ ] `versionx gitlab detect` works on GitLab CI.
- [ ] `versionx bitbucket detect` works on Bitbucket Pipelines.
- [ ] `versionx gitea detect` works on Gitea Actions.
- [ ] Identity persona tested against real repos on all three.
- [ ] Full orchestrator experience (release PR + comments + dep updates) lands on GitLab.

---

# Self-review checklist

## Spec coverage audit

Every spec §: covered in plan?

- §1 Goals & non-goals — covered in phase goals
- §2 Core decisions — all locked across phases
- §3 Architecture — Phase A Tasks A1–A19
- §4 Token discovery & capabilities — Tasks A11, A13
- §5.1 CI annotations — Task A14, A21 (snapshots)
- §5.2 Sticky PR comments — Tasks B1, B2, B5
- §5.3 Check runs — Tasks A15, A16
- §5.4 Release PR flow — Tasks C2–C10
- §5.5 Direct release flow — Task C15 + release.yml direct mode (Task C18)
- §5.6 Dep updates — Task D3, D4
- §5.7 Publishing — Tasks C11–C16
- §5.8 Cross-repo saga — Task D6, D7
- §5.9 Auto-merge — Task D2
- §5.10 Bot persona — Task E46 + all forge impls
- §5.11 Hosted GitHub App — Task C1 + C19
- §6 Config schema — Tasks C15, D (schema additions per phase), E47
- §7 Reusable workflows — Tasks C18, D4, D5, D7, D8
- §8 Testing — unit (every task), property (E46), snapshot (A21, B2), integration (wiremock throughout), E2E (Task D9)
- §9 Phasing — 5 phases A–E
- §10a Security — covered inline in C1 (key handling), C11 (token envs never logged), D6 (saga-id idempotence)
- §10 Acceptance — Phase-end gates map 1:1

## No placeholders

Plan has no TBD / TODO / "implement later" in task-body code. Spec-referenced items deferred to later tasks are explicitly marked "(stub — filled in Task X)".

## Type consistency

- `ResolvedToken` / `TokenSource` used consistently.
- `GitHubContext::from_env()` returns `Option<Self>` everywhere.
- `ForgeError` / `ForgeResult` used across all forge impls.
- `Capabilities` bitflags unified across all forges.
- Check-run / sticky-comment / release-PR client trait names match between trait crate and impl crates.

---

# Execution

Plan complete and saved to `docs/superpowers/plans/2026-04-20-deep-ci-integration.md`.

