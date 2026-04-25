---
title: Migrating from changesets
description: Move a changesets-based release workflow to Versionx. Compatibility mode, file-format mapping, cutover steps.
sidebar_position: 6
---

# Migrating from changesets

You'll learn:

- How to keep your existing `.changeset/` directory and workflow working on Versionx.
- How `@changesets/cli` commands map to `versionx release ...`.
- What's different when you want to graduate beyond changesets' model.

**Prerequisites:** an existing changesets setup, Versionx [installed](/get-started/install).

## Current alpha status

There is now a real helper:

```bash
versionx migrate --from changesets
```

The current alpha migrates the release setting it can express today:

- `[release].strategy = "changesets"`
- your existing `.changeset/` files stay in place

It also prints warnings for config that still needs manual follow-up.

## Config mapping

| changesets | Versionx |
|---|---|
| `.changeset/config.json` → `strategy` | `[release] strategy = "changesets"` |
| `.changeset/config.json` → `linked` | Warning today; manual follow-up to `[[release.groups]]` |
| `.changeset/config.json` → `ignore` | Warning today; no shipped schema key yet |
| `.changeset/config.json` → `baseBranch` / `access` / `commit` / `updateInternalDependencies` | Warning today; manual review |
| `.changeset/*.md` | Same files. Versionx reads them directly. |

Changeset file format is unchanged:

```markdown
---
"my-package": minor
"my-other-package": patch
---

Added the thing.
```

## Command mapping

| changesets | Versionx |
|---|---|
| `changeset` | not yet replaced by a dedicated Versionx command |
| `changeset version` | `versionx release plan`, then `approve`, then `apply` |
| `changeset publish` | Done by your CI after Versionx tags. See [GitHub Actions recipes](/guides/github-actions-recipes). |
| `changeset status` | inspect plans via `versionx release list` / `versionx release show` |

## Cutover recipe

1. Run `versionx migrate --from changesets`.
2. Commit: `chore: migrate from changesets to versionx`.
3. Update CI to call `versionx release plan` / `versionx release apply` instead of `changeset version`.
4. Keep `@changesets/cli` installed for now if PR authors have muscle memory; `changeset` the command still works. You can remove it later.

## What changes

- **Single binary.** You don't need `@changesets/cli` as a project dep (though it's fine to keep it as a convenience).
- **Plan / approve / apply.** The current alpha persists a release plan and applies it by `plan_id`.
- **Pre-release channels.** Versionx supports `versionx release prerelease`, but the more ergonomic changesets-style flow is still evolving.

## What you gain

- Changelogs can be AI-assisted via MCP if you want. BYO API key; Versionx calls your model and falls back to a template if you don't configure one.
- Multi-ecosystem. If you add a Rust crate or a Python package to your JS monorepo, changesets can't follow — Versionx can.
- Policy integration. Rules like "no major bumps on Fridays" or "require a waiver for breaking-change PRs" are enforceable without extra tools.

## What you lose (carefully nothing)

Changesets compatibility is still partial in the current alpha. The helper gets
you onto the real Versionx release surface, but some config still needs manual
follow-up after the generated warnings.

## See also

- [Orchestrating a release](/guides/orchestrating-a-release) — the full release guide.
- [Migrating from release-please](/get-started/migrate-from-release-please) — if you're using both in a polyglot repo.
