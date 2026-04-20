---
title: GitHub Actions
description: Official reusable actions for installing, syncing, updating, releasing, and enforcing policy — plus composition examples.
sidebar_position: 5
---

# GitHub Actions

Versionx publishes a family of reusable actions. They're thin wrappers around the `versionx` binary; the same workflows run identically in GitLab CI, Jenkins, or locally.

## Actions

| Action | Purpose |
|---|---|
| `KodyDennon/versionx-install-action@v1` | Install the `versionx` binary (cached). |
| `KodyDennon/versionx-update-action@v1` | Plan + optionally PR dependency updates. |
| `KodyDennon/versionx-release-action@v1` | Plan + apply releases (direct or release-PR). |
| `KodyDennon/versionx-policy-action@v1` | Evaluate policies, fail the job on deny. |
| `KodyDennon/versionx-sync-action@v1` | Run `versionx sync` (install runtimes, refresh lockfiles). |

## Inputs (most common)

### `versionx-install-action`

| Input | Default | Description |
|---|---|---|
| `version` | `latest` | Versionx version to install. Pin to a minor for reproducibility (`0.7`). |
| `cache` | `true` | Cache the binary in `~/.cache/versionx-install-action`. |

### `versionx-update-action`

| Input | Default | Description |
|---|---|---|
| `mode` | `plan` | `plan`, `pr`. `pr` opens a PR. |
| `scope` | `workspace` | `workspace` / `fleet` / `members:a,b,c`. |
| `patch-only` | `false` | Limit to patch-level bumps. |
| `branch-prefix` | `deps/` | Branch-name prefix when `mode: pr`. |

### `versionx-release-action`

| Input | Default | Description |
|---|---|---|
| `mode` | `direct` | `direct` or `release-pr`. |
| `strategy` | from `versionx.toml` | Override the release strategy. |
| `push` | `true` | Push the resulting commit and tags. |
| `pr-branch` | `release/next` | Branch for `release-pr` mode. |

### `versionx-policy-action`

| Input | Default | Description |
|---|---|---|
| `scope` | `workspace` | Scope passed to `versionx policy eval`. |
| `fail-on` | `deny` | `deny`, `warn`, or `none`. |

## Composition

See [GitHub Actions recipes](/guides/github-actions-recipes) for ready-to-paste workflows covering CI, weekly updates, releases (both modes), and publishing.

## Non-GitHub CI

Nothing in these actions is GitHub-specific. The shape is always:

1. `versionx` binary on PATH (install-action does this; on other CI, curl-install).
2. A sequence of `versionx <subcommand>` calls.
3. Let your CI post results / open PRs / push tags.

Example GitLab CI step:

```yaml
release:
  image: ubuntu:24.04
  script:
    - curl -LsSf https://github.com/KodyDennon/versionx/releases/latest/download/versionx-cli-installer.sh | sh
    - versionx release plan --output json > release.json
    - versionx release apply release.json
```

## See also

- [GitHub Actions recipes](/guides/github-actions-recipes) — workflow examples.
- [Policy & waivers](/guides/policy-and-waivers) — rules the policy action enforces.
