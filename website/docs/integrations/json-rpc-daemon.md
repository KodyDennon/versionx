---
title: JSON-RPC daemon
description: The versiond daemon speaks JSON-RPC 2.0 over a Unix socket or named pipe. Method list, message shapes, and streaming semantics.
sidebar_position: 3
---

# JSON-RPC daemon

:::info
The method list is **auto-generated** from the daemon's handler registry via `cargo xtask docs-rpc`.
:::

`versiond` — the Versionx daemon — speaks JSON-RPC 2.0 over a per-user local socket. The CLI, TUI, MCP server, and HTTP API are all clients of this protocol.

## Transport

- **Linux:** Unix Domain Socket at `$XDG_RUNTIME_DIR/versionx/daemon.sock`. Permissions `0600`.
- **macOS:** UDS at `~/Library/Application Support/versionx/daemon.sock`. Permissions `0600`.
- **Windows:** Named pipe `\\.\pipe\versionx-daemon-<user>`. SDDL restricts to the user's SID.
- **Framing:** 4-byte big-endian length prefix + JSON body.

## Protocol

JSON-RPC 2.0 requests and responses:

```json
// Request
{"jsonrpc": "2.0", "id": 1, "method": "sync", "params": {"cwd": "/repo"}}

// Response (final)
{"jsonrpc": "2.0", "id": 1, "result": {"status": "ok", "plan_id": "..."}}

// Response (error)
{"jsonrpc": "2.0", "id": 1, "error": {"code": 3, "message": "Policy violation"}}
```

Long-running methods emit **notifications** during execution:

```json
{"jsonrpc": "2.0", "method": "progress", "params": {"span_id": "6f1e...", "kind": "adapter.invocation.started", "data": {...}}}
```

Notifications follow the [event taxonomy](/reference/events) exactly — the daemon forwards `versionx-events` events as RPC notifications unchanged.

## Authentication

None in 1.0. The Unix socket is `0600` and the Windows pipe is SID-restricted — only the process owner can connect. Remote-exposed mode with bearer auth is a [1.2+ roadmap item](/roadmap).

## Method catalog

The full method list mirrors the CLI's command tree. Each `versionx <subcommand>` has a corresponding method:

| Method | Kind | Streaming? |
|---|---|---|
| `workspace.init` | Mutating | no |
| `workspace.status` | Read | no |
| `sync` | Mutating | yes |
| `install` | Mutating | yes |
| `update.plan` / `update.apply` | Read / Mutating | yes |
| `release.plan` / `release.apply` / `release.rollback` | Read / Mutating | yes |
| `policy.eval` / `policy.waiver.grant` / `policy.waiver.list` | Read / Mutating | no |
| `saga.plan` / `saga.apply` / `saga.compensate` / `saga.status` | Mutating / Read | yes |
| `state.inspect` / `state.rebuild` | Read / Mutating | no |
| `doctor` | Read | no |
| `daemon.ping` / `daemon.shutdown` | Read / Mutating | no |

Streaming methods emit notifications; the final `result` arrives after the last notification.

## Auto-generated per-method reference

{/* GENERATED-BELOW */}

_Method reference pending first generation. Run `cargo xtask docs-rpc`._

{/* GENERATED-ABOVE */}

## Schema

OpenRPC document published at `$XDG_DATA_HOME/versionx/schema/rpc.json` — `daemon.schema` method returns it inline too.

## Clients

- **Rust.** Use [`versionx-sdk`](/sdk/overview) — `DaemonClient` handles framing and notifications.
- **Other languages.** Any JSON-RPC 2.0 client works; handle the 4-byte length prefix yourself. Python / Node / Go examples in [`examples/`](https://github.com/KodyDennon/versionx/tree/main/examples) in the repo.

## See also

- [HTTP API](./http-api) — the same capability surface over HTTP.
- [MCP server overview](/integrations/mcp/overview) — the agent-facing layer on top of the daemon.
