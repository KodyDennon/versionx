# 10 — MVP Cut & Roadmap

## Scope
What ships first, what ships later, and why. This file is the decision, with the rationale.

---

## 1. The defensibility question

If Versionx has to pick one "killer feature" to be known for at v1.0, which one creates a wedge no existing tool has?

**Answer: cross-repo atomic release orchestration with plan/apply safety, multi-ecosystem support, and AI-as-client architecture.**

Here's the reasoning:

| Candidate killer feature | Why it's *not* the wedge |
|---|---|
| Runtime mgmt (mise replacement) | mise + proto are excellent; the wedge here is incremental. |
| Unified package-mgr interface | Useful, but users mostly stay in one ecosystem per repo. |
| Policy engine | Valuable, but OPA/Conftest/Renovate cover a lot. |
| GitHub App | Dependabot/Renovate own this space (and we've deferred it). |
| Task runner | moon + turbo + nx own this; we ship phased, not as the wedge. |
| **Cross-repo atomic release + plan/apply safety + polyglot + AI-as-client** | **Nothing else sits at this intersection.** |

The wedge is: *"You describe your releases in PR titles or changesets; an AI agent (Claude Code, Codex, etc.) proposes bumps with rationale and voice-aware changelogs via MCP; policies gate it; humans approve once; and it executes saga-atomically across Node + Python + Rust + your fleet."*

That's the story v1.0 tells. Everything else exists to support it.

---

## 2. v1.0 — the defensible MVP

### 2.1 Ships in v1.0

**Identity & distribution:**
- ✅ Project name: **Versionx**. Binary: **`versionx`**. Config: **`versionx.toml`**.
- ✅ License: **Apache 2.0**.
- ✅ Distribution: GitHub Releases (cargo-dist), curl|sh installer, Homebrew, Scoop/winget, Cargo, npm shim (`npm i -g versionx`), PyPI shim (`pip install versionx`).
- ✅ Code-signed releases (EV cert via Azure Key Vault + AzureSignTool for Windows; `rcodesign` for macOS notarization).
- ✅ **Zero telemetry.** Never phone home.

**Core infrastructure:**
- ✅ `versionx-core`, SDK, CLI, daemon, JSON-RPC, HTTP, MCP (all surfaces, loopback only).
- ✅ Config model (L1–L3: zero-config, config, lockfile).
- ✅ Local SQLite state DB via rusqlite + WAL (L4).
- ✅ Event bus, structured output, observability foundations.
- ✅ **XDG-compliant filesystem layout** on Linux; platform-appropriate paths on macOS/Windows.
- ✅ **Always-on daemon via shell hook** (`eval "$(versionx activate bash)"`); UDS 0600 local-only security.
- ✅ `versionx self update` command + daily auto-check notification (no auto-install).
- ✅ `.env` + `.env.local` auto-loading; `[vars]` block override.
- ✅ **Workspace root detection**: walk up for `versionx.toml` with `workspace = true`, then any `versionx.toml`, then git root.

**Ecosystems (Tier 1 at launch — all three in parallel):**
- ✅ Node (npm, pnpm, yarn) — versionx manages pnpm/yarn directly (no corepack).
- ✅ Python (uv, poetry, pip) — venvs delegated to uv/poetry.
- ✅ Rust (cargo).

This is bigger than the typical "Phase 1" (single ecosystem). Justified: the wedge is polyglot cross-repo release; one ecosystem can't demonstrate it.

**Runtime management:**
- ✅ Node, Python, Rust toolchain install + shims.
- ✅ **python-build-standalone** with sysconfig patching + Windows `pip.exe` generation.
- ✅ **Volta-style Windows trampoline** (~200KB static exe, argv[0] dispatch, mmap'd PATH cache).
- ✅ pnpm, yarn, uv, poetry as managed package-manager runtimes.
- ✅ Global install: `versionx install node 20` works without a repo.
- ✅ `.tool-versions` / `.mise.toml` / `.nvmrc` / `rust-toolchain.toml` read compat.

**Release orchestration (the wedge):**
- ✅ **PR-title strategy as default** (new; no existing tool defaults to this).
- ✅ Conventional Commits strategy.
- ✅ Changesets strategy.
- ✅ Manual strategy.
- ✅ AI-assist overlay via MCP (no bundled model) + BYO-API-key headless mode.
- ✅ **Voice-aware changelog** (README + past CHANGELOG + voice samples fed as context; per-package style memory in state DB).
- ✅ Plan/apply model with Blake3 IDs, `pre_requisite_hash`, configurable TTL (1h–30d, 24h default).
- ✅ Single-repo releases including monorepo with per-package versioning.
- ✅ Publishing to npm, PyPI, crates.io — **OIDC trusted publishing preferred**, token fallback with warnings.
- ✅ Interactive push prompt in TTY; `--push`/`--no-push` required in CI.
- ✅ Cross-ecosystem version translation (semver ↔ PEP 440 etc.).

**Cross-repo (the killer feature):**
- ✅ Read existing submodules and subtrees; report their state.
- ✅ Virtual monorepo via `versionx-fleet.toml` in dedicated ops repo — query, parallel sync.
- ✅ Cross-repo release coordination in `independent`, `gated`, and `coordinated` modes.
- ✅ **Saga pattern with interactive rollback** for `coordinated` mode — documented as "best-effort atomic, not truly atomic."

**Policy:**
- ✅ Declarative rules (runtime_version, dependency_version, dependency_presence, lockfile_integrity, release_gate, commit_format, link_freshness, provenance_required, advisory_block).
- ✅ **Luau scripting escape hatch** (mlua, sandboxed — not Lua 5.4).
- ✅ Severity, scope, triggers, **mandatory waiver expiry** (Snyk-style, org can override).
- ✅ `versionx.policy.lock` for fleet-inherited policy pinning.

**GitHub / CI:**
- ✅ Portable CLI runs in GH Actions, GitLab, CircleCI, Jenkins.
- ✅ Official GitHub Actions: install, sync, policy-check, release-propose, release-apply, pr-title-validate.
- ❌ **Hosted GitHub App deferred past v1.0.** Check runs and PR comments posted from within workflows using `GITHUB_TOKEN` instead.

**AI / MCP:**
- ✅ MCP server built on official `rmcp` Rust SDK (≥1.5).
- ✅ **stdio + loopback HTTP transports** (no auth, no OAuth in v1.0).
- ✅ **≤10 tools**, workflow-shaped (plan/apply pairs).
- ✅ Read-only resources mirrored as tools (for clients like Cursor that don't surface resources).
- ✅ Three preloaded prompts: `propose_release`, `audit_dependency_freshness`, `remediate_policy_violation`.
- ✅ Tool output sanitization (prompt-injection defenses).

**TUI and web UI:**
- ✅ TUI: dashboard + repo detail + release planner.
- ⚠️ Web UI: minimal — dashboard only (policy detail, release history). Full web orchestration UI deferred to v1.2.

**Task runner (phased):**
- ✅ **v1.0**: native `[tasks]` with topological dependency ordering + parallel exec. No cache. Competes with just/make at launch, reads turbo.json/moon.yml/Makefile task defs.
- 🔜 **v1.2**: content-addressed local cache (inputs/outputs).
- 🔜 **v2.0**: remote cache (S3/R2) + sandboxing.

### 2.2 Out of scope for v1.0

Explicitly deferred, in planned order:

- Go, Ruby, OCI adapters (Tier 2) → **v1.1**.
- Maven/Gradle / JVM adapter → **v1.2**.
- Tier 3 community adapters (.NET, PHP, Dart, Elixir, Swift) → **v2.0+** (community-driven).
- Scheduled workflows (dep-sync PRs, submodule update PRs) → **v1.1** (via cron-driven Actions).
- Full web UI with orchestration → **v1.2**.
- Remote state / Postgres backend → **v1.2**.
- **Hosted GitHub App** → **post-v1.0**, demand-driven.
- Remote MCP + OAuth CIMD → **v1.2+**.
- GitLab native integration (MR comments, CI components) → **v1.2**.
- Task runner local cache → **v1.2**.
- Task runner remote cache + sandboxing → **v2.0**.

---

## 3. Why this MVP is defensible

### 3.1 The wedge is demonstrable
A 5-minute demo: mono-repo with Node + Python + Rust packages, PR titles + changesets, AI (via Claude Code MCP) proposes bumps with voice-aware changelog, policy gates, human approves once, `versionx release apply` publishes everything in topological order across ecosystems. **No existing tool does that demo.**

### 3.2 It's a complete loop
Each feature supports the wedge:
- Runtime mgmt → ensures CI and dev have the same tool versions → releases are reproducible.
- Unified adapter → allows polyglot release plans.
- State DB → enables the TUI dashboard + voice-sample memory + audit.
- Policy → makes release gates enforceable.
- Task runner (phased) → lets versionx own the test-before-release path in v1.2+.
- MCP → AI is first-class, not a plugin.
- Reusable Actions → meets GitHub users where they already are, without hosting SaaS.

### 3.3 It's credibly shippable
- Tier 1 ecosystems only: 3 adapters to build, each well-understood.
- Shell-out approach: no reimplementation risk.
- Rust + tokio + rmcp + rusqlite: known-good foundation.
- Incremental phases: each shipping milestone provides value.
- **No hosted services** → no uptime ops burden during v1.0.

### 3.4 It's hard to copy
The wedge requires *all* of these at once: polyglot adapters + plan/apply safety + MCP-first AI + cross-repo saga + policy gate + PR-title intent. Anyone copying has to build the full loop; piecemeal doesn't reproduce the experience. That's the moat.

---

## 4. Phased delivery (revised from original 10-month estimate)

The v1.0 scope expanded — parallel Phase 1 polyglot + task runner + voice-aware AI changelog — pushing timeline to **~13–15 months for a 2-3 person team**, or faster with AI-agent-driven implementation.

### Phase 0 — Foundations (1.5 months)
- Monorepo bootstrap, CI, release process for Versionx itself.
- `versionx-core`, `versionx-cli` skeleton.
- Config + lockfile data model with `toml_edit`.
- Event bus.
- Shim binary (Volta-style Windows trampoline).
- cargo-dist release pipeline, code signing.

### Phase 1 — Polyglot ecosystems (3.5 months, parallel)
- Node adapter (pnpm first, then npm, then yarn).
- Python adapter (uv first, then poetry, then pip).
- Rust adapter (cargo).
- Node + Python + Rust runtime installers (with python-build-standalone sysconfig patching, rv readiness for future Ruby).
- Daemon + IPC + JSON output.
- Shell activation hooks (bash/zsh/fish/PowerShell).
- State DB (rusqlite + WAL).
- `versionx init`, `versionx sync`, `versionx verify`, `versionx install`, `versionx global` working end-to-end.

### Phase 2 — Release engine (2.5 months)
- PR-title parser + Conventional Commits + Changesets strategies.
- Plan/apply model (Blake3, pre-requisite hash, TTL).
- Single-repo monorepo releases across all three ecosystems.
- OIDC trusted publishing for npm/PyPI/crates.io.
- Interactive push + TTY detection.

### Phase 3 — AI + Policy (2 months)
- MCP server (rmcp stdio + HTTP).
- ≤10 tools with `_plan`/`_apply` pairs.
- Voice-aware changelog workflow (MCP + BYO-API-key paths).
- Luau-based policy engine (sandboxed).
- Declarative + custom rules.
- Mandatory waiver expiry.
- Policy-gate release flow.

### Phase 4 — Cross-repo + Actions (2.5 months)
- `versionx-fleet.toml` in ops repo pattern.
- Saga pattern cross-repo release with interactive rollback.
- Official GitHub Actions (install, sync, policy, release-propose, release-apply, pr-title-validate).
- PR comment + check-run posting from within workflows.
- Auto-merge wrapper (GitHub's native auto-merge API).

### Phase 5 — Task runner v1 + Polish (1.5 months)
- Native task runner: `[tasks]` block, topological exec, parallel.
- Reading turbo.json / moon.yml / Makefile task defs.
- TUI dashboard.
- Docs site (mdBook or similar).
- Examples.
- Website (versionx.dev).
- Installer distribution (brew, scoop, curl|sh, releases).
- Public announce.

**Total: ~13–15 months for v1.0.**

### Post-v1.0
- **v1.1 (+3 months)**: Go + Ruby (via rv) + OCI adapters, Tier 2 tooling, scheduled workflows (dep-sync via Actions cron), GitLab CI components.
- **v1.2 (+4 months)**: JVM (via foojay Disco API + Temurin default), full web UI, Postgres remote state, task runner local cache, GitLab MR-comment automation, remote MCP + OAuth CIMD.
- **v2.0 (+6 months)**: task runner remote cache + sandboxing, Tier 3 adapters, **hosted GitHub App** if demand materializes.

---

## 5. Risks and mitigations

| Risk | Mitigation |
|---|---|
| Rust compile times slow contributor velocity | Strict crate splitting, sccache, incremental CI |
| Shell-out to package managers breaks on tool updates | Pin tested version ranges; golden fixtures per tool version; matrix CI |
| Mise/asdf users won't switch for marginal gains | Lead with release/AI wedge, not runtime mgmt; `versionx import --from mise` easy path |
| Parallel Phase 1 polyglot blows up timeline | Single contributor per ecosystem; shared trait + test kit; CI matrix gates merge |
| AI proposals produce bad bumps | Conservative defaults: AI proposes, human always approves; audit trail |
| Voice-aware changelog relies on external LLMs — outage = no prose | Deterministic git-cliff fallback always available; AI is additive |
| Policy engine too complex | Luau sandbox strict; declarative covers 90%; mlua version pinned, audited |
| Cross-repo "atomic" release bugs | Ship with saga rollback; dry-run gate; explicit "best-effort" docs; no pretending |
| State DB corruption | Always rebuildable from git; `versionx state repair`; no load-bearing state |
| Monorepo performance at scale (200+ packages) | Benchmarks as CI gates; parallelism by default; explicit perf budget per op |
| MCP spec evolves | Own `versionx-mcp` crate; abstract over rmcp version; pin spec `2025-06-18` minimum |
| Corepack removal breaks Node users | versionx installs pnpm/yarn directly from day one; signature-verification bugs avoided |
| python-build-standalone gotchas | sysconfig patching + Windows pip.exe shim + `versionx doctor` SSL/terminfo checks |
| Windows shim perf regression | <1ms hot-path benchmark as CI gate |

---

## 6. Naming

Project: **Versionx**. Binary: **`versionx`**. Config: **`versionx.toml`** / **`versionx.lock`** / **`versionx-fleet.toml`**. Env prefix: **`VERSIONX_*`**. Daemon: **`versiond`**.

Rationale for `versionx` as the binary name:
- **Free across all registries**: crates.io, npm, PyPI, Homebrew — none have `versionx` squatted or taken.
- **Short forms are polluted**: `vx` is actively used on crates.io (v0.4.1, "Universal Development Tool Manager" — direct category collision), squatted on npm (abandoned 2015), taken on PyPI (unrelated ML lib). `verx`/`vex`/other 3-char names have similar contested histories.
- **No semantic collision** with the OWASP/PURL "vers" spec for version-range syntax.
- **8 chars is fine**: `kubectl` = 7, `gradle` = 6, `gcloud` = 6, `terraform` = 9. Nobody retyped these.
- Users who prefer a short form can alias locally: `alias vx="versionx"` or `alias v="versionx"`.

---

## 7. Open decisions that don't block MVP but need owners

- **MSRV policy** — propose "stable-1 at time of release" (i.e., rustc N-1).
- **SDK API stability** — propose: `versionx-sdk` 0.x until v1.0 GA, then semver-locked from 1.0.
- **Sigstore signing of versionx's own releases** — yes, configure from day one via cargo-dist.
- **Docs site tech** — mdBook vs Docusaurus; not urgent.
- **Governance** — BDFL for v1.0, revisit post-launch if community grows.
- **Config schema format** — publish JSON Schema for IDE autocomplete at `versionx.dev/schema/v1.json`.

---

## 8. Signals of success for v1.0

- A solo dev with one Node repo uses `versionx init` + `versionx sync` + `versionx release` and never edits a policy file.
- A 3-person startup with a pnpm monorepo uses changesets + MCP-driven AI changelog + reusable Actions and ships weekly releases with one-click approval from a Claude Code session.
- A platform team at a 200-engineer company uses virtual monorepo + policies + waivers with mandatory expiry to enforce standards across 40 repos; sees violation counts go to zero over 90 days, and catches the one policy-update-breaks-CI incident via `versionx.policy.lock`.
- An AI agent (Claude Code, Codex, Cursor) uses the MCP server to propose a cross-repo release, hits a policy wall, suggests a waiver, opens a PR with the waiver, gets human approval, retries the release saga — without the human ever touching `versionx.toml` directly.
- A platform engineer migrates from mise + changesets + bespoke release bash + Renovate config to `versionx` and reports the cross-repo story replaces 3 separate tools.

If those five personas all land, the wedge is real.

---

## 9. First thing to build

Not a crate. Not the architecture. Write **the 5-minute demo script** first — the exact `versionx` commands that will make a polyglot monorepo cross-repo release with AI-proposed bumps, voice-aware changelogs, and policy gating. That's the north star for every PR. Ship something that makes that demo possible end-to-end, even if it's ugly, then polish.

If the demo doesn't move people, the spec is wrong somewhere. Find out early.
