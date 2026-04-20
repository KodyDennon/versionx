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

Versionx has no separate "approve" step — you approve by applying. If you want a human-review gate in CI, save the plan to a file, open a PR with it, merge when approved, then apply from the merged commit.

### 3. Apply

```bash
versionx release apply <plan.json>
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
versionx release rollback
```

Reverts the release commit, deletes the tag, restores the previous lockfile. Only works pre-push. Post-push, use a normal git revert.

## Pre-releases

```bash
versionx release pre-enter next
```

Sets the release channel to `next`. Subsequent releases tag as `my-app@0.3.0-next.1`, `my-app@0.3.0-next.2`. Exit with:

```bash
versionx release pre-exit
```

The next stable release rolls up the accumulated pre-release changes.

## Hotfixes

From a release branch (e.g., `release/1.x`):

```bash
versionx release plan --branch release/1.x
versionx release apply --branch release/1.x <plan.json>
```

Versionx cuts the release from the branch tip, bumps only the affected packages, and does not move main. Cherry-pick the hotfix commit back to main yourself (or let your CI do it).

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

```bash
versionx release plan --ai-changelog
```

Versionx groups the raw commits, sends them to your configured model, and produces a polished changelog. The plan still includes the raw commits so you can diff against the AI output. No key configured? The flag falls back to the template generator.

## Publishing

Versionx does **not** publish to registries. That's your CI's job. After `versionx release apply` and `git push --follow-tags`, your workflow picks up the new tag and runs `npm publish`, `cargo publish`, `twine upload`, etc. See [GitHub Actions recipes](/guides/github-actions-recipes) for the common shape.

## Troubleshooting

- **Plan empty after merges.** Check PR titles follow the pattern the strategy expects. `versionx release plan --explain` traces the decision.
- **Prerequisite check failed.** HEAD or the lockfile has moved since the plan was produced. Regenerate the plan.
- **Stuck in pre-release.** `versionx release status` shows the current channel and the outstanding pre-release tags. `versionx release pre-exit` unsticks.
- **Tag already exists.** Someone else cut the same release. Reconcile with `versionx release status --remote` and decide to re-tag or move on.

## See also

- [Your first release](/get-started/your-first-release) — end-to-end walkthrough.
- [Multi-repo & monorepos](/guides/multi-repo-and-monorepos) — cross-repo release coordination.
- [Policy & waivers](/guides/policy-and-waivers) — enforce release rules (freeze windows, required approvers, etc.).
