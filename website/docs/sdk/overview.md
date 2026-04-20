---
title: SDK overview
description: The Rust SDK lets you embed Versionx in your own application. When to embed vs shell out, and what the surface looks like.
sidebar_position: 1
---

# SDK overview

`versionx-sdk` is the public Rust crate that re-exports the stable subset of `versionx-core`. Use it when you want Versionx's capabilities inside your own program without shelling out to the `versionx` binary.

## When to embed vs shell out

**Embed the SDK when:**

- You're building another Rust tool that already has an event loop (a build system, a CI runner, a custom orchestrator).
- You want to subscribe to the event bus programmatically.
- You need fine-grained control over which subsystems are active.

**Shell out to the binary when:**

- You're in any non-Rust language.
- You want to decouple your tool's release cycle from Versionx's.
- You want each invocation to be a clean process (CI is usually this).

Both paths produce identical results — the CLI is itself a `versionx-core` frontend. There's no "more powerful API" under the hood.

## Installation

```toml
# Cargo.toml
[dependencies]
versionx-sdk = "0.7"
```

The SDK is published on crates.io. It re-exports from `versionx-core`, and its major version tracks Versionx's major version.

## Surface at a glance

```rust
use versionx_sdk::{Core, CoreConfig, EventBus};
use versionx_sdk::commands::{SyncOptions, UpdateOptions, ReleaseOptions};
```

- `Core` — the main handle.
- `CoreConfig` — construction-time knobs.
- `EventBus` — the subscribable event stream.
- `commands::*` — the verb API mirroring the CLI.

## Minimal example

```rust
use versionx_sdk::{Core, commands::SyncOptions};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let core = Core::discover(std::env::current_dir()?).await?;
    let report = core.sync(SyncOptions::default()).await?;
    println!("Synced {} ecosystems", report.ecosystems.len());
    Ok(())
}
```

## docs.rs

Full API reference is on [docs.rs/versionx-sdk](https://docs.rs/versionx-sdk). Every `pub` item has a doc comment; pedantic clippy enforces that in CI.

## See also

- [Embedding versionx-core](./embedding) — the minimum `Cargo.toml` and the first few lines.
- [Plan / apply cookbook](./plan-apply-cookbook) — how to use the safety contract from Rust.
- [Custom adapters](./custom-adapters) — implement `PackageManagerAdapter` to add a new ecosystem.
