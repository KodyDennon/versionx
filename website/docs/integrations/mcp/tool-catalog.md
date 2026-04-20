---
title: Tool catalog
description: Every MCP tool Versionx exposes, with arguments, return shape, and usage notes.
sidebar_position: 7
---

# Tool catalog

:::info
This page is **auto-generated** from the `versionx-mcp` tool registrations via `cargo xtask docs-mcp`. Edit tool descriptions in `crates/versionx-mcp/src/tools.rs` and re-run the generator.
:::

## Tool summary

| Name | Mutating | Purpose |
|---|---|---|
| `inspect` | no | Read workspace state. |
| `plan` | no | Produce a plan for `update` / `release` / `sync` / `install`. |
| `apply` | yes | Apply a previously generated plan. |
| `propose_and_apply` | yes | Elicitation-based plan + confirm + apply. |
| `bump` | yes | Shortcut for common release bumps. |
| `policy_eval` | no | Evaluate policies against a plan. |
| `waiver_grant` | yes | Create a waiver with mandatory expiry. |
| `saga` | yes | Drive a multi-repo saga. |
| `run` | yes | Execute a `[tasks]` task. |
| `doctor` | no | Diagnose setup issues. |

## Auto-generated per-tool reference

{/* GENERATED-BELOW */}

_Tool reference pending first generation. Run `cargo xtask docs-mcp`._

{/* GENERATED-ABOVE */}

## See also

- [MCP server overview](./overview)
- [Plan / apply cookbook](/sdk/plan-apply-cookbook) — the safety contract every mutating tool participates in.
