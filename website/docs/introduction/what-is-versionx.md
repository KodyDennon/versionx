---
title: What is Versionx?
description: Versionx unifies runtime/toolchain management, polyglot dependency handling, SemVer release orchestration, multi-repo coordination, policy, and AI-agent integration behind a single progressive-disclosure CLI.
sidebar_position: 1
---

# What is Versionx?

**Versionx** is a cross-platform, cross-language, cross-package-manager version manager and release orchestrator written in Rust. The binary is `versionx`.

It unifies the jobs that today require at least five separate tools:

- **Runtime / toolchain management** — the mise/asdf/proto/nvm/rustup job of pinning language runtimes per repo.
- **Dependency management** — the npm/pip/cargo/bundle/gradle job of resolving and updating dependencies.
- **SemVer release orchestration** — the changesets/release-please/semantic-release job of bumping versions, writing changelogs, and tagging.
- **Multi-repo coordination** — the submodule/subtree/meta/vcstool/gita job of operating across many repos at once.
- **Policy enforcement** — the Renovate/Dependabot/OPA job of writing and enforcing rules across the whole stack.

All of it sits behind one progressive-disclosure CLI that stays dead simple for solo developers and scales to enterprise fleet management.

## The wedge

> **Cross-repo atomic release orchestration with plan/apply safety, polyglot version handling, and AI-as-client architecture.**

No existing tool sits at this intersection. Changesets understands versions but not toolchains. mise understands toolchains but not releases. Renovate understands updates but not orchestration. Versionx is the unified substrate underneath all of those jobs.

## What "progressive disclosure" means in practice

The tool works at five fill-levels of complexity, and graduating between them requires **zero migration**. Each layer is a strict superset of the one below.

1. **Zero-config.** Run `versionx` in a repo with nothing. It detects ecosystems and does the obvious thing.
2. **Config-only.** A `versionx.toml` with a handful of pins.
3. **Config + lockfile.** Reproducible resolved state, like cargo or pnpm.
4. **Config + lockfile + local state DB.** Cross-repo awareness on a single machine — TUI dashboard, cross-repo queries.
5. **Config + lockfile + remote state + policy engine.** Fleet management with enforceable rules.

Higher layers stay invisible until you invoke them. Lower layers never gate higher ones.

## Plan / apply, everywhere

Every mutating operation produces a JSON `Plan` with Blake3-hashed prerequisites and a configurable TTL. A human (or an AI agent) can inspect the plan, approve it, and apply it — with the guarantee that nothing about the world has changed since the plan was produced. The same contract covers dependency updates, release bumps, toolchain installs, and policy changes.

This is what makes AI-as-a-client safe: agents never execute mutations directly; they produce plans that you approve, and you approve them by calling `apply`.

## AI is a client, not a component

Versionx does not bundle an LLM. Instead, it exposes every capability through an [MCP server](/integrations/mcp/overview) (stdio and local HTTP), a [JSON-RPC daemon](/integrations/json-rpc-daemon), and a [local HTTP API](/integrations/http-api). Claude Code, Cursor, Codex, Qwen, Ollama, or any other MCP-aware agent can drive Versionx. For headless use, you configure your own API key (Anthropic, OpenAI, Gemini, or Ollama) — Versionx passes through; it never phones home.

## One binary, no telemetry

- Single static binary for Linux (glibc + musl), macOS (x86_64 + aarch64), Windows (x86_64 + aarch64).
- Distributed via GitHub Releases, Homebrew, Scoop, Cargo, and npm/PyPI shim packages.
- No runtime dependencies except `git` and the ecosystem tools Versionx drives.
- No telemetry. No usage pings. No crash reports. Adoption is measured by GitHub stars and downloads.

See [Install](/get-started/install) to get started, or [Why Versionx compares well](/introduction/how-it-compares) for the landscape.

## See also

- [Status & roadmap](/introduction/status-and-roadmap) — what ships today in 0.7 and what's planned for 1.0.
- [Design principles](/introduction/design-principles) — the load-bearing ideas behind every design decision.
- [Quickstart](/get-started/quickstart) — try it on a real repo in 60 seconds.
