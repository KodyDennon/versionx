---
title: Environment variables
description: Every environment variable Versionx reads, what it does, and its default.
sidebar_position: 7
---

# Environment variables

Everything Versionx reads from the environment, grouped by category.

## Paths and directories

| Variable | Default | Description |
|---|---|---|
| `VERSIONX_HOME` | unset | Master override. If set, every other path below is derived from this directory. Mirrors the `~/.vers`-style layouts users coming from older tools expect. |
| `VERSIONX_CONFIG_HOME` | `$XDG_CONFIG_HOME/versionx` | User config directory. |
| `VERSIONX_DATA_HOME` | `$XDG_DATA_HOME/versionx` | Runtimes, state DB, shims. |
| `VERSIONX_CACHE_HOME` | `$XDG_CACHE_HOME/versionx` | Download and resolution cache. |
| `VERSIONX_STATE_HOME` | `$XDG_STATE_HOME/versionx` | Logs. |
| `VERSIONX_RUNTIME_DIR` | `$XDG_RUNTIME_DIR/versionx` | Daemon socket. Linux-only. |

## Daemon

| Variable | Default | Description |
|---|---|---|
| `VERSIONX_DAEMON` | `auto` | `auto` / `on` / `off`. Off forces `--no-daemon` for every invocation. |
| `VERSIONX_DAEMON_SOCKET` | platform default | Override the daemon socket path. |
| `VERSIONX_DAEMON_TIMEOUT_MS` | `30000` | Client-side timeout when talking to the daemon. |

## Logging and tracing

| Variable | Default | Description |
|---|---|---|
| `VERSIONX_LOG` | `info` | `trace` / `debug` / `info` / `warn` / `error`. `RUST_LOG` is also respected with the same syntax. |
| `VERSIONX_LOG_FORMAT` | `auto` | `auto` / `pretty` / `json`. `json` emits one JSON object per line. |
| `VERSIONX_OTLP_ENDPOINT` | unset | OTLP endpoint (e.g., `http://localhost:4317`). Opt-in. |
| `VERSIONX_OTLP_PROTOCOL` | `grpc` | `grpc` / `http/protobuf`. |

## AI (BYO API key)

| Variable | Default | Description |
|---|---|---|
| `ANTHROPIC_API_KEY` | unset | Used by AI-assisted surfaces such as `versionx changelog draft` and MCP-adjacent flows if Anthropic is selected. |
| `OPENAI_API_KEY` | unset | OpenAI path. |
| `GEMINI_API_KEY` | unset | Google Gemini path. |
| `OLLAMA_BASE_URL` | unset | Local Ollama endpoint. |
| `VERSIONX_AI_PROVIDER` | unset | Explicit provider selection when multiple are configured. |

Versionx never calls an LLM unless one of these is set.

## GitHub

| Variable | Default | Description |
|---|---|---|
| `GITHUB_TOKEN` | unset | Used by `versionx-github` for API calls. Standard GitHub Actions usage. |
| `VERSIONX_GH_APP_ID` | unset | App-mode authentication (1.1+). |
| `VERSIONX_GH_APP_PRIVATE_KEY` | unset | App-mode private key path. |

## Networking

| Variable | Default | Description |
|---|---|---|
| `HTTP_PROXY` / `HTTPS_PROXY` / `NO_PROXY` | unset | Standard proxy envs honored by `reqwest`. |
| `VERSIONX_OFFLINE` | `0` | `1` forces offline mode — no downloads, no network probes. |

## CI detection

| Variable | Default | Description |
|---|---|---|
| `CI` | unset | Standard flag. When set, Versionx defaults to `--no-daemon`, disables color, and uses `--output json` for structured steps. |

## Shim dispatch

| Variable | Default | Description |
|---|---|---|
| `VERSIONX_SHIM_DEBUG` | `0` | `1` logs the shim's dispatch decision to stderr — useful for debugging "wrong version" issues. |
| `VERSIONX_SHIM_CACHE` | `$VERSIONX_DATA_HOME/shim-cache.bin` | Path to the mmap'd PATH cache. |

## See also

- [State DB schema](./state-db-schema) — the DB that `VERSIONX_DATA_HOME` points to.
- [Managing toolchains](/guides/managing-toolchains) — where the shim variables matter.
