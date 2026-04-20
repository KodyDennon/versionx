---
title: Install
description: Install Versionx on macOS, Linux, or Windows. One static binary, no runtime dependencies except git.
sidebar_position: 1
---

# Install

Versionx ships as a single static binary for every major platform. Pick the path that matches how you usually install dev tools.

## macOS / Linux (curl installer)

```bash
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/KodyDennon/versionx/releases/latest/download/versionx-cli-installer.sh | sh
```

Installs into `~/.cargo/bin` (or `~/.local/bin` depending on your setup) and prints the exact path so you can verify.

## Windows (PowerShell installer)

```powershell
powershell -ExecutionPolicy ByPass -c "irm https://github.com/KodyDennon/versionx/releases/latest/download/versionx-cli-installer.ps1 | iex"
```

Installs into `%LOCALAPPDATA%\Programs\versionx`. The installer adds the directory to your user PATH.

## Homebrew (macOS / Linux)

```bash
brew install versionx
```

:::note
Formula publishes with each tagged release. Cutting edge users: `brew install --HEAD versionx`.
:::

## Scoop (Windows)

```powershell
scoop bucket add versionx https://github.com/KodyDennon/versionx
scoop install versionx
```

## Cargo (from source)

```bash
cargo install versionx-cli
```

Installs the `versionx` binary into `~/.cargo/bin`. Requires Rust 1.95+.

## npm (shim package)

```bash
npm install -g @versionx/cli
```

The npm package is a thin shim that downloads the right platform binary on first run and forwards invocations to it. Useful inside JavaScript-heavy environments.

## PyPI (shim package)

```bash
pipx install versionx
# or
pip install --user versionx
```

The PyPI package is the same shim idea for Python-heavy environments.

## Manual binary

Prebuilt archives for every supported platform are published on [GitHub Releases](https://github.com/KodyDennon/versionx/releases/latest). Download, extract, put the binary on your PATH.

Supported platforms:

- `x86_64-unknown-linux-gnu` (glibc 2.28+)
- `x86_64-unknown-linux-musl` (fully static)
- `aarch64-unknown-linux-gnu`
- `x86_64-apple-darwin`
- `aarch64-apple-darwin`
- `x86_64-pc-windows-msvc`
- `aarch64-pc-windows-msvc`

## Verify the install

```bash
versionx --version
```

You should see something like:

```text
versionx 0.7.0 (abc1234 2026-04-12)
```

## Wire up the shell hook

Versionx uses a thin shell hook to keep the per-session daemon alive and to put its shim directory on your PATH. Run this once per machine:

```bash
versionx install-shell-hook
```

This writes the right line to `~/.zshrc` / `~/.bashrc` / your fish config. Reload your shell:

```bash
exec $SHELL
```

Now a bare `versionx` in any repo works and shows the right next steps.

## Uninstall

If you installed via the curl / PowerShell installer, use the paired uninstaller:

```bash
# macOS / Linux
curl -LsSf https://github.com/KodyDennon/versionx/releases/latest/download/versionx-cli-uninstaller.sh | sh

# Windows
powershell -ExecutionPolicy ByPass -c "irm https://github.com/KodyDennon/versionx/releases/latest/download/versionx-cli-uninstaller.ps1 | iex"
```

If you installed via a package manager, use that package manager's uninstall command.

To remove all per-user state (runtimes, shims, cache, state DB), run:

```bash
versionx nuke --dry-run     # preview what would be deleted
versionx nuke               # actually delete
```

## See also

- [Quickstart](/get-started/quickstart) — try it on a real repo.
- [Environment variables](/reference/environment-variables) — for controlling where Versionx keeps its state.
