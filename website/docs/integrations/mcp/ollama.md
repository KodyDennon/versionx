---
title: Ollama
description: Use a local Ollama model as the driver for Versionx MCP tools.
sidebar_position: 6
---

# Ollama

Ollama is a local-only model runner. MCP support depends on the client you pair it with — the Ollama model itself doesn't speak MCP, but agent wrappers like [LangGraph](https://langchain-ai.github.io/langgraph/), [Continue.dev](https://continue.dev/), or a custom script can bridge Ollama to Versionx's MCP server.

## Two ways to use Ollama with Versionx

### 1. BYO-API-key mode (no MCP)

Point the AI-assisted surfaces at your local Ollama endpoint:

```bash
export OLLAMA_BASE_URL=http://127.0.0.1:11434
export VERSIONX_AI_PROVIDER=ollama
versionx changelog draft
```

This uses Ollama for one-shot LLM calls without any MCP dance. Simplest path for local-only use.

### 2. As the driver for MCP (via an agent wrapper)

Any MCP-aware agent that supports configurable LLM backends can combine Ollama and Versionx. Examples:

- **Continue.dev.** Configure Versionx as an MCP server and point Continue's model settings at Ollama. Tool-calling-capable Ollama models (e.g., `qwen2.5-coder`, `llama3.3`) work best.
- **Custom script.** The Versionx MCP server runs as a subprocess; your script reads Ollama-generated tool calls and forwards them. The `rmcp` SDK on PyPI and npm can handle the client side.

## Local + private

Neither path sends data anywhere beyond your machine. Ollama is local; Versionx never phones home; the MCP server is stdio-only by default.

## See also

- [MCP server overview](./overview)
- [Environment variables](/reference/environment-variables) — `OLLAMA_BASE_URL`, `VERSIONX_AI_PROVIDER`.
