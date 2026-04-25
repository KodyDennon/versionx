---
title: Debugging & tracing
description: RUST_LOG patterns, OTLP setup, using --plan to inspect behavior, and common traps when a bug looks weird.
sidebar_position: 7
---

# Debugging & tracing

## Turn on logs

```bash
VERSIONX_LOG=debug versionx sync
# or the equivalent RUST_LOG form
RUST_LOG=versionx=debug,versionx_release=trace versionx release plan
```

Target-level filtering uses crate names. Common patterns:

```bash
# Adapter-level tracing only
RUST_LOG=versionx_adapter_node=trace,versionx=info versionx bump

# Policy evaluation trace
RUST_LOG=versionx_policy=trace versionx policy check
```

## Structured logs

```bash
VERSIONX_LOG_FORMAT=json versionx sync | jq
```

One JSON event per line. Pipe to `jq`, `fx`, or anything that speaks line-delimited JSON.

## OTLP

```bash
export VERSIONX_OTLP_ENDPOINT=http://localhost:4317
versionx sync
```

Traces go to whatever backend accepts OTLP — Jaeger, Tempo, Honeycomb, Datadog. The default export protocol is gRPC; set `VERSIONX_OTLP_PROTOCOL=http/protobuf` if your backend needs HTTP.

## Use the planner surfaces to inspect behavior

If you're chasing "why is this command doing X":

```bash
versionx release plan --output json | jq
```

The JSON plan is the exact set of mutations the release command would make.
Often you can spot the bug without running `apply`.

## Use `versionx doctor`

```bash
versionx doctor
```

Audits the environment: shell hook, PATH, daemon socket, runtime cache, state DB schema version, policy files. Prints anything suspicious with a suggested fix.

## Common traps

### "It worked yesterday"

- **Daemon holding stale state.** `versionx daemon stop && versionx daemon start`.
- **Shell hook missing.** Re-run `versionx install-shell-hook`, then open a new shell.
- **Clock skew.** Plan TTLs check against the system clock; if the clock jumped, recent plans may appear expired.

### "The plan doesn't match what happens"

- Prerequisites are re-verified on `apply`. If they fail, nothing runs — check the error.
- Adapter ran the native tool with different env than your shell. Turn on `VERSIONX_LOG=trace` to see the exact command + env.

### "Tests pass locally, fail on CI"

- Color output. CI sets `CI=true`; color is off. Snapshots should normalize.
- Timezones. Snapshots of timestamps need `VERSIONX_TEST_TZ=UTC`.
- File ordering. Use `BTreeMap` or sort explicitly.

### "MCP tool doesn't appear in Claude / Cursor"

- MCP client spawn env lacks your PATH. Use an absolute command path in the MCP config.
- Server crashed silently; run `versionx mcp --transport stdio --verbose` manually and read stderr.

## Attaching a debugger

```bash
cargo build -p versionx-cli --profile release-debug
rust-gdb target/release-debug/versionx
```

Or LLDB on macOS. The `release-debug` profile keeps optimizations but preserves debug info.

## See also

- [Events & tracing](/reference/events) — the event taxonomy users consume.
- [Writing tests](./writing-tests) — regressions surface as tests.
