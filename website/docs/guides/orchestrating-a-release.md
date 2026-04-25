---
title: Orchestrating a release
description: Plan, approve, and apply releases with any strategy — PR titles, changesets, Conventional Commits, CalVer, or manual.
sidebar_position: 3
---

# Orchestrating a release

You'll learn:

- Every release strategy Versionx supports.
- The plan / approve / apply / rollback cycle.
- How to handle pre-releases, hotfixes, and stuck releases.

**Prerequisites:** a repo with `versionx.toml`; [your first release](/get-started/your-first-release) completed.

## Strategies

Configure in `versionx.toml`:

```toml
[release]
strategy = "pr-title"     # default
```

| Strategy | Input | When to use |
|---|---|---|
| `pr-title` | Conventional-Commit-shaped PR titles from squash merges | Default for most teams. Matches the GitHub squash-merge workflow. |
| `commits` | Full commit history parsed as Conventional Commits | Teams that rebase or preserve commit history and write every commit as a Conventional Commit. |
| `changesets` | `.versionx/changesets/*.md` files | Teams that prefer explicit intent files per PR. Full `@changesets/cli` compatibility. |
| `calver` | Current date | Date-driven releases (e.g., `2026.04.0`). |
| `manual` | `version` field in config / explicit CLI arg | Truly ad-hoc releases. |

## The cycle

Every strategy ends up in the same four steps.

### 1. Plan

```bash
versionx release plan
```

Produces:

- A version bump per affected package.
- Changelog entries grouped by category.
- Prerequisites (HEAD SHA, lockfile Blake3, TTL).

Inspect as JSON:

```bash
versionx release plan --output json
```

### 2. Approve

The current alpha has an explicit approval step:

```bash
versionx release approve <plan-id>
```

### 3. Apply

```bash
versionx release apply <plan-id>
```

Atomic:

1. Version bumps in every affected manifest.
2. Changelog entry appended.
3. Lockfile refreshed.
4. Commit + tag.
5. State DB records the run.

Prerequisites are re-checked before any mutation. If HEAD moved or the lockfile hash changed, apply fails cleanly.

### 4. Rollback (before push)

```bash
versionx release rollback <plan-id>
```

Reverts the release commit, deletes the tag, restores the previous lockfile. Only works pre-push. Post-push, use a normal git revert.

## Pre-releases

```bash
versionx release prerelease <plan-id> --channel rc
```

This rewrites an approved plan into a prerelease variant and applies it through
the current CLI path.

## Hotfixes

From a release branch (e.g., `release/1.x`):

The current public alpha does not expose the richer branch-specific flags yet.
Use the release planner from the branch tip you want to release from.

## Cross-package coordination

For repos with multiple publishable packages:

```toml
[release.linked]
groups = [
  ["@my-app/core", "@my-app/cli"],       # always bump together
]

[release.ignore]
packages = ["internal-test-helpers"]     # never publish
```

`versionx release plan` respects these groupings.

## Cross-repo atomic releases

See [Multi-repo & monorepos](/guides/multi-repo-and-monorepos) for the saga protocol that coordinates atomic releases across repos.

## AI-assisted changelogs

If you've configured a BYO API key (Anthropic, OpenAI, Gemini, or Ollama):

Use the dedicated changelog surface instead:

```bash
versionx changelog draft
```

## Publishing

Versionx does **not** publish to registries. That's your CI's job. After `versionx release apply` and `git push --follow-tags`, your workflow picks up the new tag and runs `npm publish`, `cargo publish`, `twine upload`, etc. See [GitHub Actions recipes](/guides/github-actions-recipes) for the common shape.

## Troubleshooting

- **Plan empty after merges.** Check PR titles follow the pattern the strategy expects and that the repo already has the config/lockfile state Versionx expects.
- **Prerequisite check failed.** HEAD or the lockfile has moved since the plan was produced. Regenerate the plan.
- **Tag already exists.** Someone else cut the same release. Inspect the saved plan and repo state, then decide whether to retry or roll forward.

## See also

- [Your first release](/get-started/your-first-release) — end-to-end walkthrough.
- [Multi-repo & monorepos](/guides/multi-repo-and-monorepos) — cross-repo release coordination.
- [Policy & waivers](/guides/policy-and-waivers) — enforce release rules (freeze windows, required approvers, etc.).
