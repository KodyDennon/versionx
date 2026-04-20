---
title: Workspace tour
description: One-paragraph orientation to every crate in the Versionx workspace, with the right starting file in each.
sidebar_position: 2
---

# Workspace tour

Thirty crates. Below is a paragraph-per-crate orientation with the file to read first. For the canonical dependency rules, see [Architecture](./architecture) and the [spec](https://github.com/KodyDennon/versionx/blob/main/docs/spec/01-architecture-overview.md).

## Core and SDK

### `versionx-core`

The brain. Every mutating operation originates here. Produces `Plan`s; applies `Plan`s. Stateless intent engine. **Start at `src/commands/mod.rs`** — the entry point for every verb.

### `versionx-sdk`

Public crate that re-exports the stable subset of `versionx-core`. Versioned with the tool. **Start at `src/lib.rs`.**

## Frontends

### `versionx-cli`

The `versionx` binary. `clap`-derived. Parses args, calls `versionx-core`, renders output. **Start at `src/main.rs`.**

### `versionx-tui`

The interactive dashboard. `ratatui` + `crossterm`. Views: Dashboard, Repo Detail, Release Planner, Policy Inspector, Run Log. **Start at `src/app.rs`.**

### `versionx-daemon`

The `versiond` long-running process. JSON-RPC 2.0 over UDS / named pipe. Handlers map 1:1 to `versionx-core` public functions. **Start at `src/server.rs`.**

### `versionx-web`

Loopback-only axum HTTP surface. OpenAPI via `aide`. **Start at `src/router.rs`.**

### `versionx-mcp`

MCP server on `rmcp`. Stdio + streamable HTTP transports. Tool count ~10, workflow-shaped. **Start at `src/tools.rs`.**

## Shims and bootstrap

### `versionx-shim`

Tiny static binary (< 200 KB). Dispatches via `argv[0]` lookup against an mmap'd PATH cache. Sub-ms cold dispatch. **Start at `src/main.rs`.** Built under the `shim` profile for minimum size.

## Adapters (package managers)

### `versionx-adapter-trait`

The `PackageManagerAdapter` trait + test kit. Adapters never depend on `versionx-core`. **Start at `src/lib.rs`.**

### `versionx-adapter-node`

npm / pnpm / yarn. Drives them via their native CLIs. **Start at `src/adapter.rs`.**

### `versionx-adapter-python`

pip / uv / poetry. Same pattern. **Start at `src/adapter.rs`.**

### `versionx-adapter-rust`

cargo. **Start at `src/adapter.rs`.**

### `versionx-adapters`

Meta-crate re-exporting every adapter for convenient consumption from the CLI and daemon. **Start at `src/lib.rs`.**

## Runtimes (toolchain installers)

### `versionx-runtime-trait`

The `RuntimeInstaller` trait. **Start at `src/lib.rs`.**

### `versionx-runtime-node`

Installs Node and pins pnpm/yarn directly (no corepack). **Start at `src/installer.rs`.**

### `versionx-runtime-python`

Installs CPython via `python-build-standalone`. **Start at `src/installer.rs`.**

### `versionx-runtime-rust`

Wraps rustup. Never sets `RUSTC` in the env it hands out (leaves that to rustup). **Start at `src/installer.rs`.**

## Configuration and state

### `versionx-config`

TOML schema, validation, migration. Uses `toml_edit` so it preserves formatting on rewrites. **Start at `src/schema.rs`.**

### `versionx-lockfile`

Read/write for `versionx.lock`. Blake3-hash aggregation of native lockfiles. **Start at `src/format.rs`.**

### `versionx-state`

SQLite state DB. rusqlite + WAL. Migrations via `rusqlite_migration`. **Start at `src/db.rs`.**

## Logic engines

### `versionx-policy`

Declarative TOML parser + Luau evaluator (sandboxed via `mlua`). **Start at `src/evaluator.rs`.**

### `versionx-release`

SemVer, CalVer, PR-title parser, Conventional Commits, changesets. **Start at `src/strategies/mod.rs`.**

### `versionx-tasks`

Topological task runner. Local cache in 1.2; remote cache in 2.0. **Start at `src/runner.rs`.**

### `versionx-multirepo`

Submodule / subtree / virtual-monorepo / ref handlers. Saga protocol. **Start at `src/topology.rs`.**

## Git and GitHub

### `versionx-git`

Git ops. `gix` for reads, `git2` for writes. **Start at `src/repo.rs`.**

### `versionx-github`

Thin `octocrab` wrapper. **Start at `src/client.rs`.**

## Workspace glue

### `versionx-workspace`

Workspace-layer orchestration. Discovers roots, composes members. **Start at `src/lib.rs`.**

### `versionx-events`

Structured event bus. tracing-compatible, broadcast channel-based. **Start at `src/events.rs`.**

## Meta

### `xtask`

Internal build automation (CI chores, docs regeneration, etc.). **Start at `src/main.rs`.**

## See also

- [Architecture](./architecture) — the dependency rules between these crates.
- [Dev environment setup](./dev-environment) — getting a build running.
