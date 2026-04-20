---
title: MCP server overview
description: How AI agents drive Versionx through the Model Context Protocol. Stdio and HTTP transports, tool count discipline, and the plan/apply safety model.
sidebar_position: 1
---

# MCP server overview

Versionx ships a full MCP server built on the official [`rmcp`](https://github.com/modelcontextprotocol/rust-sdk) Rust SDK. Any MCP-aware agent can drive Versionx — plan releases, update dependencies, install runtimes, evaluate policies — without writing any glue code.

## Transports

Two transports, both shipped in the same binary:

- **stdio** (primary). The agent spawns `versionx mcp` as a child process and speaks JSON-RPC over stdin/stdout. Used by Claude Code, Cursor, Codex, Qwen, and most others.
- **Local HTTP** (streamable). The daemon exposes the MCP server at `http://127.0.0.1:<port>/mcp` on loopback. Used for persistent sessions (browser-based agents, custom integrations) where spawn-per-call isn't a fit.

No remote-exposed MCP in 1.0. Loopback only. Remote with proper auth is a 1.2+ question.

## Starting the server

### Stdio

Configured per agent — see [Claude Code](./claude-code), [Cursor](./cursor), [Codex](./codex), [Qwen](./qwen), [Ollama](./ollama).

Manual:

```bash
versionx mcp --transport stdio
```

### HTTP

```bash
versionx mcp --transport http --port 7777
```

The daemon starts this automatically when it sees an MCP-tagged configuration.

## Tools

The tool count is **intentionally capped at around ten**, workflow-shaped rather than one-tool-per-command. Research on MCP agent accuracy is unambiguous: large tool catalogs degrade agent performance. Versionx's shape:

| Tool | Mutating? | Shape |
|---|---|---|
| `inspect` | no | Read workspace state. Subsumes `status`, `current`, `list`. |
| `plan` | no | Produce a plan for one of: `update`, `release`, `sync`, `install`. |
| `apply` | yes | Apply a previously generated plan. |
| `propose_and_apply` | yes | Elicitation-based "plan, confirm with user, apply" in one call. |
| `bump` | yes | Shortcut for common release bumps. |
| `policy_eval` | no | Evaluate policies against a draft plan. |
| `waiver_grant` | yes | Create a waiver with mandatory expiry. |
| `saga` | yes | Drive a multi-repo saga: plan / apply / compensate. |
| `run` | yes | Execute a task from `[tasks]`. |
| `doctor` | no | Diagnose setup issues. |

Every mutating tool has `_plan` and `_apply` variants. The `_propose_and_apply` variant uses MCP elicitation (falls back to two sequential calls when the client doesn't support elicitation).

## Resources and prompts

- **Resources.** `versionx://config`, `versionx://state/repos`, `versionx://policy/rules`. Every resource is **also** mirrored as a tool, because client support for resources is uneven.
- **Prompts.** Shipped: `propose_release`, `audit_dependency_freshness`, `remediate_policy_violation`.

## Safety

Every mutating tool produces a JSON plan with:

- Blake3 hashes of every prerequisite (lockfile, config, HEAD).
- A configurable TTL (default 5 minutes).
- A structured action list.

`apply` re-checks prerequisites at execution time. If anything has changed, `apply` fails cleanly. This is the same contract that covers CLI plan/apply — MCP inherits it unchanged.

## No bundled LLM

Versionx never calls an LLM on its own through MCP. The agent's LLM does the reasoning. Versionx serves context and accepts plans.

For non-MCP environments where you still want AI assistance (e.g., a `--ai-changelog` flag), configure an API key. See [Environment variables](/reference/environment-variables) for the list.

## Tool catalog

See [Tool catalog](./tool-catalog) for the auto-generated per-tool reference.

## See also

- Per-agent setup: [Claude Code](./claude-code), [Cursor](./cursor), [Codex](./codex), [Qwen](./qwen), [Ollama](./ollama).
- [Plan / apply cookbook](/sdk/plan-apply-cookbook) — the contract every tool participates in.
