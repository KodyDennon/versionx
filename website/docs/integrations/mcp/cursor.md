---
title: Cursor
description: Wire Versionx into Cursor's MCP configuration.
sidebar_position: 3
---

# Cursor

Cursor supports MCP over stdio. Settings live in `~/.cursor/mcp.json` (global) or `.cursor/mcp.json` at the repo root (per-project — recommended).

## Configure

```json
{
  "mcpServers": {
    "versionx": {
      "command": "versionx",
      "args": ["mcp", "--transport", "stdio"],
      "type": "stdio"
    }
  }
}
```

Reload the window. Versionx tools appear in the agent sidebar under the `versionx` group.

## First conversation

> `@versionx` What's outdated in this repo?

Cursor resolves the `@versionx` reference to the MCP server and runs `inspect`.

> Plan a minor update for the tokio package.

Runs `plan` with the narrowed scope.

> Apply it.

Runs `apply`. Prerequisites verified, result streamed back.

## Troubleshooting

- **Cursor can't find `versionx`.** Use the absolute path in `"command"`.
- **Tools show up but calls hang.** Check `versionx daemon status`; the server holds a daemon connection for most operations.

## See also

- [MCP server overview](./overview)
- [Tool catalog](./tool-catalog)
