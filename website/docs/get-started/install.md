---
title: Install
description: Install the current Versionx alpha on macOS, Linux, or Windows.
sidebar_position: 1
---

# Install

Versionx is currently in a public `0.1` alpha. The reliable install surfaces
today are:

- GitHub Releases prerelease artifacts
- Source builds via Cargo

Homebrew, Scoop, npm, and PyPI packages are planned, but they are not the
recommended install path for the current alpha.

## GitHub Releases (recommended for alpha testers)

Open the [GitHub Releases page](https://github.com/KodyDennon/versionx/releases)
and download the newest prerelease for your platform.

Current artifacts include macOS, Linux, and Windows archives for `versionx-cli`.
This is the install path the project actively verifies today.

## Cargo (build from source)

```bash
git clone https://github.com/KodyDennon/versionx
cd versionx
cargo install --path crates/versionx-cli
```

Requires Rust `1.95+`. This is the simplest way to test the current alpha if you
already have a Rust toolchain and want a source-based install.

## Supported release targets

- `x86_64-unknown-linux-gnu`
- `aarch64-unknown-linux-gnu`
- `x86_64-apple-darwin`
- `aarch64-apple-darwin`
- `x86_64-pc-windows-msvc`

These are the release targets configured in the current `cargo-dist` pipeline.

## Verify the install

```bash
versionx --version
```

You should see:

```text
versionx 0.1.0
```

## Wire up the shell hook

Versionx uses a thin shell hook to keep the per-session daemon alive and to put its shim directory on your PATH. Run this once per machine:

```bash
versionx install-shell-hook
```

This writes the right line to `~/.zshrc` / `~/.bashrc` / your fish config and
sets up the shim/daemon path the CLI expects. Reload your shell:

```bash
exec $SHELL
```

Now a bare `versionx` in any repo works and shows the right next steps.

## Planned package channels

These are intentionally not advertised as current alpha defaults yet:

- Homebrew
- Scoop
- npm shim
- PyPI shim

The release tooling is being shaped for them, but the public docs should not ask
you to depend on them until they are actually live and verified end to end.

To remove all per-user state (runtimes, shims, cache, state DB), run:

```bash
versionx nuke --dry-run     # preview what would be deleted
versionx nuke               # actually delete
```

## See also

- [Quickstart](/get-started/quickstart) — try it on a real repo.
- [Environment variables](/reference/environment-variables) — for controlling where Versionx keeps its state.
