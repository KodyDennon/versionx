---
title: Qwen
description: Wire Versionx into Qwen-based agents via MCP.
sidebar_position: 5
---

# Qwen

Qwen-based agents (Alibaba Qwen Code, Qwen Coder CLI, derivatives) accept MCP over stdio. The exact config file depends on the specific agent; the shape is universal.

## Configure

```yaml
# qwen-agent config (typical shape)
mcp:
  servers:
    - name: versionx
      command: versionx
      args: ["mcp", "--transport", "stdio"]
```

## Notes

Qwen's tool-selection behavior benefits from explicit hints. Versionx's tool descriptions include recommended usage patterns (e.g., "call `plan` before `apply`") that Qwen picks up on without extra prompting.

## See also

- [MCP server overview](./overview)
- [Tool catalog](./tool-catalog)
