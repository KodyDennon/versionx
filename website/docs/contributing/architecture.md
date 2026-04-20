---
title: Architecture
description: Summary of the architectural rules every contributor needs to know — plus a pointer to the canonical internal spec.
sidebar_position: 3
---

# Architecture

The canonical architecture document is [`docs/spec/01-architecture-overview.md`](https://github.com/KodyDennon/versionx/blob/main/docs/spec/01-architecture-overview.md). Read that for the full picture. This page is the short version every contributor needs to keep in head.

## The crate split

- **`versionx-core`** is the brain. Everything mutating originates here.
- **Frontends** (`versionx-cli`, `versionx-tui`, `versionx-daemon`, `versionx-web`, `versionx-mcp`) are thin — they translate between a transport and `versionx-core`.
- **Adapters** (`versionx-adapter-*`) drive real ecosystem tools. They implement one trait.
- **Runtimes** (`versionx-runtime-*`) install language toolchains. They implement another trait.
- **Supporting crates** (`versionx-config`, `-lockfile`, `-state`, `-policy`, `-release`, etc.) are single-purpose libraries used by core.

## Dependency rules (load-bearing)

1. **`versionx-core`** depends only on: adapters, runtimes, config, lockfile, state, policy, release, tasks, multirepo, git, github, events.
2. **Frontends** depend on `versionx-core`. They **never** import adapters, runtimes, or state directly.
3. **Adapters** depend only on `versionx-adapter-trait` and common utility crates. Adapters **never** depend on `versionx-core`. One-way.
4. **No frontend depends on another frontend.**

These rules are enforced by `cargo-deny` config + architecture tests. Violating them is a CI failure.

## Why the rules exist

- Clean core/frontend split → open-core monetization later is a clean boundary, not surgery.
- One-way adapter dep → adapters can be extracted to separate crates or repos at any time.
- No frontend-to-frontend → the TUI, CLI, MCP server are independently swappable.

## Process model (short)

- **Mode A — Direct.** CLI → core → adapters. Used in CI, `--no-daemon`, first run. ~20ms startup.
- **Mode B — Daemon-backed.** CLI ↔ daemon ↔ core. Default interactive mode. Socket at `$XDG_RUNTIME_DIR/versionx/daemon.sock` (or platform equivalent). ~1ms steady-state.
- **Mode C — Daemon + web + MCP.** The daemon exposes HTTP and MCP over loopback in addition to the CLI socket.

Full diagram and trace of `versionx sync` end-to-end in the [spec](https://github.com/KodyDennon/versionx/blob/main/docs/spec/01-architecture-overview.md#4-data-flow-versionx-sync-worked-example).

## The library boundary

`versionx-core` is a **stateless intent engine** that produces and applies `Plan`s. Frontends are prohibited from:

- Directly calling `git` or ecosystem tools.
- Writing to the state DB without going through `versionx-state`.
- Reimplementing resolution or pinning logic.

If you find yourself reaching for `Command::new("git")` in a frontend, that's the signal that a helper belongs in `versionx-git` or `versionx-core`.

## Transport surfaces

Every user-visible capability lives in `versionx-core`. Transports are thin.

- **CLI** (`versionx`) — `clap` derive. `--help-json` emits the full command tree.
- **TUI** (`versionx tui`) — `ratatui`; reads state, fires the same core calls as CLI.
- **Daemon RPC** — JSON-RPC 2.0 over UDS / named pipe. Methods map 1:1 to core.
- **HTTP API** — axum on loopback; OpenAPI via `aide`.
- **MCP** — `rmcp`. Stdio + streamable HTTP. ~10 workflow-shaped tools.

## See also

- [Workspace tour](./workspace-tour) — one paragraph per crate.
- [`docs/spec/01-architecture-overview.md`](https://github.com/KodyDennon/versionx/blob/main/docs/spec/01-architecture-overview.md) — the authoritative version.
