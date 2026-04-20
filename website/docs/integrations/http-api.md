---
title: HTTP API
description: The loopback-only axum HTTP surface. OpenAPI spec via aide, SSE event stream, REST-ish endpoints.
sidebar_position: 4
---

# HTTP API

`versionx-web` exposes a small axum HTTP surface on loopback. The same capability set as the [JSON-RPC daemon](./json-rpc-daemon), but HTTP-shaped for browser-based tooling and language-agnostic automation.

## Status

- **Loopback only** in 1.0. Binds `127.0.0.1:<port>`.
- **No auth.** Trust is the OS user + loopback.
- **Remote + bearer auth** is a [1.2+ roadmap](/roadmap) item.

## Starting

Automatic when the daemon runs:

```bash
versionx daemon start      # also starts the HTTP server on a random port
versionx daemon status     # prints the URL (e.g., http://127.0.0.1:47821)
```

Manual:

```bash
versionx web --port 7777
```

## Endpoints

| Method | Path | Purpose |
|---|---|---|
| `GET` | `/api/v1/repos` | List workspaces known to the daemon. |
| `GET` | `/api/v1/repos/{id}` | Workspace detail. |
| `POST` | `/api/v1/repos/{id}/sync` | Run a sync (returns plan + apply result). |
| `GET` | `/api/v1/plans` | List plans (recent, active). |
| `GET` | `/api/v1/plans/{id}` | One plan's detail. |
| `POST` | `/api/v1/plans/{id}/apply` | Apply a plan. |
| `GET` | `/api/v1/events` | SSE stream of daemon events. |
| `GET` | `/api/v1/policies` | Active policy set. |
| `GET` | `/api/v1/openapi.json` | OpenAPI 3 spec (via `aide`). |
| `GET` | `/docs` | Scalar UI against the OpenAPI spec. |

## SSE event stream

```bash
curl http://127.0.0.1:47821/api/v1/events
```

Emits events matching the [event taxonomy](/reference/events). One `data:` line per event, JSON body.

## OpenAPI

The spec is auto-generated from the axum routes and their types via [`aide`](https://crates.io/crates/aide). View the interactive reference at `/docs` (served by the daemon), or fetch JSON:

```bash
curl http://127.0.0.1:47821/api/v1/openapi.json | jq
```

## Using the spec

Generate a typed client in your language of choice:

```bash
# TypeScript
npx openapi-typescript http://127.0.0.1:47821/api/v1/openapi.json --output versionx.ts

# Python
datamodel-codegen --url http://127.0.0.1:47821/api/v1/openapi.json --output versionx_client.py
```

## Example: drive a sync from Node

```ts
const res = await fetch(`http://127.0.0.1:${port}/api/v1/repos/${id}/sync`, {
  method: "POST",
});
const result = await res.json();
console.log(result.plan_id, result.outcome);
```

## See also

- [JSON-RPC daemon](./json-rpc-daemon) — the same capabilities over JSON-RPC.
- [MCP server overview](/integrations/mcp/overview) — if you're integrating an AI agent instead of a service.
