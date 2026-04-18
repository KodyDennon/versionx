# Versionx — Full Vision Specification

> **Versionx** (binary: `versionx`) is a cross-platform, cross-language, cross-package-manager version manager written in Rust. It unifies runtime/toolchain management (asdf/mise), dependency management (npm/pip/cargo/etc.), SemVer release orchestration (changesets/release-please), multi-repo coordination (submodules/subtrees/virtual monorepos), policy enforcement, and first-class AI-agent integration via MCP — all behind a single progressive-disclosure interface that stays dead simple for solo devs and scales to enterprise fleet management.

The wedge: **cross-repo atomic release orchestration with plan/apply safety, multi-ecosystem support, and AI-as-client architecture.** No existing tool sits at this intersection.

---

## What this spec set contains

This is a **full vision spec**. It describes the end-state product. MVP cuts and phased delivery are called out in `10-mvp-and-roadmap.md`.

The spec is designed to be **agent-ready**: each file is self-contained, references others by filename, and is written so an AI coding agent (or a human) can pick up any piece and start building without needing the rest of the context window.

| # | File | Scope |
|---|------|-------|
| 00 | `00-README.md` | This file — index, north star, design principles |
| 01 | `01-architecture-overview.md` | Crate layout, process model, data flow, transport surfaces |
| 02 | `02-config-and-state-model.md` | `versionx.toml`, lockfile, state DB schema, progressive disclosure layers |
| 03 | `03-ecosystem-adapters.md` | Shell-out adapter interface, tier-1/2/3 plan, per-ecosystem contracts |
| 04 | `04-runtime-toolchain-mgmt.md` | mise/asdf replacement: installers, shims, per-repo pinning |
| 05 | `05-release-orchestration.md` | SemVer, PR-title parsing, changesets, AI-assisted bumps via MCP |
| 06 | `06-multi-repo-and-monorepo.md` | Submodule/subtree/virtual-monorepo support, cross-repo releases |
| 07 | `07-policy-engine.md` | Policy DSL (declarative + Luau), evaluation model, fleet rules |
| 08 | `08-github-integration.md` | Portable CLI + official Actions; hosted GitHub App deferred past v1.0 |
| 09 | `09-programmatic-and-ai-api.md` | Rust SDK, JSON-RPC, HTTP, MCP server (rmcp-based), agent contracts |
| 10 | `10-mvp-and-roadmap.md` | Phased delivery plan and defensibility analysis |
| 11 | `11-version-roadmap.md` | Concrete 0.1 → 1.0 → 1.4 release plan with per-version scope, gates, and demos |

---

## North star

> **A solo dev on one repo and a platform team managing 400 repos should use the same tool, feel native to both, and never hit a wall where the tool says "you've outgrown me."**

Every design decision in this spec traces back to that sentence.

---

## Design principles

These are load-bearing. When in doubt, an implementer should re-read these and choose the option that honors more of them.

### 1. Progressive disclosure is the product
The tool must work at five distinct fill-levels of complexity, and graduating between them must require **zero migration**. Each layer is a strict superset of the one below.

1. **Zero-config.** Run `versionx` in a repo with nothing. It detects ecosystems and does the obvious thing.
2. **Config-only.** A `versionx.toml` with a handful of pins.
3. **Config + lockfile.** Reproducible resolved state, like cargo/pnpm.
4. **Config + lockfile + local state DB.** Cross-repo awareness on a single machine (TUI dashboard, cross-repo queries).
5. **Config + lockfile + remote state + policy engine.** Fleet management, enforceable rules.

Higher layers are invisible until invoked. Lower layers never gate higher ones.

### 2. The core is a library; everything else is a frontend
`versionx-core` is a Rust crate with a stable public API. The CLI, TUI, daemon, web UI, and MCP server are all *frontends* over it. This means:
- Every capability is callable programmatically before it's exposed as a command.
- AI agents are first-class clients, not an afterthought.
- Open-core monetization later (if ever) is a clean split at the crate boundary.

### 3. Shell out, don't reimplement (mostly)
Package-manager integration works by driving the real tools (`npm`, `pip`, `cargo`, `go`, `bundle`, `mvn`, `gradle`) through a uniform adapter trait. We do not reimplement resolvers. We **do** own:
- Runtime/toolchain installation (the mise/asdf piece — because existing tools leak state badly).
- Package-manager version management (pnpm, yarn, uv directly — corepack is being removed from Node 25+).
- The lockfile aggregator (a single `versionx.lock` that references native lockfiles by hash).
- The release/policy/orchestration layers (the value-add).
- A native task runner (phased: v1.0 topo exec, v1.2 local cache, v2.0 remote cache).

### 4. Git is the source of truth, state DB is the cache
Nothing in the state DB may be load-bearing for correctness. If a user deletes the state DB, the next `versionx sync` rebuilds it from git + config + lockfile. This is non-negotiable — it's what keeps "simple" and "complex" on the same substrate.

### 5. AI is a client, not a component
Versionx ships no bundled LLM. Instead, the MCP server (stdio + local HTTP) exposes every capability so Claude Code, Codex, Cursor, Qwen, or any other agent can drive it. For headless use, a BYO-API-key path lets users configure Anthropic/OpenAI/Gemini/Ollama directly. Every mutating operation supports `--plan` (emit the plan as JSON, don't execute) and `--apply <plan.json>` (execute a pre-approved plan with Blake3-hashed prerequisites and a configurable TTL). This is how humans and agents share safe workflows.

### 6. Portable first, GitHub-deep second
The single binary must run identically on a dev laptop, in GitHub Actions, in GitLab CI, and in a Jenkins agent. v1.0 ships reusable GitHub Actions for the common flows; a hosted GitHub App is **deferred past v1.0** so we don't carry SaaS infrastructure burden while establishing the CLI.

### 7. Rust, single static binary, cross-platform
Linux (glibc + musl), macOS (x86_64 + aarch64), Windows (x86_64 + aarch64). One binary. No runtime dependencies except git and the ecosystem tools it drives. Distribution via GitHub Releases, Homebrew, Scoop, Cargo, npm/PyPI shim packages, and a `curl | sh` installer.

### 8. Observable by default, no telemetry ever
Every operation emits structured events (tracing + OTLP export on user's endpoint). The TUI, web UI, and CI logs all consume the same event stream. Debugging a weird `versionx` run should never require guessing. **Versionx never phones home.** No usage pings, no crash telemetry, no "help us improve" dialogs. Users learn adoption via GitHub stars and downloads.

---

## What Versionx is NOT

Keeping the boundary sharp:

- **Not a package registry.** It doesn't host packages. It drives the tools that talk to registries.
- **Not an LLM provider.** It doesn't host or bundle models. It serves AI agents via MCP and optionally calls user-configured LLM APIs.
- **Not a git replacement.** It uses git. It doesn't reimplement git operations beyond what's needed to coordinate repos.
- **Not a CI service.** It runs inside your CI of choice. It doesn't compete with GitHub Actions / CircleCI / etc.
- **Not a secrets manager.** It reads tokens from env/keychain. It doesn't store secrets.
- **Not yet a SaaS.** v1.0 is OSS CLI + Actions only. Hosted GitHub App is a post-v1 question.

---

## Competitive landscape (why this exists)

| Problem | Existing tools | Versionx's angle |
|---|---|---|
| Runtime pinning | asdf, mise, proto, nvm, pyenv, rustup | Single tool, faster (Rust), native shims, integrates with everything else |
| Dependency mgmt | npm/pip/cargo/etc. directly | Unifies status/update/audit across ecosystems; doesn't replace the resolvers |
| Release automation | changesets, release-please, semantic-release | PR-title default, multi-ecosystem, multi-repo, AI-assisted via MCP |
| Monorepo mgmt | Nx, Turborepo, Lerna, moon | Language-agnostic; shells out to ecosystem tools; adds policy + cross-repo; native runner phased |
| Multi-repo coord | Meta, gita, vcstool | Adds state DB, policy, atomic release coordination |
| Policy enforcement | Renovate, Dependabot, OPA | Unified DSL (declarative TOML + Luau sandbox) across the whole stack |
| AI integration | None at this layer | First-class MCP server (rmcp); every command has JSON IO; plan/apply safety model |

No existing tool sits at this intersection. That's the bet.

---

## Naming note

The project is **Versionx**. The binary is **`versionx`** — `vx` was ruled out after verification: `vx` is taken on crates.io by an active tool in the same space ("Universal Development Tool Manager"), squatted on npm, and taken on PyPI. `versionx` is free on crates.io, npm, PyPI, and Homebrew. See `10-mvp-and-roadmap.md §6` for the full naming rationale. Users who want a short form can alias locally.

---

## How to read this spec

- **If you're building**: start at `01-architecture-overview.md`, then jump to the specific subsystem file.
- **If you're evaluating the vision**: read this file + `10-mvp-and-roadmap.md`.
- **If you're an AI agent picking up a task**: every file has a "Scope" and "Contract" section at the top. Read those first.
