---
title: Polyglot dependency updates
description: Unified status, update, and audit across every package manager. How Versionx drives npm, pip, cargo, and others.
sidebar_position: 2
---

# Polyglot dependency updates

You'll learn:

- How to see outdated dependencies across every ecosystem in one view.
- How to plan and apply updates atomically.
- How Versionx drives the real package managers without replacing them.

**Prerequisites:** a repo with at least one ecosystem in it.

## One view, every ecosystem

```bash
versionx status
```

Shows outdated packages from npm, pip, cargo, and others in a single table:

```text
Outdated (5)

ecosystem  package            current    latest   bump
node       axios              ^1.6.0     ^1.7.7   minor
node       @types/node        ^20.11.0   ^22.7.5  major
python     fastapi            0.115.0    0.115.6  patch
rust       serde              1.0.210    1.0.217  patch
rust       tokio              1.41.1     1.42.0   minor
```

## Plan an update

```bash
versionx update --plan
```

Emits a JSON plan with the exact manifest edits and lockfile refreshes. Nothing changes yet.

Scope narrower:

```bash
versionx update --plan --eco node            # only Node packages
versionx update --plan axios tokio           # only named packages
versionx update --plan --patch-only          # only patch-level bumps
versionx update --plan --breaking            # include major bumps
```

## Apply an update

```bash
versionx update --plan > update.json
versionx apply update.json
```

What happens atomically per ecosystem:

- Manifest edits (`package.json`, `pyproject.toml`, `Cargo.toml`).
- Lockfile refresh via the native package manager (`npm install`, `pip install --upgrade`, `cargo update`).
- A single commit with a conventional message per ecosystem.

If any ecosystem's update fails (network error, test failure in a post-hook), the whole apply rolls back. Nothing is half-committed.

## Why "drive, don't reimplement"

Versionx does not have its own resolver for npm, pip, or cargo. It:

1. Reads the manifests itself (fast, deterministic).
2. Produces a plan with the intended new version ranges.
3. Invokes the native package manager to resolve and write lockfiles.
4. Verifies the result.

This means:

- Your lockfile format is unchanged. Consumers who don't use Versionx still see a normal `package-lock.json`, `poetry.lock`, or `Cargo.lock`.
- Resolver bugs, security hole fixes, and new lockfile formats come from the upstream package manager the day they ship.
- Edge cases specific to your package manager (peer deps, workspace protocols, feature flags) are handled by the real tool.

## Audit mode

```bash
versionx audit
```

Runs each ecosystem's native audit (`npm audit`, `pip-audit`, `cargo audit`) and merges the results into one view. Respects policy rules: if your policy says "block merges on critical CVEs," `versionx audit --policy` exits non-zero accordingly.

## Scheduling updates in CI

See [GitHub Actions recipes](/guides/github-actions-recipes) for a weekly-update workflow that opens one PR per patch group.

## Policy integration

To require updates go through a specific channel (e.g., "security patches only this week"), see [Policy & waivers](/guides/policy-and-waivers).

## Troubleshooting

- **`versionx update` says up to date but `npm outdated` disagrees.** Check `.versionxignore` and any `[ignore]` block in `versionx.toml`. Versionx may be filtering packages intentionally.
- **Lockfile conflicts after apply.** Versionx runs the native package manager in a known-clean state. If you're seeing conflicts, run `versionx sync --clean` to refresh from scratch.
- **Rust `cargo update` is slow.** Turn on the daemon's file watcher so it can cache the resolved index: `versionx activate bash` in your shell rc.

## See also

- [Orchestrating a release](/guides/orchestrating-a-release) — how dep updates flow into the release pipeline.
- [Policy & waivers](/guides/policy-and-waivers) — enforce rules on what bumps are allowed.
