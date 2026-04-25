---
title: Quickstart
description: Try the current Versionx alpha on a real repo in a few minutes.
sidebar_position: 2
---

# Quickstart

You'll learn:

- How to run Versionx with zero configuration on an existing repo.
- What the auto-detection produces.
- Which command to reach for next.

**Prerequisites:** you've [installed](/get-started/install) Versionx. `versionx install-shell-hook`
is recommended, but not required for the basic alpha flow.

## 1. Run it bare

Inside any project with a `package.json`, `Cargo.toml`, `pyproject.toml`, or similar, run:

```bash
versionx
```

You'll see something like:

```text
versionx 0.1.0 · ./my-app
  git✓ · config✗ · lock✗ · daemon✗ · 3 components discovered

  → run `versionx init` to synthesize a versionx.toml for this workspace.
  → run `versionx daemon start` (or `versionx install-shell-hook`) for warm caching.
```

That's the current zero-config alpha story: discovery is real, and the CLI gives
you the next useful step instead of assuming the repo is already configured.

## 2. See what's in scope

```bash
versionx status
```

Gives you a longer view of detected components, runtime pins, git/config/lockfile
state, and the current daemon status. Same JSON is available with `--output json`.

## 3. Generate config

```bash
versionx init
```

This writes a starter `versionx.toml` from the ecosystems Versionx discovered.
For a simple Node repo, the generated file looks like:

```toml
[versionx]
schema_version = "1"

[runtimes]
pnpm = "9.0.0"

[ecosystems.node]
package_manager = "pnpm"
```

## 4. Create the lockfile state

```bash
versionx sync
```

This resolves and records the current state into `versionx.lock`. That makes
later verification and release planning reproducible.

## 5. Propose a release

```bash
versionx release plan
```

`release plan` is an alias for the current `release propose` command. It writes
the plan into `.versionx/plans/` and prints the `plan_id` you need for the next
steps.

```bash
versionx release approve <plan-id>
versionx release apply <plan-id>
```

## What's not in this alpha yet

- Update plan approval/apply artifacts like the release workflow has
- Published reusable GitHub Actions
- Broader package-manager distribution channels like Homebrew/Scoop/npm/PyPI

## What next?

- [Your first release](/get-started/your-first-release) — the full walkthrough, end to end.
- [Managing toolchains](/guides/managing-toolchains) — pin Node/Python/Rust per repo.
- [Migrating from mise](/get-started/migrate-from-mise) or [asdf](/get-started/migrate-from-asdf) — bring your existing pins with you.

## See also

- [What is Versionx?](/introduction/what-is-versionx) — the big-picture pitch if you haven't read it.
- [Plan / apply cookbook](/sdk/plan-apply-cookbook) — deeper on the safety contract.
