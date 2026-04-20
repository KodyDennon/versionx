---
title: Plan / apply cookbook
description: Recipes for producing, serializing, transporting, and applying plans safely from the Rust SDK.
sidebar_position: 3
---

# Plan / apply cookbook

Every mutating operation in Versionx is a **plan** that you can produce, inspect, transport, and apply later. Prerequisites are Blake3-hashed; a TTL bounds how long the plan is safe to apply.

This is how humans and AI agents share workflows without either losing safety.

## Produce a plan

```rust
use versionx_sdk::commands::UpdateOptions;

let plan = core.update(UpdateOptions::plan_only()).await?;
```

Every `commands::*Options` type has a `plan_only()` / `plan()` / `apply()` builder — consistent across `sync`, `update`, `release`, `install`.

## Inspect

The plan is a serializable struct:

```rust
println!("id:          {}", plan.id);
println!("kind:        {}", plan.kind);
println!("ttl:         {}s", plan.ttl.as_secs());
println!("blake3(head): {}", plan.prereqs.head_blake3);
println!("blake3(lock): {}", plan.prereqs.lock_blake3);
println!("steps:");
for step in &plan.steps {
    println!("  {} -> {}", step.description(), step.effect());
}
```

## Serialize

```rust
let json = serde_json::to_string_pretty(&plan)?;
std::fs::write("plan.json", &json)?;
```

JSON is stable; round-trip is guaranteed within a major version.

## Deserialize and apply

```rust
let json = std::fs::read_to_string("plan.json")?;
let plan: versionx_sdk::Plan = serde_json::from_str(&json)?;

let outcome = core.apply(plan).await?;
```

`apply` re-checks prerequisites atomically before running any mutation. If the HEAD hash, lockfile hash, or TTL don't match, it returns `Error::PrerequisitesChanged` or `Error::PlanExpired`. No partial apply.

## TTL tuning

```rust
use std::time::Duration;

let plan = core
    .update(UpdateOptions::plan_only().ttl(Duration::from_secs(60 * 30)))
    .await?;
```

Default TTL is 5 minutes. Longer TTLs are useful for PR-review flows where a plan lives in a PR comment. Shorter TTLs are useful for automation pipelines that want a tight "apply immediately" window.

## Round-trip through CI / humans

A common shape:

1. CI produces a plan on PR open. Posts it as a PR comment.
2. Reviewer reads the plan, approves the PR.
3. CI on merge reads the plan, re-checks, applies.

```rust
// On PR open
let plan = core.update(UpdateOptions::plan_only().ttl(Duration::from_secs(60 * 60 * 24))).await?;
post_pr_comment(&plan)?;

// On merge
let plan: versionx_sdk::Plan = parse_pr_comment()?;
let outcome = core.apply(plan).await?;   // fails if anything changed
```

The TTL is the safety net. If a PR sits for three days and a lockfile changed in the meantime, `apply` fails cleanly and CI regenerates.

## Plans across transports

The same JSON moves freely:

- CLI → file → CLI.
- CLI → stdout → pipe → daemon via MCP.
- Daemon → HTTP → browser → back through HTTP to the daemon.
- SDK → your own RPC → other SDK.

## Rejecting expired plans on purpose

```rust
use versionx_sdk::Error;

match core.apply(plan).await {
    Err(Error::PrerequisitesChanged { .. }) => {
        println!("World moved. Regenerating…");
        let fresh = core.update(UpdateOptions::plan_only()).await?;
        core.apply(fresh).await?;
    }
    Err(Error::PlanExpired { .. }) => {
        eprintln!("TTL expired, refusing to apply.");
    }
    res => { res?; }
}
```

## See also

- [Embedding versionx-core](./embedding) — how to construct a `Core`.
- [MCP server overview](/integrations/mcp/overview) — the same contract served to AI agents.
