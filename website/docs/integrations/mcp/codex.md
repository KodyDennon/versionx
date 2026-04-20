---
title: Codex (OpenAI)
description: Wire Versionx into OpenAI's Codex CLI via MCP.
sidebar_position: 4
---

# Codex

OpenAI's Codex CLI supports MCP over stdio. Configuration lives in `~/.codex/config.toml`.

## Configure

```toml
[[mcp_servers]]
name = "versionx"
command = "versionx"
args = ["mcp", "--transport", "stdio"]
```

Restart the Codex session. The Versionx tools are available as `versionx.inspect`, `versionx.plan`, etc.

## Notes

- Codex favors fewer, workflow-shaped tools. Versionx's ~10-tool design fits cleanly.
- `versionx.propose_and_apply` uses MCP elicitation; Codex supports elicitation in recent builds. On older builds, Versionx falls back to `plan` followed by `apply`.

## See also

- [MCP server overview](./overview)
- [Tool catalog](./tool-catalog)
