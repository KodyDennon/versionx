---
title: Roadmap
description: Version-sliced roadmap from 0.7 (today) through 1.0 and beyond. Each item has a status badge.
sidebar_position: 100
---

# Roadmap

User-facing, version-sliced. Internal design rationale lives in [`docs/spec/11-version-roadmap.md`](https://github.com/KodyDennon/versionx/blob/main/docs/spec/11-version-roadmap.md).

Status legend:

- **Shipped** — available in the current release.
- **In progress** — landed in `main`, not yet in a tagged release, or actively being built.
- **Planned** — committed to a specific milestone.
- **Considered** — on the list, not committed.

---

## 0.7 (current) — feature-complete

**Shipped:**

- Workspace discovery with zero-config auto-detection.
- Content-hash bump planner with Blake3 prerequisites and TTLs.
- Release engine: plan / approve / apply / rollback for every strategy.
- Policy engine: declarative TOML + sandboxed Luau, waivers with mandatory expiry.
- MCP server: `rmcp`, stdio + local HTTP, ~10 tools.
- BYO-API-key AI overlay: Anthropic / OpenAI / Gemini / Ollama.
- Versiond daemon: JSON-RPC 2.0, file-watch cache invalidation.
- TUI dashboard.
- Cross-repo fleet orchestration with saga protocol.
- 30 crates, 280+ tests.

---

## 0.8 — hardening

**Planned:**

- Windows parity audit across every subsystem. Close every "Unix-only" corner.
- Cross-platform CI matrix expansion (Windows ARM64, Linux musl on ARM64).
- Error-message pass. Every user-facing error ends with an actionable suggestion.
- Doctor command (`versionx doctor`) covers every known failure mode.
- Performance pass on `versionx status` for large monorepos (500+ packages).

**Considered:**

- Per-package task caching (scaffolding for the local cache landing in 1.2).

---

## 0.9 — ecosystem breadth

**Planned:**

- Go adapter (Tier 2 → Stable).
- Ruby adapter (Tier 2 → Stable).
- Go runtime installer.
- Ruby runtime installer (rv primary, ruby-build fallback).

**Considered:**

- Bundler (Ruby) migration from `Gemfile.lock`.

---

## 1.0 — stable baseline

**Planned:**

- Every subsystem tagged Stable (no `Experimental` badges on Reference pages).
- Reusable GitHub Actions published to a stable org.
- Cross-repo saga tested end-to-end by an outside user on ≥20 repos.
- docs.rs API reference complete for the whole SDK surface.
- API and config stability guarantees documented.

**Target:** late 2026. No committed date.

---

## 1.1

**Planned:**

- OCI adapter (container-image-based artifacts).
- JVM runtime installer (Temurin default, foojay Disco API).
- Versioned docs on this site (snapshot 1.0 and 1.1).

---

## 1.2

**Planned:**

- JVM adapter (Maven / Gradle).
- Local task cache in `versionx-tasks`.
- Remote state backend (Postgres).
- Optional bearer-auth for the HTTP API.

**Considered:**

- Hosted GitHub App (SaaS). Still a business question.

---

## 2.0

**Planned:**

- Remote task cache.
- Large-fleet optimizations (multi-user shared daemon, systemd/launchd units).

**Considered:**

- Extension SDK for third-party adapters / policies / release strategies as first-class out-of-tree plugins.

---

## Explicitly deferred

- **Telemetry of any kind.** Never.
- **Bundled LLM.** Never. BYO API key or run your own Ollama.
- **Replacing a resolver.** We drive real package managers; we don't reimplement them.

---

## How this page evolves

The roadmap updates on release PRs. Badges move from **Planned** to **In progress** to **Shipped** as work lands. If you care about a specific item and want to know what's blocking it, [open a discussion](https://github.com/KodyDennon/versionx/discussions).

## See also

- [Status & roadmap summary](/introduction/status-and-roadmap)
- [`docs/spec/11-version-roadmap.md`](https://github.com/KodyDennon/versionx/blob/main/docs/spec/11-version-roadmap.md) — the authoritative internal doc.
