---
title: RFCs & design docs
description: Big changes start in docs/spec/. How to propose a new RFC, the review model, and the relationship between the spec and the site.
sidebar_position: 9
---

# RFCs & design docs

Big changes start in [`docs/spec/`](https://github.com/KodyDennon/versionx/tree/main/docs/spec). That directory is the authoritative design surface for the project — the Docusaurus site documents what Versionx does today; the spec documents what Versionx aims to be and why.

## When to write a spec entry

Write a new spec page or update an existing one when:

- You're proposing a new subsystem (e.g., a new tier of adapters, a new release strategy, remote state).
- You're changing a load-bearing interface in a way that affects multiple crates.
- You're making a decision that future maintainers will need the context for ("why did we shell out instead of reimplement?").

Skip the spec for:

- Bug fixes.
- Small feature additions that fit cleanly into existing subsystems.
- Documentation or test-only changes.

## Proposing a new spec page

1. Copy the nearest existing spec doc as a template.
2. Include the standard sections: **Scope**, **Contract**, **Design principles honored**, **Rationale**, **Open questions**.
3. File a PR titled `docs(spec): propose <topic>`.
4. The PR body includes a summary and a decision request.

## Review model

- Spec PRs are reviewed for architectural fit, not prose polish.
- Two approvals before merge for anything affecting crate boundaries.
- One approval for single-subsystem additions.
- Open questions are OK at merge time — they become follow-up issues.

## Relationship to the site

- **Spec is authoritative for design decisions.** "Why do we do X?" → spec.
- **Site is authoritative for current behavior.** "How do I do X?" → site.

When a spec doc changes in a way users care about, the corresponding site page(s) must update in the same PR. The reverse is not required — site-only edits don't need spec changes.

## The spec files today

Index at [`docs/spec/00-README.md`](https://github.com/KodyDennon/versionx/blob/main/docs/spec/00-README.md). Current set:

| # | File | Scope |
|---|---|---|
| 00 | `00-README.md` | Index, north star, design principles |
| 01 | `01-architecture-overview.md` | Crate layout, process model, transport surfaces |
| 02 | `02-config-and-state-model.md` | `versionx.toml`, lockfile, state DB |
| 03 | `03-ecosystem-adapters.md` | `PackageManagerAdapter` trait, tier plan |
| 04 | `04-runtime-toolchain-mgmt.md` | Runtime installers, shims, pinning |
| 05 | `05-release-orchestration.md` | Release strategies, AI-assisted changelogs |
| 06 | `06-multi-repo-and-monorepo.md` | Topologies, saga protocol |
| 07 | `07-policy-engine.md` | Policy DSL, Luau, waivers |
| 08 | `08-github-integration.md` | Actions first, App deferred |
| 09 | `09-programmatic-and-ai-api.md` | SDK, JSON-RPC, HTTP, MCP |
| 10 | `10-mvp-and-roadmap.md` | Phased delivery plan |
| 11 | `11-version-roadmap.md` | Per-version 0.1 → 1.4 plan |

Add a new file with the next number. The 00 index grows to match.

## See also

- [Architecture](./architecture) — the summary that points at the spec.
- [Design principles](/introduction/design-principles) — the load-bearing ideas the spec is written against.
