---
title: Your first release
description: Walk through the current single-repo release flow in the Versionx alpha.
sidebar_position: 3
---

# Your first release

You'll learn:

- How to cut a full release from scratch with Versionx.
- The plan / apply cycle for releases.
- What ends up in git when the release finishes.

**Prerequisites:**

- Versionx [installed](/get-started/install).
- A git repo with at least one ecosystem in it (Node, Python, or Rust).
- A git remote you can push to.

## 1. Init

From the repo root:

```bash
versionx init
```

Creates a `versionx.toml` reflecting what was detected. Open it; a minimal file for a Node app looks like:

```toml
[workspace]
name = "my-app"

[release]
strategy = "pr-title"       # Conventional Commit PR titles
initial-version = "0.1.0"

[ecosystems.node]
# Versionx drives npm/pnpm/yarn via the existing package.json.
```

Commit the file:

```bash
git add versionx.toml
git commit -m "chore: add versionx.toml"
```

## 2. Make a change and PR

Write a small change. Merge a PR titled like a Conventional Commit:

```text
feat(api): add /status endpoint
```

Squash-merge the PR. That one line becomes the changelog entry.

If you prefer the [changesets](/get-started/migrate-from-changesets) workflow instead, drop a file in `.versionx/changesets/`:

```markdown
---
"my-app": minor
---

Added /status endpoint.
```

## 3. Plan the release

```bash
versionx release plan
```

You'll see a plan like:

```text
plan_id: blake3:25115a4550714143356805ed1d418d842bceae6331dba8b47645e46faffee855
strategy: conventional
approved: false
expires_at: 2026-04-26 03:42:03 UTC
saved_to: .versionx/plans/25115a4550714143356805ed1d418d842bceae6331dba8b47645e46faffee855.toml
proposed bumps (1):
  my-app                           0.1.0 -> 0.2.0     [minor]
```

The current alpha persists release plans to `.versionx/plans/` and identifies
them by `plan_id`.

## 4. Approve

```bash
versionx release approve <plan-id>
```

## 5. Apply

```bash
versionx release apply <plan-id>
```

What happens, atomically:

1. **Version bumps** in every affected manifest (`package.json`, `Cargo.toml`, `pyproject.toml`).
2. **Changelog** written to `CHANGELOG.md` with a new entry.
3. **Lockfile** refreshed so dependency resolution is reproducible.
4. **Commit** created with a conventional message (`chore(release): my-app@0.2.0`).
5. **Tag** created at that commit (`my-app-v0.2.0`).
6. **State DB** records the run.

Prerequisites are checked before any mutation: if HEAD has moved or the lockfile hash doesn't match what the plan was generated against, apply fails with a clear error. Nothing is half-applied.

## 6. Push

```bash
git push --follow-tags
```

Your CI picks up the new tag and runs whatever publish workflow you have wired. Versionx itself does not publish to registries; that's your CI's job. See [GitHub Actions recipes](/guides/github-actions-recipes) for common shapes.

## 7. Verify

```bash
versionx status
```

Should show:

```text
Release     my-app@0.2.0  (tagged abc1234, 12s ago)
Policy      clean
```

Done. You've cut a release end to end.

## What about rollback?

If something's wrong after apply but before push, use the same `plan_id`:

```bash
versionx release rollback <plan-id>
```

Reverts the release commit, deletes the tag, and restores the previous lockfile. Only works if you haven't pushed yet. After push, use a normal git revert workflow.

## What about cross-repo?

Fleet release orchestration exists in the codebase, but the outside-user alpha
story is still centered on single-repo workflows first. Treat multi-repo release
guides as advanced/experimental until the hardening pass lands.

## See also

- [Orchestrating a release](/guides/orchestrating-a-release) — full guide to every strategy and corner case.
- [Release reference](/reference/cli/versionx) — every flag on every `versionx release` command.
- [`versionx.toml` reference](/reference/versionx-toml) — `[release]` section schema.
