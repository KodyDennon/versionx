---
title: Events & tracing
description: Structured event taxonomy, OTLP configuration, and how to consume Versionx's observability stream.
sidebar_position: 6
---

# Events & tracing

:::info
The event catalog below is **auto-generated** from `versionx-events`. Edit the enum variants there and re-run `cargo xtask docs-events`.
:::

Every Versionx operation emits structured events. The same stream feeds:

- The CLI's progress output.
- The TUI dashboard.
- The daemon's RPC notifications (forwarded to CLI/TUI/web/MCP clients).
- OTLP export (if you configured an endpoint).
- The state DB's run history (Zstd-compressed per run).

This page documents the taxonomy and how to consume it.

## Enabling

### On stderr (human-friendly)

```bash
VERSIONX_LOG=debug versionx sync
```

### As JSON lines

```bash
VERSIONX_LOG_FORMAT=json versionx sync
```

Each line is one JSON event. Pipe to `jq`:

```bash
versionx sync --output ndjson | jq 'select(.kind | startswith("adapter."))'
```

### As OTLP

```bash
export VERSIONX_OTLP_ENDPOINT=http://localhost:4317
versionx sync
```

Traces land in whatever backend you've configured (Jaeger, Tempo, Honeycomb, Datadog — anything that speaks OTLP).

## Event shape

Every event is the same JSON object:

```json
{
  "ts": "2026-04-18T14:02:17.123Z",
  "kind": "adapter.invocation.started",
  "span_id": "6f1e...",
  "parent_id": "3a2c...",
  "workspace_id": 12,
  "data": {
    "ecosystem": "node",
    "path": "apps/web",
    "command": "pnpm install"
  }
}
```

## Categories

Events are categorized by dotted prefix:

- `config.*` — config load, validation, migration.
- `adapter.*` — ecosystem adapter invocation, output, completion.
- `runtime.*` — runtime download, extract, shim install.
- `policy.*` — policy evaluation, violation, warning.
- `release.*` — plan, bump, tag, publish.
- `git.*` — fetch, push, subtree sync.
- `state.*` — DB write, query, migration.
- `saga.*` — multi-repo saga lifecycle.
- `mcp.*` — MCP transport-specific events.
- `rpc.*` — daemon RPC events.

## Auto-generated catalog

Below this line is the generated event catalog. If you're reading this before the `docs-events` xtask has run for the first time, see `crates/versionx-events/src/events.rs` for the enum directly.

{/* GENERATED-BELOW */}

{/* The xtask populates this section with one H3 per event variant, including:
     - Variant name (e.g., `adapter.invocation.started`)
     - Data fields and their types
     - Emitting crate
     - Since-version
*/}

_Event catalog pending first generation. Run `cargo xtask docs-events`._

{/* GENERATED-ABOVE */}

## Consumption patterns

### From an AI agent

Agents connected via [MCP](/integrations/mcp/overview) receive events as progress notifications on any streaming tool call.

### From your own code

Via the [Rust SDK](/sdk/overview):

```rust
use versionx_sdk::{Core, EventBus, Event};

let bus = EventBus::new();
let mut rx = bus.subscribe();
let core = Core::with_event_bus(bus);

tokio::spawn(async move {
    while let Ok(event) = rx.recv().await {
        if event.kind.starts_with("policy.") {
            eprintln!("{event:?}");
        }
    }
});

core.sync(Default::default()).await?;
```

### From a shell pipeline

```bash
versionx sync --output ndjson \
  | jq 'select(.kind == "adapter.invocation.completed") | .data'
```

## See also

- [Design principles](/introduction/design-principles) — principle 8, observability.
- [Environment variables](./environment-variables) — `VERSIONX_OTLP_*` config.
