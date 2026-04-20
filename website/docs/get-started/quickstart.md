---
title: Quickstart
description: Try Versionx on a real repo in 60 seconds. Zero configuration. Detects what you're using and suggests next steps.
sidebar_position: 2
---

# Quickstart

You'll learn:

- How to run Versionx with zero configuration on an existing repo.
- What the auto-detection produces.
- Which command to reach for next.

**Prerequisites:** you've [installed](/get-started/install) Versionx and run `versionx install-shell-hook`.

## 1. Run it bare

Inside any project with a `package.json`, `Cargo.toml`, `pyproject.toml`, or similar, run:

```bash
versionx
```

You'll see something like:

```text
Versionx 0.7.0

Workspace  ./my-app    (node 22.11.0, python 3.13.1, rust 1.95)
Outdated   3 packages in apps/web  (axios ^1.6 → ^1.7)
Policy     clean
Ready      release plan   (last release 12d ago)

What next?
  versionx status                show ecosystem + release health
  versionx update --plan         preview dependency bumps
  versionx release plan          propose the next release
```

That's a zero-config run. Versionx walked the directory, detected which ecosystems are in use, and offered relevant actions.

## 2. See what's in scope

```bash
versionx status
```

Gives you a longer view — every detected ecosystem, every outdated dependency, every policy rule that would match, the current release state. Same JSON is available with `--output json`.

## 3. Preview a dependency bump (no side effects)

```bash
versionx update --plan
```

Emits a JSON plan describing exactly which package manifests would change, which lockfiles would refresh, and what the new resolved versions would be. Nothing has happened yet. You can inspect the plan, stash it, and apply it later:

```bash
versionx update --plan > plan.json
# review plan.json
versionx apply plan.json
```

This is the core **plan / apply** contract. Every mutating command supports it.

## 4. Generate a config (optional)

If you want to pin a runtime or change defaults, produce a starter `versionx.toml`:

```bash
versionx init
```

It writes a minimal config reflecting what was detected. Edit away. See [`versionx.toml` reference](/reference/versionx-toml) for the full schema.

## 5. Propose a release

```bash
versionx release plan
```

Parses commit history (or changesets, or PR titles — depending on your chosen strategy) and emits a release plan: what bumps where, what the changelog looks like, what tags to create. Approve and apply the same way:

```bash
versionx release plan > release.json
# review release.json
versionx release apply release.json
```

## What next?

- [Your first release](/get-started/your-first-release) — the full walkthrough, end to end.
- [Managing toolchains](/guides/managing-toolchains) — pin Node/Python/Rust per repo.
- [Migrating from mise](/get-started/migrate-from-mise) or [asdf](/get-started/migrate-from-asdf) — bring your existing pins with you.

## See also

- [What is Versionx?](/introduction/what-is-versionx) — the big-picture pitch if you haven't read it.
- [Plan / apply cookbook](/sdk/plan-apply-cookbook) — deeper on the safety contract.
