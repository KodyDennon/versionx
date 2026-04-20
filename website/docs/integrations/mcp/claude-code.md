---
title: Claude Code
description: Wire the Versionx MCP server into Claude Code. Stdio configuration, a first example, and common pitfalls.
sidebar_position: 2
---

# Claude Code

Claude Code is the official CLI for Claude. It speaks MCP natively over stdio.

## Configure

Edit `~/.claude/mcp.json`:

```json
{
  "mcpServers": {
    "versionx": {
      "command": "versionx",
      "args": ["mcp", "--transport", "stdio"],
      "env": {}
    }
  }
}
```

Or per-project, edit `.claude/mcp.json` at the repo root with the same shape. Per-project config takes precedence.

Restart Claude Code. The Versionx tools show up under the `versionx:*` namespace.

## First prompt

Try:

> Inspect the workspace and tell me what's outdated.

Claude calls `versionx:inspect` and summarizes the outdated list.

> Now plan a patch-only update for Node packages.

Claude calls `versionx:plan` with `kind=update, scope=node, patch-only=true` and shows the proposed bumps.

> Apply the plan.

Claude calls `versionx:apply`. Prerequisites are re-verified; nothing applies if anything has moved.

## Agent prompts

Three prompts ship with the server and are available in Claude Code:

- `/versionx:propose_release` — walks you through proposing the next release.
- `/versionx:audit_dependency_freshness` — produces a prioritized freshness audit.
- `/versionx:remediate_policy_violation` — proposes fixes (or waivers) for current policy violations.

## Troubleshooting

- **`versionx: command not found` when Claude spawns.** Claude uses a non-login shell; your PATH edit from `install-shell-hook` may not apply. Add `"command": "/full/path/to/versionx"` in the MCP config.
- **Daemon says "permission denied" on socket.** You're on Linux and `$XDG_RUNTIME_DIR` isn't the same user as Claude Code. Run Claude as your usual user.
- **Tools don't appear.** Check `claude-code --mcp-debug` (or equivalent in your version). Versionx logs on stderr via `versionx mcp --verbose`.

## See also

- [MCP server overview](./overview) — the tool shape and safety model.
- [Tool catalog](./tool-catalog) — full per-tool reference.
