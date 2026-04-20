---
title: Release engineering
description: How Versionx releases itself. Dogfooding via the release engine, xtask jobs, dist config, and the docs regeneration pipeline.
sidebar_position: 8
---

# Release engineering

Versionx releases itself with Versionx. Pragmatic dogfooding.

## The release pipeline

1. PRs merge to `main` via squash with Conventional-Commit-style titles.
2. On every merge, `.github/workflows/release.yml` runs `versionx release plan`.
3. If a release is due, a release PR opens (or a direct release cuts — we alternate depending on the change shape).
4. On release PR merge, the release applies, tags push, and `dist` builds the artifacts.
5. Artifacts publish to GitHub Releases, Homebrew tap, Scoop bucket, and crates.io.

## `dist-workspace.toml`

The workspace is packaged via `cargo-dist` (config in `dist-workspace.toml` at the root). It handles:

- Platform matrix builds.
- Archive naming.
- Installer generation (curl-shell, PowerShell).
- npm and PyPI shim packages.
- Homebrew formula and Scoop manifest updates.

Tweaks happen in `dist-workspace.toml`; don't add ad-hoc scripts to the workflow.

## xtask release chores

```bash
cargo xtask ci          # fmt + clippy + tests (local parity with CI)
cargo xtask docs        # regenerate every auto-gen docs page
cargo xtask crates      # list workspace members in dependency order
```

Add new chores here, not in the public `versionx` CLI.

## Docs regeneration

Reference pages on this site are generated from source:

| Page | xtask subcommand |
|---|---|
| CLI reference | `cargo xtask docs-cli` |
| `versionx.toml` reference | `cargo xtask docs-config` |
| Events catalog | `cargo xtask docs-events` |
| MCP tool catalog | `cargo xtask docs-mcp` |
| JSON-RPC methods | `cargo xtask docs-rpc` |
| Exit codes | `cargo xtask docs-exit-codes` |

CI gate: after `cargo xtask docs`, `git diff --exit-code -- website/` must pass. Drift between code and docs fails the build.

## Publishing

- **GitHub Releases.** `cargo-dist` produces the archives and installers and attaches them.
- **Homebrew.** Formula lives in `KodyDennon/homebrew-versionx`; `dist` pushes an update on each tag.
- **Scoop.** Bucket lives in `KodyDennon/scoop-versionx`; same flow.
- **crates.io.** `versionx-sdk`, `versionx-adapter-trait`, `versionx-runtime-trait`, `versionx-config`, `versionx-events`, `versionx-adapter-*`, `versionx-runtime-*` publish. The `versionx-cli` binary is also published for `cargo install versionx-cli`.
- **npm / PyPI.** Shim packages publish with each release via `dist`.

## Versioning

- Pre-1.0, all crates share the same version bumped in lockstep.
- Post-1.0, the SDK (`versionx-sdk`, `versionx-adapter-trait`, `versionx-runtime-trait`, `versionx-config`, `versionx-events`) tracks tool majors. Other internal crates may diverge.

## Tagging

Tag shape: `v<X>.<Y>.<Z>` for the overall tool; `<crate>-v<X>.<Y>.<Z>` for individual crate releases. `cargo-dist` creates all tags.

## Pre-release channels

```bash
versionx release pre-enter next
```

Releases tag as `v0.8.0-next.1`, `v0.8.0-next.2`. `pre-exit` rolls up to a stable bump.

## See also

- [Orchestrating a release](/guides/orchestrating-a-release) — the same flow from a user's perspective.
- [Dev environment setup](./dev-environment) — getting a build running locally.
