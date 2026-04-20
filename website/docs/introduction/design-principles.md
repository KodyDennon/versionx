---
title: Design principles
description: Eight load-bearing principles that shape every design decision in Versionx.
sidebar_position: 4
---

# Design principles

These principles are load-bearing. When a design decision is in tension, the option that honors more of these wins.

## 1. Progressive disclosure is the product

The tool must work at five distinct fill-levels of complexity, and graduating between them must require **zero migration**. Each layer is a strict superset of the one below.

1. **Zero-config.** Run `versionx` in a repo with nothing. It detects ecosystems and does the obvious thing.
2. **Config-only.** A `versionx.toml` with a handful of pins.
3. **Config + lockfile.** Reproducible resolved state, like cargo or pnpm.
4. **Config + lockfile + local state DB.** Cross-repo awareness on one machine.
5. **Config + lockfile + remote state + policy engine.** Fleet management.

Higher layers stay invisible until invoked. Lower layers never gate higher ones.

## 2. The core is a library; everything else is a frontend

`versionx-core` is a Rust crate with a stable public API. The CLI, TUI, daemon, web UI, and MCP server are all *frontends* over it. This means:

- Every capability is callable programmatically before it's exposed as a command.
- AI agents are first-class clients, not an afterthought.
- Open-core monetization later (if ever) is a clean split at the crate boundary.

See [Architecture](/contributing/architecture) for the full boundary rules.

## 3. Shell out, don't reimplement (mostly)

Package-manager integration works by driving the real tools (`npm`, `pip`, `cargo`, `go`, `bundle`, `mvn`, `gradle`) through a uniform adapter trait. We do not reimplement resolvers.

We **do** own:

- Runtime / toolchain installation (the mise/asdf piece — existing tools leak state badly).
- Package-manager version management (pnpm, yarn, uv directly — corepack is being removed from Node 25+).
- The lockfile aggregator (a single `versionx.lock` referencing native lockfiles by hash).
- The release / policy / orchestration layers (the value-add).
- A native task runner (phased: 1.0 topo exec, 1.2 local cache, 2.0 remote cache).

## 4. Git is the source of truth; the state DB is a cache

Nothing in the state DB may be load-bearing for correctness. If you delete `state.db`, the next `versionx sync` rebuilds it from git plus config plus lockfile. This is non-negotiable — it keeps "simple" and "complex" on the same substrate.

## 5. AI is a client, not a component

Versionx ships no bundled LLM. The [MCP server](/integrations/mcp/overview) (stdio + local HTTP) exposes every capability so Claude Code, Cursor, Codex, Qwen, Ollama, or any other agent can drive Versionx.

For headless use, a BYO-API-key path lets you configure Anthropic, OpenAI, Gemini, or Ollama directly.

Every mutating operation supports `--plan` (emit the plan as JSON, don't execute) and `--apply <plan.json>` (execute a pre-approved plan with Blake3-hashed prerequisites and a configurable TTL). This is how humans and agents share safe workflows.

## 6. Portable first, GitHub-deep second

The single binary must run identically on a dev laptop, in GitHub Actions, in GitLab CI, and in a Jenkins agent. v1.0 ships reusable GitHub Actions for the common flows. A hosted GitHub App is **deferred past v1.0** so we don't carry SaaS infrastructure burden while establishing the CLI.

## 7. Rust, single static binary, cross-platform

- Linux (glibc + musl), macOS (x86_64 + aarch64), Windows (x86_64 + aarch64).
- One binary. No runtime dependencies except `git` and the ecosystem tools it drives.
- Distribution via GitHub Releases, Homebrew, Scoop, Cargo, npm/PyPI shim packages, and a `curl | sh` installer.

## 8. Observable by default — no telemetry ever

Every operation emits structured events (tracing + OTLP export on **your** endpoint). The TUI, web UI, and CI logs all consume the same event stream. Debugging a weird `versionx` run should never require guessing.

**Versionx never phones home.**

- No usage pings.
- No crash telemetry.
- No "help us improve" dialogs.

Adoption is measured by GitHub stars and downloads.

## See also

- [Architecture](/contributing/architecture) — how the core/frontend split is enforced.
- [Plan / apply cookbook](/sdk/plan-apply-cookbook) — principle 5 in practice.
- [Events reference](/reference/events) — principle 8 in practice.
