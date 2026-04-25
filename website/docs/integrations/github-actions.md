---
title: GitHub Actions
description: Current GitHub Actions guidance for the Versionx alpha.
sidebar_position: 5
---

# GitHub Actions

Versionx does **not** currently ship a published family of reusable GitHub
Actions. That is planned. For the current alpha, use plain workflow steps that
install the binary and then call the CLI directly.

## Current recommendation

Pin a real release tag and install from GitHub Releases in your workflow. For
example:

```yaml
env:
  VERSIONX_VERSION: v0.1.0-alpha.1776754296

steps:
  - uses: actions/checkout@v4
    with:
      fetch-depth: 0
  - run: |
      curl -LsSf \
        "https://github.com/KodyDennon/versionx/releases/download/${VERSIONX_VERSION}/versionx-cli-installer.sh" | sh
  - run: versionx --version
```

Update the pinned release tag when you upgrade.

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
    - curl -LsSf https://github.com/KodyDennon/versionx/releases/download/${VERSIONX_VERSION}/versionx-cli-installer.sh | sh
    - versionx release plan
    - versionx release approve "$PLAN_ID"
    - versionx release apply "$PLAN_ID"
```

## See also

- [GitHub Actions recipes](/guides/github-actions-recipes) — plain-shell workflow examples for the current alpha.
- [Policy & waivers](/guides/policy-and-waivers) — rules the policy action enforces.
