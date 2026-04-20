---
title: GitHub Actions recipes
description: Ready-to-paste workflows for install, sync, release, update, and policy. The reusable Versionx Actions.
sidebar_position: 7
---

# GitHub Actions recipes

Official reusable actions live at [`KodyDennon/versionx-actions`](https://github.com/KodyDennon/versionx). Below are the common workflow shapes.

## Install Versionx

Use on every job that needs the binary:

```yaml
# .github/workflows/ci.yml (excerpt)
- uses: KodyDennon/versionx-install-action@v1
  with:
    version: '0.7'      # or 'latest'
```

Caches the binary in `~/.cache/versionx-install-action` between runs.

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
      - uses: KodyDennon/versionx-install-action@v1
      - run: versionx sync --no-daemon
```

## Weekly dependency updates

One PR per ecosystem with grouped patch bumps:

```yaml
name: versionx update
on:
  schedule:
    - cron: '0 6 * * 1'        # Mondays 06:00 UTC
  workflow_dispatch:

jobs:
  update:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with: { token: ${{ secrets.GITHUB_TOKEN }} }
      - uses: KodyDennon/versionx-install-action@v1
      - uses: KodyDennon/versionx-update-action@v1
        with:
          mode: pr                   # open a PR per ecosystem
          patch-only: true
          branch-prefix: deps/
```

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
      pull-requests: write
    steps:
      - uses: actions/checkout@v4
        with: { fetch-depth: 0 }
      - uses: KodyDennon/versionx-install-action@v1
      - uses: KodyDennon/versionx-release-action@v1
        with:
          mode: direct
          strategy: pr-title
          push: true
```

## Release-PR pattern (release-please style)

If you want release-please's "maintain a PR that opens when bumps accumulate" behavior:

```yaml
name: versionx release PR
on:
  push:
    branches: [main]

jobs:
  release-pr:
    runs-on: ubuntu-latest
    permissions:
      contents: write
      pull-requests: write
    steps:
      - uses: actions/checkout@v4
        with: { fetch-depth: 0 }
      - uses: KodyDennon/versionx-install-action@v1
      - uses: KodyDennon/versionx-release-action@v1
        with:
          mode: release-pr
          strategy: pr-title
          pr-branch: release-please-compat     # rename to whatever you like
```

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
      - uses: KodyDennon/versionx-install-action@v1
      - uses: KodyDennon/versionx-policy-action@v1
        with:
          scope: workspace
          fail-on: deny                 # or: warn, or: none
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
      - uses: KodyDennon/versionx-install-action@v1
      - run: versionx sync --no-daemon
      - run: versionx policy eval --fail-on deny
      - run: versionx run --task test
      - run: versionx run --task lint
```

`versionx run --task test` executes whatever `[tasks.test]` in `versionx.toml` defines. See the `[tasks]` section in [`versionx.toml` reference](/reference/versionx-toml).

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
      - uses: KodyDennon/versionx-install-action@v1
      - run: versionx sync --no-daemon
      - run: npm publish
        env:
          NODE_AUTH_TOKEN: ${{ secrets.NPM_TOKEN }}
```

## See also

- [Orchestrating a release](/guides/orchestrating-a-release) — release strategies in depth.
- [Policy & waivers](/guides/policy-and-waivers) — writing the rules the action checks.
