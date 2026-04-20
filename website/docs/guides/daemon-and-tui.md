---
title: The daemon & TUI
description: When to run versiond, what it caches, and how to use the TUI dashboard to see every repo at once.
sidebar_position: 6
---

# The daemon & TUI

You'll learn:

- What `versiond` is and when you want it running.
- How the daemon speeds up repeat invocations.
- The TUI dashboard and its keyboard shortcuts.

**Prerequisites:** Versionx [installed](/get-started/install); `versionx install-shell-hook` run once.

## The daemon in one paragraph

`versiond` is a long-running user-level process that holds cached workspace state, watches files for changes, and serves CLI / TUI / MCP / HTTP clients over a local socket. It starts automatically when your shell is activated and stays alive until you log out. You never have to start it manually, and it's stateless in the sense that deleting it and restarting reconstructs everything from config, lockfile, and git.

In CI, the daemon is **not** used — every invocation runs direct for predictability and to avoid cross-run state pollution.

## When the daemon kicks in

Automatic:

- Your shell hook starts it on first invocation of any `versionx` command.
- File-watch triggers cache invalidation when `versionx.toml` or lockfiles change.
- Long-running commands (`sync`, `release apply`, fleet ops) stream progress notifications back to every connected client.

Skip the daemon:

```bash
versionx --no-daemon status
```

Useful when you want a known-clean run and don't mind the ~20ms startup hit.

## Sockets

| Platform | Path |
|---|---|
| Linux | `$XDG_RUNTIME_DIR/versionx/daemon.sock` |
| macOS | `~/Library/Application Support/versionx/daemon.sock` |
| Windows | `\\.\pipe\versionx-daemon-<user>` |

Permissions on Unix are `0600` (owner only). Windows uses SDDL restricted to your user's SID. No auth token is needed locally — the OS provides the isolation.

## Inspecting the daemon

```bash
versionx daemon status        # up, pid, uptime, clients
versionx daemon stop          # send SIGTERM, wait for clean exit
versionx daemon restart
versionx daemon log --follow  # tail the daemon's event log
```

## The TUI

```bash
versionx tui
```

Views (switch with `1` .. `5`):

1. **Dashboard** — every repo Versionx is aware of. Status column, outstanding updates, policy state.
2. **Repo detail** — one repo at a time. Ecosystem breakdown, runtime pins, recent runs.
3. **Release planner** — pending changes across scopes; draft a release plan interactively.
4. **Policy inspector** — every active rule and every active waiver with time remaining.
5. **Run log** — the raw event stream. Filter with `f`.

Keyboard shortcuts:

| Key | Action |
|---|---|
| `1`–`5` | Switch view. |
| `?` | Help overlay. |
| `/` | Filter the current view. |
| `r` | Refresh. |
| `enter` | Drill into the selected row. |
| `esc` | Back / dismiss. |
| `q` | Quit. |
| `p` | Produce a plan for the selected scope. |
| `a` | Apply the most recent plan (with confirmation). |

Everything the TUI shows comes from the daemon's cached state. It's read-heavy by design: writes happen through normal plan/apply flows.

## Observability

Every operation emits structured events. Point OTLP at whatever endpoint you use:

```bash
export VERSIONX_OTLP_ENDPOINT=http://localhost:4317
versionx sync
```

See [Events & tracing](/reference/events) for the event taxonomy.

## Troubleshooting

- **Daemon won't start.** `versionx daemon log` shows the startup error. Common cause: stale socket from a previous session. `versionx daemon restart --force` removes it and retries.
- **Slow startup of every command.** You're hitting the no-daemon path. Check `versionx doctor`; your shell hook may not be active.
- **TUI crashes or renders funny.** Terminal emulator / font issues. Most often fixed by a terminal supporting truecolor + unicode. `versionx tui --ascii` renders a box-drawing-safe variant.

## See also

- [JSON-RPC daemon](/integrations/json-rpc-daemon) — the protocol the daemon speaks.
- [MCP server](/integrations/mcp/overview) — how agents connect.
