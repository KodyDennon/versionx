---
title: Managing toolchains
description: Pin, install, and switch language runtimes per repo. How Versionx's shim layer works and how to keep it out of your way.
sidebar_position: 1
---

# Managing toolchains

You'll learn:

- How to pin Node, Python, Rust (and more) per repo.
- What Versionx's shim layer actually does.
- How to install, list, and prune runtimes.
- How to share a set of pins across a fleet.

**Prerequisites:** Versionx [installed](/get-started/install); `versionx install-shell-hook` run once.

## Pin a runtime

In `versionx.toml`:

```toml
[runtimes]
node = "22.11.0"
python = "3.13.1"
rust = "1.95"
```

Versions can be exact (`22.11.0`), minor-loose (`22`), semver ranges (`^22.11`), or latest-in-channel (`lts`, `stable`, `nightly` where meaningful).

Open a new shell inside the repo. `node --version` resolves to the pinned version via the Versionx shim. When you leave the directory, PATH behavior depends on global defaults (see below).

## How shims work

Versionx prepends a single shim directory to your PATH (`$XDG_DATA_HOME/versionx/shims` on Linux, `~/Library/Application Support/versionx/shims` on macOS, `%LOCALAPPDATA%\versionx\shims` on Windows). Each shim is a tiny native binary that looks up the right version by:

1. Looking up from the current directory for a `versionx.toml`.
2. Falling back to the user's global `config.toml`.
3. Dispatching to the real binary in the runtime cache.

Cold-path dispatch is sub-millisecond — the shim uses an mmap'd PATH cache so it doesn't need to parse TOML on every call.

## Install what's pinned

```bash
versionx sync
```

Installs every pin in `versionx.toml` that isn't already cached. Existing installs are not re-downloaded.

Install a specific runtime at a specific version:

```bash
versionx install node 22.11.0
```

List what's installed:

```bash
versionx runtime list
```

## Switch versions

Edit `versionx.toml`. The shim picks up the change immediately — no shell reload needed. `versionx install` (or `versionx sync`) will pull the new version if it isn't cached.

You can also set user-wide defaults:

```bash
versionx global set node 22
versionx global get node
versionx global unset node
```

## Global defaults

Per-user defaults live in `$XDG_CONFIG_HOME/versionx/config.toml`:

```toml
[runtimes]
node = "lts"
python = "3.13"
```

Used when you're outside any `versionx.toml`-scoped directory. Repo pins always override globals.

## Package manager pinning

Node package managers (pnpm, yarn) and Python package managers (uv, pipx) are pinnable as first-class runtimes:

```toml
[runtimes]
node = "22"
pnpm = "9.15.4"
uv = "0.5.8"
```

Versionx installs them as real binaries — **not** via corepack (which is being removed in Node 25+).

## Clean up

Prune runtimes not referenced by any known repo:

```bash
versionx runtime prune --dry-run
versionx runtime prune
```

## Sharing pins across a fleet

For multiple repos that should share a toolchain baseline, put the pins in a fleet config:

```toml
# In your ops repo: platform-ops/versionx-fleet.toml
[runtimes]
node = "22"
python = "3.13"
```

Downstream repos inherit:

```toml
# In each repo: versionx.toml
[workspace]
inherit = ["fleet://acme-platform/baseline"]
```

See [Multi-repo & monorepos](/guides/multi-repo-and-monorepos) for how the fleet config is resolved.

## Troubleshooting

- **`command not found` inside a repo.** Run `versionx doctor`. Likely the shim directory isn't on PATH — check your shell rc has the `versionx install-shell-hook` line.
- **Version mismatch.** `versionx which <tool>` shows the resolved binary and why it was chosen.
- **Slow first run of a tool.** First invocation of a newly-installed runtime pays a shim cache rebuild cost (~5ms). Subsequent runs are sub-ms.

## See also

- [`versionx.toml` reference](/reference/versionx-toml) — `[runtimes]` schema.
- [Adding a runtime installer](/contributing/adding-a-runtime-installer) — if you want to add a language Versionx doesn't support yet.
