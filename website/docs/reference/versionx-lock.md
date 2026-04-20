---
title: versionx.lock reference
description: Format specification, content-hash model, and merge-conflict guidance for the Versionx lockfile.
sidebar_position: 4
---

# `versionx.lock`

The Versionx lockfile aggregates native lockfiles by hash plus the resolved runtime versions. It is the single reproducibility artifact for a Versionx-managed workspace.

## Location

At the workspace root. Always committed to git. Always LF line endings.

## Format

TOML. Stable top-level shape:

```toml
version = 1

[meta]
generated-by = "versionx 0.7.0"
generated-at = "2026-04-18T14:02:17Z"
schema = 1

[runtimes.node]
version = "22.11.0"
source = "nodejs.org"
sha256 = "..."

[runtimes.python]
version = "3.13.1"
source = "python-build-standalone"
sha256 = "..."

[[ecosystems.node]]
path = "apps/web"
manifest = "package.json"
manager = "pnpm"
manager-version = "9.15.4"
native-lockfile = "apps/web/pnpm-lock.yaml"
blake3 = "9f1e..."

[[ecosystems.python]]
path = "services/api"
manifest = "pyproject.toml"
manager = "uv"
manager-version = "0.5.8"
native-lockfile = "services/api/uv.lock"
blake3 = "3a2c..."

[[ecosystems.rust]]
path = "."
manifest = "Cargo.toml"
native-lockfile = "Cargo.lock"
blake3 = "ab11..."

[workspace]
members-hash = "blake3:d41f..."
policy-hash  = "blake3:8021..."
```

## Content-hash model

Versionx does **not** maintain its own resolution of every transitive dependency. That's what the native lockfiles are for. Instead, `versionx.lock` stores a Blake3 hash of each native lockfile plus the resolved runtime versions. The hash is the reproducibility guarantee: if `pnpm-lock.yaml` or `Cargo.lock` changes, `versionx.lock` must change too.

This means:

- Diff noise is minimal. Changing one dep in one ecosystem updates one hash and the upstream lockfile.
- Merges are tractable. See below.
- `versionx sync` verifies every hash on startup; any drift prints a clear error.

## Merge conflicts

Conflicts in `versionx.lock` usually come in two shapes.

### Native lockfile also conflicted

Resolve the native lockfile (`pnpm-lock.yaml`, `Cargo.lock`, etc.) first using that tool's standard workflow, then:

```bash
versionx sync
```

which recomputes the Blake3 hashes and rewrites `versionx.lock` cleanly.

### Only `versionx.lock` conflicted

Accept either side, then run `versionx sync`. Versionx re-verifies every hash and rewrites. If the native lockfile content matches expectations, the final `versionx.lock` is deterministic regardless of which side you started with.

## Schema migration

`version = 1` is the 1.0 format. Breaking schema changes bump this field and Versionx migrates on first read, rewriting the file in the new shape. You should commit the migrated file.

## What's not in `versionx.lock`

- **Transitive dependency trees.** Those live in native lockfiles.
- **Local state.** The state DB is per-user and never committed.
- **Secrets or tokens.** Versionx never writes these to disk.

## See also

- [`versionx.toml` reference](./versionx-toml) — config that drives what's in the lockfile.
- [Polyglot dependency updates](/guides/polyglot-dependency-updates) — how updates flow into the lockfile.
