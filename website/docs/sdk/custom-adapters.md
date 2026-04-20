---
title: Custom adapters
description: Implement the PackageManagerAdapter trait to teach Versionx about a new ecosystem.
sidebar_position: 4
---

# Custom adapters

A `PackageManagerAdapter` teaches Versionx about a new ecosystem. The trait is small and lives in the `versionx-adapter-trait` crate. Adapters **never** depend on `versionx-core`; the core learns about them via an internal registry.

If you're adding a first-party adapter, see [Contributing → Adding a package-manager adapter](/contributing/adding-a-package-manager-adapter) for the project-side conventions. This page is for out-of-tree adapters or for understanding the trait in the abstract.

## The trait (abridged)

```rust
#[async_trait::async_trait]
pub trait PackageManagerAdapter: Send + Sync {
    /// A stable identifier (e.g., "npm", "pip", "cargo", "bundler").
    fn id(&self) -> &'static str;

    /// Heuristics to decide if this adapter applies to a given directory.
    async fn detect(&self, cwd: &Utf8Path) -> Result<Detection>;

    /// Read the manifest(s) and report installed / requested versions.
    async fn status(&self, cwd: &Utf8Path) -> Result<StatusReport>;

    /// Plan dependency updates; do not mutate anything.
    async fn plan_update(&self, cwd: &Utf8Path, opts: &UpdateOptions) -> Result<UpdatePlan>;

    /// Apply a plan previously produced by this adapter.
    async fn apply(&self, cwd: &Utf8Path, plan: &UpdatePlan) -> Result<UpdateOutcome>;

    /// Run the ecosystem's native audit and return findings.
    async fn audit(&self, cwd: &Utf8Path) -> Result<AuditReport>;
}
```

Tier guidance:

- **Tier 1** (Node, Python, Rust): full adapter + matching runtime installer.
- **Tier 2** (Go, Ruby, JVM): full adapter; runtime lands in a later milestone.
- **Tier 3** (experimental / niche): adapter only; user is expected to install the runtime themselves.

## A thin example

```rust
use versionx_adapter_trait::{PackageManagerAdapter, Detection, StatusReport, UpdatePlan, UpdateOutcome, UpdateOptions, AuditReport, Result};
use camino::Utf8Path;

pub struct ExampleAdapter;

#[async_trait::async_trait]
impl PackageManagerAdapter for ExampleAdapter {
    fn id(&self) -> &'static str { "example" }

    async fn detect(&self, cwd: &Utf8Path) -> Result<Detection> {
        Ok(Detection::yes_if_exists(cwd, "example.toml"))
    }

    async fn status(&self, cwd: &Utf8Path) -> Result<StatusReport> {
        // Parse example.toml, compare installed vs requested...
        Ok(StatusReport::empty())
    }

    async fn plan_update(&self, _: &Utf8Path, _: &UpdateOptions) -> Result<UpdatePlan> {
        Ok(UpdatePlan::nothing_to_do())
    }

    async fn apply(&self, _: &Utf8Path, _: &UpdatePlan) -> Result<UpdateOutcome> {
        Ok(UpdateOutcome::nothing_to_do())
    }

    async fn audit(&self, _: &Utf8Path) -> Result<AuditReport> {
        Ok(AuditReport::empty())
    }
}
```

## Registering out-of-tree

In-tree adapters are wired via the core's static registry. Out-of-tree adapters can register at runtime via the SDK:

```rust
let core = versionx_sdk::Core::builder()
    .with_adapter(Arc::new(ExampleAdapter))
    .cwd(cwd)
    .build()
    .await?;
```

## Testing

The `versionx-adapter-trait` crate ships a test kit:

```rust
use versionx_adapter_trait::testkit::AdapterTestHarness;

#[tokio::test]
async fn detects_example_toml() {
    let harness = AdapterTestHarness::new().with_file("example.toml", "…").build();
    let adapter = ExampleAdapter;
    assert!(adapter.detect(harness.path()).await.unwrap().is_yes());
}
```

Property tests, snapshot fixtures, and a "round-trip a plan" harness are included.

## See also

- [Adding a package-manager adapter](/contributing/adding-a-package-manager-adapter) — first-party convention and CI gates.
- [SDK overview](./overview)
