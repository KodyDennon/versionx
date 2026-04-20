---
title: Status & roadmap
description: Versionx is 0.7 feature-complete. Thirty crates, 280+ tests. Path to 1.0 runs through stability hardening and ecosystem breadth.
sidebar_position: 3
---

# Status & roadmap

Versionx is **0.7 feature-complete**. Every subsystem called out in the [spec](https://github.com/KodyDennon/versionx/tree/main/docs/spec) has landed as real code with tests. The path to 1.0 is hardening, not new invention.

## What's shipped today

- **Workspace discovery.** Auto-detects Node, Python, and Rust projects without any config. Produces a sensible `versionx.toml` on request.
- **Content-hash bump planner.** Plans dependency and release bumps with Blake3 prerequisites and a TTL so plans are safe to share.
- **Release engine.** Plan / approve / apply flow with rollback for every release strategy (`pr-title`, `changesets`, `commits`, `calver`, `manual`).
- **Policy engine.** Declarative TOML rules plus a sandboxed Luau evaluator for complex logic. Waivers with mandatory expiry.
- **MCP server.** Full `rmcp` implementation, stdio + local HTTP, ~10 workflow-shaped tools.
- **BYO-API-key AI overlay.** Anthropic, OpenAI, Gemini, and Ollama clients for headless use. No bundled LLM.
- **Versiond daemon.** JSON-RPC 2.0 over UDS / named pipes, file-watch cache invalidation, event streaming.
- **TUI dashboard.** `ratatui`-based fleet view and release planner.
- **Cross-repo fleet orchestration.** Saga protocol for atomic multi-repo releases.
- **30 crates, 280+ tests.** Unit, property, snapshot, and integration coverage.

## Version badges

Every Reference page on this site carries one of:

- **Stable** — shipped, tested, contract-locked until the next major.
- **Experimental** — shipped, but the surface may change before 1.0.
- **Planned** — not yet implemented. Tracked on the [roadmap](/roadmap).

Anything without a badge is Stable.

## Road to 1.0

1.0 ships when:

1. Every subsystem is Stable (no Experimental badges in Reference).
2. All Tier-1 ecosystems (Node, Python, Rust) have a full adapter + runtime pair with migration paths.
3. Tier-2 ecosystems (Go, Ruby) land as adapters (runtimes can trail).
4. The reusable GitHub Actions are published to a stable org.
5. Windows parity for every feature that has a Unix-only corner.
6. Fleet operations have been exercised by at least one outside user on ≥20 repos.

Target: late 2026. No committed date.

## Post-1.0 (1.1 / 1.2 / 2.0)

- **1.1** — Go + Ruby adapters and runtimes reach Stable. OCI adapter lands.
- **1.2** — Tier-2 JVM adapter (maven/gradle). Local task cache. Remote state DB (Postgres backend). Optional bearer-auth for the HTTP API.
- **2.0** — Remote task cache. Hosted GitHub App (maybe — still a business question).

The full per-version plan with gates and demos lives in [`docs/spec/11-version-roadmap.md`](https://github.com/KodyDennon/versionx/blob/main/docs/spec/11-version-roadmap.md).

## What's deliberately deferred

- **Hosted GitHub App.** Reusable Actions are enough for 1.0. SaaS infrastructure burden isn't worth the user experience delta yet.
- **Remote-exposed HTTP API.** Loopback-only in 1.0. Remote is a 1.2 question with real auth and rate-limiting.
- **Bundled LLM.** Versionx will not ship a model. Ever.
- **Telemetry.** Not in 1.0. Not in 2.0. Not ever.

## See also

- [Roadmap](/roadmap) — the version-sliced feature timeline.
- [Design principles](/introduction/design-principles) — why the roadmap is shaped this way.
- [`docs/spec/11-version-roadmap.md`](https://github.com/KodyDennon/versionx/blob/main/docs/spec/11-version-roadmap.md) — the authoritative internal roadmap.
