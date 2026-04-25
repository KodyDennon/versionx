---
title: Polyglot dependency updates
description: How the current Versionx alpha plans and runs dependency updates across shipped ecosystems.
sidebar_position: 2
---

# Polyglot dependency updates

Versionx now ships a real alpha `update` flow for the ecosystems that already
have adapters in the CLI: Node, Python, and Rust.

## Current command surface

Preview the update without mutating anything:

```bash
versionx update --plan
```

Target one ecosystem:

```bash
versionx update --plan --ecosystem rust
```

Target one package when the ecosystem adapter supports it:

```bash
versionx update --plan serde
versionx update --ecosystem node react
```

Execute the update and refresh `versionx.lock`:

```bash
versionx update
```

The current alpha writes updated ecosystem lock metadata back into
`versionx.lock`. It does **not** yet emit a separate plan artifact with
approval/apply semantics the way `versionx release plan` does.

## What this does today

- Drives the real package manager for the ecosystem:
- Node: `pnpm update`, `npm update`, or `yarn up`
- Python: `uv lock --upgrade`, `poetry update`, or `pip install --upgrade`
- Rust: `cargo update`
- Supports `--plan` for dry-run previews.
- Supports `--ecosystem <id>` for scoped runs.
- Forwards an optional dependency selector to the adapter.
- Refreshes `versionx.lock` after a real update run.

## What is still missing

- Approval/apply plan artifacts for updates
- CI recipes that open dependency-update PRs automatically
- Tier-2 ecosystem coverage beyond Node, Python, and Rust
- Policy-aware update filtering and waiver-driven automation

## See also

- [Quickstart](/get-started/quickstart)
- [Orchestrating a release](/guides/orchestrating-a-release)
- [Roadmap](/roadmap)
