---
title: Embedding versionx-core
description: Minimum Cargo.toml, constructing a Core, subscribing to events, and shutting down cleanly.
sidebar_position: 2
---

# Embedding `versionx-core`

You'll learn:

- The minimum `Cargo.toml` for embedding.
- How to construct a `Core` and run one command.
- How to subscribe to the event bus.
- How to shut down cleanly when your program is long-lived.

## Minimum Cargo.toml

```toml
[package]
name = "my-tool"
version = "0.1.0"
edition = "2024"

[dependencies]
versionx-sdk = "0.7"
tokio = { version = "1.41", features = ["full"] }
anyhow = "1"
```

## Construct a Core

```rust
use versionx_sdk::{Core, CoreConfig};
use camino::Utf8PathBuf;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cwd = Utf8PathBuf::from_path_buf(std::env::current_dir()?)
        .map_err(|p| anyhow::anyhow!("non-utf8 path: {p:?}"))?;

    let core = Core::builder()
        .cwd(cwd)
        .use_daemon(false)            // run in-process
        .build()
        .await?;

    // ...
    Ok(())
}
```

`Core::discover(cwd)` is the shorthand when you don't need custom config.

## Run a command

```rust
use versionx_sdk::commands::{StatusOptions, UpdateOptions};

let status = core.status(StatusOptions::default()).await?;
println!("{} ecosystems detected", status.ecosystems.len());

let plan = core.update(UpdateOptions::plan_only()).await?;
for bump in plan.bumps {
    println!("{} {} -> {}", bump.package, bump.current, bump.next);
}
```

Every command returns a structured report. No stdout parsing.

## Subscribe to events

```rust
use versionx_sdk::{EventBus, Event};

let bus = EventBus::new();
let mut rx = bus.subscribe();

let core = Core::builder().cwd(cwd).event_bus(bus.clone()).build().await?;

tokio::spawn(async move {
    while let Ok(event) = rx.recv().await {
        match event.kind.as_str() {
            k if k.starts_with("adapter.") => eprintln!("{event:?}"),
            _ => {}
        }
    }
});

core.sync(Default::default()).await?;
```

Event shape and taxonomy match [Events & tracing](/reference/events).

## Apply a plan

```rust
let plan = core.update(UpdateOptions::plan_only()).await?;

// Serialize, show the user, ask for approval...

let outcome = core.apply(plan).await?;
println!("{outcome:?}");
```

Prerequisites are re-verified inside `apply`. If HEAD moved or the lockfile changed, `apply` returns an error instead of mutating state.

## Shutdown

```rust
core.shutdown().await?;
```

Drops the state DB connection, flushes the event bus, releases the daemon connection (if any). Optional but recommended for long-lived programs.

## Common pitfalls

- **`Core` handles are not `Clone`.** Wrap in `Arc` if you need to share across tasks.
- **Blocking the runtime.** Every command is `async`. Use `tokio::spawn_blocking` for anything you want to run off the scheduler.
- **State DB contention.** If your program runs Versionx concurrently in the same `VERSIONX_DATA_HOME`, wrap writes in a single-writer actor. The CLI does this automatically; the SDK leaves it to you.

## See also

- [SDK overview](./overview) — when to embed vs shell out.
- [Plan / apply cookbook](./plan-apply-cookbook) — the safety contract in Rust.
- [Custom adapters](./custom-adapters) — if you're adding an ecosystem Versionx doesn't know about.
