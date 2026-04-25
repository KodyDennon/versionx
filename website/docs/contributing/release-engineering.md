---
title: Release engineering
description: How Versionx releases itself. Dogfooding via the release engine, xtask jobs, dist config, and the docs regeneration pipeline.
sidebar_position: 8
---

# Release engineering

Versionx currently releases itself primarily through `cargo-dist` and GitHub
Actions. The long-term dogfooding path is still part of the roadmap.

## The release pipeline

1. PRs merge to `main` via squash with Conventional-Commit-style titles.
2. `.github/workflows/release.yml` runs the `cargo-dist` release flow.
3. Tag pushes and main-branch prerelease snapshots build platform artifacts.
4. GitHub Releases receive the generated archives/installers.
5. Additional package channels remain roadmap work until they are verified end to end.

## `dist-workspace.toml`

The workspace is packaged via `cargo-dist` (config in `dist-workspace.toml` at the root). It handles:

- Platform matrix builds.
- Archive naming.
- Installer generation (curl-shell, PowerShell).
- Future package-manager distribution work once those channels are live.

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
- **crates.io.** `versionx-sdk`, `versionx-adapter-trait`, `versionx-runtime-trait`, `versionx-config`, `versionx-events`, `versionx-adapter-*`, `versionx-runtime-*` publish. The `versionx-cli` binary is also published for `cargo install versionx-cli`.
- **Homebrew / Scoop / npm / PyPI.** Planned, but do not describe them as live until the public release flow is verified.

## Versioning

- Pre-1.0, all crates share the same version bumped in lockstep.
- Post-1.0, the SDK (`versionx-sdk`, `versionx-adapter-trait`, `versionx-runtime-trait`, `versionx-config`, `versionx-events`) tracks tool majors. Other internal crates may diverge.

## Tagging

Tag shape: `v<X>.<Y>.<Z>` for the overall tool; `<crate>-v<X>.<Y>.<Z>` for individual crate releases. `cargo-dist` creates all tags.

## See also

- [Orchestrating a release](/guides/orchestrating-a-release) — the same flow from a user's perspective.
- [Dev environment setup](./dev-environment) — getting a build running locally.
