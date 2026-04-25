---
title: GitHub Actions recipes
description: Plain GitHub Actions workflow examples for the current Versionx alpha.
sidebar_position: 7
---

# GitHub Actions recipes

Versionx does not yet publish reusable Actions. These examples use normal shell
steps and a pinned Versionx release tag.

Set this once per workflow:

```yaml
env:
  VERSIONX_VERSION: v0.1.0-alpha.1776754296
```

## Install Versionx

Use on every job that needs the binary:

```yaml
- uses: actions/checkout@v4
  with:
    fetch-depth: 0
- run: |
    curl -LsSf \
      "https://github.com/KodyDennon/versionx/releases/download/${VERSIONX_VERSION}/versionx-cli-installer.sh" | sh
- run: versionx --version
```

## Daily sync

Keep runtimes, shims, and lockfile state fresh across PR branches:

```yaml
name: versionx sync
on: [pull_request]

jobs:
  sync:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: |
          curl -LsSf \
            "https://github.com/KodyDennon/versionx/releases/download/${VERSIONX_VERSION}/versionx-cli-installer.sh" | sh
      - run: versionx sync --no-daemon
```

## Dependency updates

You can preview or run dependency updates in CI today. The current alpha does
not yet have a separate approve/apply artifact for updates, so the command you
run is the command that performs the work.

Preview only:

```yaml
- run: versionx update --plan --ecosystem rust
```

Execute:

```yaml
- run: versionx update --ecosystem rust
```

If you want PR automation, wrap this in your own branch-and-commit workflow for
now.

## Release on merge to main

Direct release via PR titles:

```yaml
name: versionx release
on:
  push:
    branches: [main]

jobs:
  release:
    runs-on: ubuntu-latest
    permissions:
      contents: write
    steps:
      - uses: actions/checkout@v4
        with: { fetch-depth: 0 }
      - run: |
          curl -LsSf \
            "https://github.com/KodyDennon/versionx/releases/download/${VERSIONX_VERSION}/versionx-cli-installer.sh" | sh
      - run: versionx release plan
      - run: versionx release approve "$PLAN_ID"
      - run: versionx release apply "$PLAN_ID"
      - run: git push --follow-tags
```

Capture the `plan_id` from `versionx release plan` in a step output or shell
variable. The alpha CLI applies by `plan_id`, not by passing a JSON file path.

## Policy check on every PR

Fail CI if policy denies the proposed changes:

```yaml
name: versionx policy
on: [pull_request]

jobs:
  policy:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: |
          curl -LsSf \
            "https://github.com/KodyDennon/versionx/releases/download/${VERSIONX_VERSION}/versionx-cli-installer.sh" | sh
      - run: versionx policy check
```

## Composing: full CI

```yaml
name: CI
on: [pull_request, push]

jobs:
  ci:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: |
          curl -LsSf \
            "https://github.com/KodyDennon/versionx/releases/download/${VERSIONX_VERSION}/versionx-cli-installer.sh" | sh
      - run: versionx sync --no-daemon
      - run: versionx policy check
      - run: cargo test --workspace
      - run: cargo clippy --workspace --all-targets -- -D warnings
```

## Publish step

Versionx tags; your workflow publishes:

```yaml
name: publish
on:
  push:
    tags: ['*-v*']

jobs:
  npm:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with: { fetch-depth: 0 }
      - run: |
          curl -LsSf \
            "https://github.com/KodyDennon/versionx/releases/download/${VERSIONX_VERSION}/versionx-cli-installer.sh" | sh
      - run: versionx sync --no-daemon
      - run: npm publish
        env:
          NODE_AUTH_TOKEN: ${{ secrets.NPM_TOKEN }}
```

## See also

- [Orchestrating a release](/guides/orchestrating-a-release) — release strategies in depth.
- [Policy & waivers](/guides/policy-and-waivers) — writing the rules the action checks.
