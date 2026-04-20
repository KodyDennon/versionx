---
title: versionx (root command)
description: The versionx CLI — top-level flags, global options, and the subcommand index.
sidebar_position: 1
---

# `versionx`

:::info
This page and every page under `reference/cli/` are **auto-generated** from the `clap` definitions in `versionx-cli`. Edit the derive attributes in `crates/versionx-cli/src/main.rs` (or its child modules) and re-run `cargo xtask docs-cli`.
:::

`versionx` is the main binary. Run it bare inside a repo to see the auto-detection output and suggested next commands.

## Global flags

| Flag | Default | Description |
|---|---|---|
| `--output <fmt>` | `human` | `human` / `json` / `ndjson`. `ndjson` streams events per-line — use for agents and shell pipelines. |
| `--quiet`, `-q` | off | Suppress all output except errors. |
| `--verbose`, `-v` | off | Increase verbosity. Stack: `-v`, `-vv`, `-vvv`. |
| `--cwd <path>` | current dir | Working directory for the command. |
| `--no-daemon` | off | Bypass the daemon and run in-process. |
| `--help-json` | off (hidden) | Emit the full command tree as JSON (used by the MCP server). |

## Subcommands

The complete subcommand catalog is generated under `reference/cli/`. Start points:

- `versionx init` — write a starter `versionx.toml`.
- `versionx status` — show workspace health.
- `versionx sync` — install runtimes + refresh lockfiles as needed.
- `versionx install` — install a specific runtime or package-manager version.
- `versionx update` — plan dependency bumps.
- `versionx apply` — apply a previously generated plan.
- `versionx release` — plan / apply / rollback releases.
- `versionx policy` — evaluate policies, manage waivers.
- `versionx fleet` — cross-repo operations.
- `versionx saga` — multi-repo saga control.
- `versionx tui` — interactive dashboard.
- `versionx daemon` — daemon lifecycle control.
- `versionx mcp` — start the MCP server.
- `versionx doctor` — diagnose common setup issues.

## Auto-generated detail

{/* GENERATED-BELOW */}

_Subcommand pages pending first generation. Run `cargo xtask docs-cli`._

{/* GENERATED-ABOVE */}

## See also

- [`versionx.toml` reference](../versionx-toml) — the config every subcommand consults.
- [Quickstart](/get-started/quickstart) — hands-on walkthrough.
