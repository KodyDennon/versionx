---
title: Dev environment setup
description: Clone, build, test, and run Versionx locally. Toolchain pin, xtask commands, IDE tips.
sidebar_position: 1
---

# Dev environment setup

You'll learn:

- What to install to build Versionx.
- How to run the same checks CI runs.
- IDE tips that help with a 30-crate workspace.

## Prerequisites

- **Rust 1.95+.** Pinned in `rust-toolchain.toml`; `rustup` picks it up automatically.
- **Git.**
- **`cargo-deny`** and **`cargo-nextest`** (optional but recommended for running CI locally).

```bash
cargo install cargo-deny cargo-nextest
```

Platform-specific:

- **macOS.** Xcode command-line tools.
- **Linux.** Build-essential (`gcc`, `make`). On musl hosts, `musl-tools`.
- **Windows.** Visual Studio Build Tools with the C++ workload.

## Clone and build

```bash
git clone https://github.com/KodyDennon/versionx
cd versionx
cargo check --workspace
cargo test --workspace
```

First-time compile of the full workspace takes a few minutes. Subsequent builds are incremental.

## xtask

The workspace bundles an `xtask` crate — internal build automation that shouldn't be in the public `versionx` CLI.

```bash
cargo xtask ci       # fmt + clippy + tests (what CI runs)
cargo xtask crates   # list workspace members
cargo xtask docs     # regenerate every auto-gen docs page
```

Add new chores to `xtask/src/main.rs` — don't pollute the public CLI with dev-only tasks.

## Run the binary

```bash
cargo run -p versionx-cli -- status
```

Or install the dev build on your PATH and use it as `versionx`:

```bash
cargo install --path crates/versionx-cli --offline
versionx status
```

## Run the daemon

```bash
cargo run -p versionx-daemon
```

In another terminal, run the CLI. It'll pick up the daemon via the socket.

## Run the MCP server

```bash
cargo run -p versionx-cli -- mcp --transport stdio
```

Pair with Claude Code, Cursor, or Codex per [MCP integrations](/integrations/mcp/overview).

## Run the TUI

```bash
cargo run -p versionx-cli -- tui
```

## IDE tips

- **rust-analyzer.** Set `"rust-analyzer.check.command": "clippy"` so diagnostics match CI warnings.
- **VS Code.** Enable "Save Without Formatting" if you're working through a heavy edit; rustfmt on save can fight you. Turn it back on when you're done.
- **Workspace-wide features.** Don't enable `--all-features` at check time; some feature combos are mutually exclusive by design.

## Before committing

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace
```

Or just:

```bash
cargo xtask ci
```

## See also

- [Workspace tour](./workspace-tour) — what each crate does.
- [Writing tests](./writing-tests) — the test conventions.
- [CONTRIBUTING.md](https://github.com/KodyDennon/versionx/blob/main/CONTRIBUTING.md) — the short version for first-timers.
