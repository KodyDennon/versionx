# Developer documentation — design

**Date:** 2026-04-20
**Status:** Design approved; ready for implementation plan
**Author:** Kody + Claude (brainstorm)
**Scope:** Produce a user-facing documentation site and an expanded README that serve three audiences — end users, integrators, and contributors — without disrupting the existing `docs/spec/` vision specification.

---

## 1. Goals & non-goals

### Goals

1. Ship a documentation site that is **complete at launch** for the 0.7 feature set.
2. Cover three distinct audiences without forcing any of them to read past irrelevant content:
   - **End users** — people running `versionx` on their own repos.
   - **Integrators** — people embedding the SDK or driving Versionx from an AI agent / CI / their own tooling.
   - **Contributors** — people landing PRs on the project.
3. Replace the current README with a tight landing-and-funnel variant that directs deeper questions to the site.
4. Preserve `docs/spec/` as the authoritative internal design / RFC surface — do not migrate it.
5. Prevent documentation drift through auto-generation where the source of truth lives in code.

### Non-goals

- Building a SaaS-grade marketing site. No analytics, no lead capture, no newsletter. Respects the "no telemetry ever" principle.
- Versioned documentation pre-1.0. Docs track `main`; snapshotting begins at 1.0.
- Custom domain. `kodydennon.github.io/versionx` at launch; move to `versionx.dev` (or similar) post-1.0 if desired.
- Reimplementing what docs.rs already provides. Full rustdoc API stays on docs.rs; the site links out.
- PR preview deployments. Could be added later; not in v1.
- Migrating `docs/spec/` content. It stays where it is. The site links to it from the Contributing section.

---

## 2. Core decisions (locked)

| Decision | Choice | Rationale |
|---|---|---|
| Site generator | Docusaurus (Classic preset, TypeScript, MDX v3) | Future-proof, versioned docs ready, Algolia DocSearch, long track record |
| Location in repo | `website/` at repo root | Docusaurus convention; isolates Node toolchain |
| Relationship to `docs/spec/` | **Split.** Spec stays as internal design/RFC territory. Site is user-facing only. Site links to spec where relevant. | Avoids disruptive content migration; keeps agents happy with plain markdown; two surfaces with distinct purposes |
| Honesty rule | **Shipped + roadmap pages.** Reference pages document 0.7 reality. A dedicated Roadmap section mirrors `docs/spec/11` for users. No aspirational examples in how-to pages. | Users can trust the docs reflect current behavior |
| v1 scope | **Complete before launch** — full tutorial, every adapter/runtime page, SDK cookbook, migration guides, every CLI command. | User-stated preference; worth the one-time lift |
| README shape | Landing + funnel, ~150 lines, marketing quality | Better GitHub first impression; site carries depth |
| Hosting | GitHub Pages on `kodydennon.github.io/versionx` | Zero infra cost; easy custom-domain migration later |
| Versioning | Unversioned until 1.0; snapshot per major after | Avoids churn of pre-1.0 snapshots |
| Search | Algolia DocSearch (applied for pre-launch) + local search plugin fallback | DocSearch is free for OSS; local plugin ensures day-one search |
| License on site content | Apache-2.0 (matches repo) | Consistency |

---

## 3. Information architecture

### Top-level sections (Docusaurus sidebar)

```
Home (custom MDX landing)
├── Introduction
│   ├── What is Versionx
│   ├── How it compares         (vs mise, asdf, changesets, release-please, Nx)
│   ├── Status & roadmap
│   └── Design principles
│
├── Get Started
│   ├── Install
│   ├── Quickstart              (zero-config bare run)
│   ├── Your first release      (end-to-end, one repo)
│   └── Migrating from…         (mise, asdf, changesets, release-please)
│
├── Guides                      (task-oriented)
│   ├── Managing toolchains
│   ├── Polyglot dependency updates
│   ├── Orchestrating a release
│   ├── Multi-repo & monorepos
│   ├── Policy & waivers
│   ├── The daemon & TUI
│   └── CI: GitHub Actions recipes
│
├── Reference                   (exhaustive, authoritative)
│   ├── CLI                     (auto-gen from clap)
│   ├── versionx.toml           (auto-gen from versionx-config)
│   ├── versionx.lock           (format spec)
│   ├── State DB schema
│   ├── Events & tracing        (auto-gen catalog)
│   ├── Exit codes              (auto-gen)
│   └── Environment variables
│
├── Integrations
│   ├── MCP server              (per-agent: Claude Code, Cursor, Codex, Qwen, Ollama)
│   ├── JSON-RPC daemon         (versiond protocol)
│   ├── HTTP API                (Scalar OpenAPI view)
│   └── GitHub Actions
│
├── SDK (Rust)
│   ├── Overview
│   ├── Embedding versionx-core
│   ├── Plan/apply cookbook
│   ├── Custom adapters
│   └── Link to docs.rs
│
├── Contributing
│   ├── Dev environment setup
│   ├── Workspace tour
│   ├── Architecture deep-dive  (pointer to docs/spec/01)
│   ├── Adding a package-manager adapter
│   ├── Adding a runtime installer
│   ├── Writing tests
│   ├── Debugging & tracing
│   ├── Release engineering
│   └── RFCs & design docs      (pointer to docs/spec/)
│
└── Roadmap
```

### Cross-cutting page conventions

- **Status badge** in frontmatter on every Reference page: `Stable` / `Experimental` / `Planned`. Default for shipped 0.7 features is `Stable` unless the spec or release notes explicitly mark them experimental. `Planned` pages must not appear in v1 except on the Roadmap; if listed elsewhere, a banner links to the tracking issue and roadmap entry.
- **"You'll learn"** (3 bullets) and **"Prerequisites"** at the top of every Guide.
- **"Since"** version on every CLI flag, config key, event, and SDK item in Reference.
- **"See also"** at the bottom of every page (2-4 internal links).
- **"Edit this page"** footer link (Docusaurus built-in) to the source file on GitHub.
- Code blocks always have a language tag; command examples use `console` with `$` prompts.
- Admonitions (`:::note / :::tip / :::warning / :::danger`) used liberally on release/policy pages for known gotchas.

---

## 4. Content inventory

Each bullet is a page that must be **non-stub** at launch.

### Introduction (shared across audiences)

- What is Versionx (the pitch, 60s version, positioning)
- How it compares (feature matrix vs mise, asdf, changesets, release-please, Nx)
- Status & roadmap (summary; links to the Roadmap section)
- Design principles (distilled from `docs/spec/00`, load-bearing principles only)

### End users

- Install (macOS/Linux curl, Windows PS, Homebrew, Scoop, Cargo, npm shim, PyPI shim, manual binary, verification)
- Quickstart — bare `versionx` run in an existing repo; walk through auto-detection; the suggestions UI
- Your first release — init repo, add `versionx.toml`, make a change, `release plan`, approve, apply
- Migrating from mise
- Migrating from asdf
- Migrating from changesets
- Migrating from release-please
- Guide: managing toolchains
- Guide: polyglot dependency updates
- Guide: orchestrating a release
- Guide: multi-repo & monorepos
- Guide: policy & waivers
- Guide: the daemon & TUI
- Guide: GitHub Actions recipes
- Reference: CLI (auto-gen)
- Reference: `versionx.toml` (auto-gen)
- Reference: `versionx.lock`
- Reference: State DB schema
- Reference: Events & tracing (auto-gen)
- Reference: Exit codes (auto-gen)
- Reference: Environment variables

### Integrators

- MCP server overview + per-agent pages for Claude Code, Cursor, Codex, Qwen, Ollama; stdio vs HTTP; tool catalog (auto-gen from rmcp registrations)
- JSON-RPC daemon — versiond protocol, message shapes, authentication, lifecycle
- HTTP API — Scalar-rendered OpenAPI from `aide`; local-only binding rationale
- GitHub Actions — inputs/outputs of each reusable action, composition patterns
- SDK overview (embed vs shell out)
- Embedding `versionx-core`
- Plan/apply cookbook (TTL, prerequisite Blake3 hashing, JSON round-trip)
- Custom adapters — `PackageManagerAdapter` trait, tier guidelines, worked example
- Pointer page to docs.rs

### Contributors

- Dev environment setup (toolchain pin, `cargo xtask ci`, cargo-deny, cargo-nextest, IDE tips)
- Workspace tour (one short page per crate: purpose, key types, dependents, where to start reading)
- Architecture deep-dive (summary + pointer to `docs/spec/01`)
- Adding a package-manager adapter (worked example, testing pattern)
- Adding a runtime installer (verification, shim wiring, platform matrix)
- Writing tests (unit / property / snapshot / integration tiers; when each applies)
- Debugging & tracing (`RUST_LOG`, OTLP, `--plan` for inspection, common traps)
- Release engineering (how Versionx releases itself)
- RFCs & design docs — "big changes start in `docs/spec/`" — process

### Roadmap

- One page sliced by version (0.8 → 1.0 → 1.x) — echoes `docs/spec/11` but written for users, with Status badges per item.

---

## 5. Auto-generation pipeline

Hand-written reference rots. Where the source of truth lives in code, generate.

| Page | Source of truth | Generator |
|---|---|---|
| CLI reference | `clap` derive attrs in `versionx-cli` | `cargo xtask docs-cli` → per-command markdown |
| `versionx.toml` reference | `versionx-config` types + serde attrs | `cargo xtask docs-config` → schema walker output |
| Events catalog | `versionx-events` enum variants + doc comments | `cargo xtask docs-events` |
| MCP tool catalog | `versionx-mcp` rmcp registrations | `cargo xtask docs-mcp` |
| JSON-RPC methods | `versionx-daemon` handler registry | `cargo xtask docs-rpc` |
| HTTP API | `aide` OpenAPI output | Scalar view embedded in a Docusaurus page |
| Exit codes | `versionx-core` error taxonomy | `cargo xtask docs-exit-codes` |

### Rules

- Generators emit to `website/docs/reference/<area>/generated-*.md`.
- Every generated file starts with `<!-- GENERATED by \`cargo xtask docs-<name>\`. Do not edit. -->`.
- `cargo xtask docs` is an aggregate task that runs all generators.
- CI gate: `cargo xtask docs` followed by `git diff --exit-code -- website/` — drift fails the build.
- SDK (full rustdoc) is **not** regenerated; the site links to docs.rs.

---

## 6. README restructure

**Target:** ~150 lines, funnel to site, usable as a marketing-quality first impression.

### Outline

```
Versionx                         (H1)
  tagline (1 line, compelling)
  hero — screencast/asciinema or static screenshot (deferred: SVG demo block for v1)
  Install                        (3 code blocks: macOS/Linux, Windows, cargo)
  60-second demo                 (single fenced example: bare run → plan → apply)
  What's in the box              (5 bullets: runtimes, deps, releases, policy, AI/MCP)
  Status                         (single paragraph; link to /roadmap on site)
  Docs · SDK · MCP               (3 link cards to corresponding site sections)
  Community                      (Discussions, issues)
  Contributing · Security · License   (one line each, linked)
```

### Rules

- No `docs/spec/` links in the README. Those live in Contributing on the site.
- Every link that points offsite goes to the Docusaurus site first; the site funnels deeper.
- Installation block must be copy-pasteable without line wrapping on default GitHub width.
- No marketing puff that can't be backed by shipped behavior.

---

## 7. Tooling & build

### Docusaurus project shape

- `website/` — Docusaurus Classic preset.
- `website/package.json` — `npm`, not pnpm/yarn (simpler CI).
- `website/docusaurus.config.ts` — TypeScript config.
- `website/sidebars.ts` — hand-authored sidebar matching section 3 IA.
- `website/src/pages/index.tsx` — custom landing (not default Docusaurus splash).
- `website/docs/` — all MDX content.
- `website/static/diagrams/` — committed SVG diagrams.
- Dark mode on by default; Infima tokens tuned to project colors.
- `@docusaurus/theme-mermaid` for inline mermaid diagrams.
- `@easyops-cn/docusaurus-search-local` as the search fallback; Algolia DocSearch swapped in once application approves.

### xtask generators

- New `xtask/src/docs/` module, one submodule per generator.
- `cargo xtask docs` runs all of them.
- Generators are pure functions: no network, no side effects outside the target directory.

### CI workflow

- New `.github/workflows/docs.yml`.
- Triggers:
  - Push/PR affecting `website/**`, `crates/**/*.rs` (for generators), or `xtask/**`.
- Jobs:
  1. Run `cargo xtask docs`; fail if it mutates tracked files (`git diff --exit-code -- website/`).
  2. `npm ci` and `npm run build` in `website/`.
  3. `lychee` link check over the built HTML; fails on any 404 or broken internal link.
  4. On push to `main` only: deploy the `website/build/` output to GitHub Pages via the official Pages action.

### Repo-level additions

- `website/` (new)
- `xtask/src/docs/*.rs` (new)
- `.github/workflows/docs.yml` (new)
- `lychee.toml` (new; link-check config)
- `.gitignore` updates for `website/node_modules`, `website/build`, `website/.docusaurus`
- `CODEOWNERS` entry for `website/**` and `docs/spec/**`
- README rewrite

---

## 8. Maintenance model

- **Feature author writes the docs.** PR template gains a "Docs updated?" checkbox. For CLI/config/event/MCP/RPC/HTTP changes, regeneration is automatic; for guides and reference prose, it is manual.
- **Roadmap page** updated as part of release PRs.
- **Spec vs site contract:** `docs/spec/` remains authoritative for design decisions. When a spec doc changes in a way users care about, the corresponding site page must be updated in the same PR.
- **Style guide (brief):**
  - Plain voice, no marketing puff in Reference
  - No headings deeper than `###` outside reference pages
  - Code blocks always have a language tag
  - Imperative examples (`versionx plan`, not "you would run versionx plan")
  - No "awesome", "powerful", "blazing fast", etc.

---

## 9. Open implementation questions

These surface during the implementation plan, not the design:

- Exact `clap` metadata walker — `clap_mangen` gives us man-page markdown; we may need a thin custom pass to produce per-command Docusaurus MDX.
- Algolia DocSearch approval timing — the local search plugin covers day-one launch if Algolia isn't ready.
- Whether the HTTP API page embeds Scalar in-site or links out to the `aide`-served instance of the local daemon.
- Exact Infima palette values for dark and light themes.
- Whether we ship `CODEOWNERS` immediately or add during the first outside contribution.
- Exact set of asciinema/screencast recordings for the landing and Quickstart.

---

## 10. Acceptance criteria

The project is complete when:

1. `website/` builds cleanly (`npm run build`) with no warnings.
2. All pages listed in section 4 exist and are non-stub.
3. `cargo xtask docs` regenerates every auto-generated page with no manual edits needed afterwards.
4. CI gate blocks drift: any code change that affects generated docs must update the generated output.
5. `lychee` passes with zero broken links.
6. The site is live at `kodydennon.github.io/versionx`.
7. The new README is ≤ 170 lines and contains no `docs/spec/` links.
8. `docs/spec/` is untouched except for any corrections discovered during doc writing.
9. A contributor can land a PR that changes a CLI command and have the docs update end-to-end without human drift.

---

## 11. Deferred / post-v1

- Custom domain.
- PR preview deployments.
- Versioned documentation (flipped on at 1.0).
- Algolia DocSearch (if not approved in time for launch — local plugin carries).
- i18n / localization.
- Embedded API playground for the HTTP surface.
- Blog section.
