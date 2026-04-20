---
title: Migrating from release-please
description: Move a release-please-based workflow to Versionx. PR-title parsing, release-PR vs direct, cutover steps.
sidebar_position: 7
---

# Migrating from release-please

You'll learn:

- Which release-please patterns translate directly (most) and which don't (release PRs).
- How to set up Versionx's PR-title strategy to match your current behavior.
- What to do differently once you cut over.

**Prerequisites:** a working release-please setup, Versionx [installed](/get-started/install).

## The big-picture difference

release-please maintains a **release PR** that accumulates changes and opens automatically. Merging that PR cuts the release.

Versionx's default (`strategy = "pr-title"`) parses Conventional Commit-style PR titles from squash merges and cuts releases from CI (or locally). There is **no release PR**.

If you specifically want the release-PR workflow, Versionx supports it via `strategy = "pr-title"` + a CI recipe that opens the release PR. It's a 15-line GitHub Action, not a separate tool.

## Config mapping

| release-please | Versionx |
|---|---|
| `.release-please-config.json` → `packages` | `versionx.toml` → `[workspace]` + package manifests |
| `release-type` (`node`, `rust`, `python`) | Auto-detected per ecosystem |
| `bump-minor-pre-major` | `[release] pre-major = "minor"` |
| `bump-patch-for-minor-pre-major` | `[release] pre-major-minor-bumps = "patch"` |
| `include-component-in-tag` | `[release] tag-component = true` |
| `extra-files` | `[release.extra-files]` |
| `changelog-path` | `[release] changelog-path` |
| `changelog-type` | `[release] changelog-format` |
| `.release-please-manifest.json` | Not needed — state lives in git tags + state DB |

## The fast path

```bash
versionx migrate release-please
```

Reads `.release-please-config.json` and writes `versionx.toml`. Inspect.

## CI cutover

Old GitHub Actions step:

```yaml
- uses: googleapis/release-please-action@v4
  with:
    release-type: node
```

New step:

```yaml
- uses: KodyDennon/versionx-release-action@v1
  with:
    strategy: pr-title
```

The action is just a thin wrapper around `versionx release plan` + `versionx release apply`. See [GitHub Actions recipes](/guides/github-actions-recipes) for the full workflow including the release-PR pattern.

## What changes

- **No release PR by default.** Merges happen; the next `versionx release plan` cuts a release. If you prefer a release PR, opt into it with a 15-line workflow — the release action supports `mode: release-pr`.
- **No manifest file to check into the repo.** State DB plus git tags is the source of truth.
- **Commit messages are less strict.** release-please requires Conventional Commits on every commit. Versionx (with `pr-title` default) only requires Conventional-style PR titles since squash-merge is the norm. If you squash-merge already, this matches what you do. If you don't, switch to the `commits` strategy instead for full-history parsing.

## What you gain

- Multi-ecosystem releases in the same repo without ceremony. release-please handles this via `release-type: manifest` but the config gets unwieldy; Versionx detects per-directory.
- Cross-repo releases (a saga protocol) — release-please can't do this.
- Plan / apply. The release is a JSON object you can review before it's cut.
- Policy integration. Block releases during a freeze window or without a waiver.

## What doesn't translate cleanly

- **Automatic maintenance of the release PR.** If you relied on release-please for this, set `mode: release-pr` on the Versionx action; it maintains the same PR shape. If that's a deal-breaker, release-please and Versionx can coexist — Versionx still drives runtimes and deps; release-please still drives releases — there's no collision.

## Troubleshooting

- **Release plan shows no bumps after a merge.** Check the PR title actually matched Conventional Commit format. Run `versionx release plan --explain` for the step-by-step decision trace.
- **Two release PRs fighting.** Disable release-please's workflow before enabling Versionx's release-PR mode.

## See also

- [Orchestrating a release](/guides/orchestrating-a-release) — all strategies in depth.
- [GitHub Actions recipes](/guides/github-actions-recipes) — the release-PR and direct-release shapes.
