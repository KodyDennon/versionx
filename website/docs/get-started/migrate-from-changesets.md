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

## The fast path

```bash
versionx migrate changesets
```

- Reads `.changeset/config.json`.
- Writes `versionx.toml` with `[release] strategy = "changesets"`.
- Leaves `.changeset/*.md` files in place — they're the input.

## Config mapping

| changesets | Versionx |
|---|---|
| `.changeset/config.json` → `baseBranch` | `[release] base-branch` |
| `.changeset/config.json` → `access` | `[release] access` |
| `.changeset/config.json` → `commit` | `[release] commit-on-version` |
| `.changeset/config.json` → `linked` | `[release.linked]` |
| `.changeset/config.json` → `ignore` | `[release.ignore]` |
| `.changeset/config.json` → `updateInternalDependencies` | `[release] update-internal-dependencies` |
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
| `changeset` | `versionx release add` |
| `changeset version` | `versionx release plan && versionx release apply` |
| `changeset publish` | Done by your CI after Versionx tags. See [GitHub Actions recipes](/guides/github-actions-recipes). |
| `changeset status` | `versionx release status` |

## Cutover recipe

1. Run `versionx migrate changesets`. Inspect the produced `versionx.toml`.
2. Commit: `chore: migrate from changesets to versionx`.
3. Update CI to call `versionx release plan` / `versionx release apply` instead of `changeset version`.
4. Keep `@changesets/cli` installed for now if PR authors have muscle memory; `changeset` the command still works. You can remove it later.

## What changes

- **Single binary.** You don't need `@changesets/cli` as a project dep (though it's fine to keep it as a convenience).
- **Plan / apply.** `versionx release plan` emits a JSON plan. `versionx release apply plan.json` executes it. The traditional `changeset version` + manual review still works — Versionx exposes the same imperative path.
- **Pre-release channels.** Versionx has a separate `--channel` flag rather than changesets' `pre enter / pre exit` mode. Migration: `changeset pre enter next` → `versionx release pre-enter next`.

## What you gain

- Changelogs can be AI-assisted via MCP if you want. BYO API key; Versionx calls your model and falls back to a template if you don't configure one.
- Multi-ecosystem. If you add a Rust crate or a Python package to your JS monorepo, changesets can't follow — Versionx can.
- Policy integration. Rules like "no major bumps on Fridays" or "require a waiver for breaking-change PRs" are enforceable without extra tools.

## What you lose (carefully nothing)

The changesets format is first-class in Versionx. Every other thing you can do in changesets, Versionx can do. If you find a gap, it's a bug — please [open an issue](https://github.com/KodyDennon/versionx/issues).

## See also

- [Orchestrating a release](/guides/orchestrating-a-release) — the full release guide.
- [Migrating from release-please](/get-started/migrate-from-release-please) — if you're using both in a polyglot repo.
