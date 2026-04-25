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
| `include-component-in-tag` | `[release] tag_template = "{package}-v{version}"` |
| `changelog-path` | `[release] changelog` |
| `packages` | Warning today; manual follow-up may still be needed |
| `bump-minor-pre-major` / `bump-patch-for-minor-pre-major` / `extra-files` / `changelog-type` | Warning today; manual review |
| `.release-please-manifest.json` | Not needed — state lives in git tags + state DB |

## Current alpha status

There is now a real helper:

```bash
versionx migrate --from release-please
```

The current alpha migrates the release settings it can represent today:

- `[release].strategy = "pr-title"`
- `changelog-path` when it can determine one
- `include-component-in-tag` as `tag_template = "{package}-v{version}"`

It also prints warnings for release-please settings that still need manual
translation.

## CI cutover

Old GitHub Actions step:

```yaml
- uses: googleapis/release-please-action@v4
  with:
    release-type: node
```

New step: install Versionx in CI, then run the CLI directly. See
[GitHub Actions recipes](/guides/github-actions-recipes) for the current plain-shell pattern.

## What changes

- **Migration is partial, not magical.** The helper gets the obvious release settings into `versionx.toml`, then prints the remaining manual follow-up.
- **No manifest file to check into the repo.** State DB plus git tags is the source of truth.
- **Commit messages are less strict.** release-please requires Conventional Commits on every commit. Versionx (with `pr-title` default) only requires Conventional-style PR titles since squash-merge is the norm. If you squash-merge already, this matches what you do. If you don't, switch to the `commits` strategy instead for full-history parsing.

## What you gain

- Multi-ecosystem releases in the same repo without ceremony. release-please handles this via `release-type: manifest` but the config gets unwieldy; Versionx detects per-directory.
- Cross-repo releases (a saga protocol) — release-please can't do this.
- Plan / apply. The release is a JSON object you can review before it's cut.
- Policy integration. Block releases during a freeze window or without a waiver.

## What doesn't translate cleanly

- **Automatic maintenance of the release PR.** If that behavior is critical today, keep release-please for that slice while you evaluate the rest of Versionx.

## Troubleshooting

- **Release plan shows no bumps after a merge.** Check the PR title actually matched Conventional Commit format and that the repo has the config/lockfile state Versionx expects.
- **Two release PRs fighting.** Disable release-please's workflow before enabling Versionx's release-PR mode.

## See also

- [Orchestrating a release](/guides/orchestrating-a-release) — all strategies in depth.
- [GitHub Actions recipes](/guides/github-actions-recipes) — the release-PR and direct-release shapes.
