---
title: Roadmap
description: Honest roadmap from the current 0.1 alpha through 1.0.
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

## 0.1 (current) — public alpha foundation

**Shipped:**

- Workspace discovery with zero-config auto-detection.
- `versionx init` config generation.
- Release engine: plan/propose, approve, apply, rollback, snapshot, prerelease.
- Policy engine: declarative TOML + sandboxed Luau, waivers with mandatory expiry.
- MCP server: `rmcp`, stdio + local HTTP, ~10 tools.
- BYO-API-key AI overlay: Anthropic / OpenAI / Gemini / Ollama.
- Versiond daemon: JSON-RPC 2.0, file-watch cache invalidation.
- Cross-repo fleet orchestration with saga protocol.
- 30 crates, 280+ tests.

---

## Next alpha steps — hardening and honesty

**Planned:**

- Make the docs/site match the shipped alpha surface everywhere.
- Improve first-run UX around `init`, shell-hook install, and doctor output.
- Harden the single-repo release flow for outside users.
- Publish and verify broader install channels only after they are actually live.
- Windows parity audit across every subsystem. Close every "Unix-only" corner.

---

## 1.0 — stable baseline

**Planned:**

- Core CLI and docs surfaces stable enough to recommend broadly.
- Broader package distribution channels live and verified.
- Dependency-update workflows land as real commands.
- Published automation surfaces that are actually supported, not just envisioned.
- API and config stability guarantees documented.

**Target:** late 2026. No committed date.

---

## Later milestones

- Additional ecosystem breadth (Go/Ruby/JVM and beyond)
- Better fleet ergonomics
- Versioned docs
- Optional remote backends and auth once the local-first story is solid

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
