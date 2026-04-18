# Contributing to Versionx

Thanks for considering a contribution! Versionx is early-stage — the most valuable contributions right now are bug reports, design feedback, and small focused PRs.

## Quick start

```bash
git clone https://github.com/kody/versionx
cd versionx
cargo check --workspace
cargo test --workspace
cargo xtask ci        # runs fmt + clippy + tests as CI would
```

Prereqs:
- Rust 1.88+ (pinned in `rust-toolchain.toml`; `rustup` picks it up automatically).
- Git.
- A reasonably recent `cargo-deny` and `cargo-nextest` if you want to run the full CI locally: `cargo install cargo-deny cargo-nextest`.

## Repo structure

See [README.md](README.md) for the workspace layout and [`docs/spec/01-architecture-overview.md`](docs/spec/01-architecture-overview.md) for the full crate breakdown.

## Making changes

1. **Open an issue first** for anything larger than a typo fix. The design space is wide; we want to avoid rework.
2. **Follow the crate boundaries.** `versionx-core` is the brain; frontends (`versionx-cli`, `versionx-mcp`, etc.) should never call `git` or ecosystem tools directly.
3. **Adapters never depend on core.** See [`docs/spec/01-architecture-overview.md`](docs/spec/01-architecture-overview.md) for the dependency rules.
4. **Every mutating operation is plan/apply.** The core contract — we don't bypass it.
5. **Tests travel with code.** Minimum: a happy-path unit test. Ideally: a property test for anything involving state transitions.

## Code style

- `cargo fmt --all` before committing.
- `cargo clippy --workspace --all-targets -- -D warnings` must pass.
- Doc comments on every `pub` item (clippy checks this via the pedantic lint set).
- Prefer `thiserror` at crate boundaries and `anyhow` in the CLI.

## Commit messages

Versionx eats its own dogfood: the default release strategy is **PR-title parsing** following [Conventional Commits](https://www.conventionalcommits.org/). Examples:

- `feat(cli): add --output json-lines`
- `fix(adapter-node): handle missing packageManager field`
- `chore: bump rmcp to 1.5.3`
- `docs: correct example in 04-runtime-toolchain-mgmt.md`

PR titles are what end up in the changelog (squash-merge workflow). Commit messages inside a PR matter less, but keep them readable.

Breaking changes: add `!` after type/scope and a `BREAKING CHANGE:` footer, e.g.:

```
feat(release)!: rename `strategy=conventional` to `strategy=commits`

BREAKING CHANGE: Users of the conventional-commits strategy must
rename their versionx.toml `[release] strategy = "conventional"`
entry to `strategy = "commits"`. `vx migrate` handles this.
```

## Licensing

By contributing you agree your changes are licensed under Apache-2.0 (the project license). No CLA required; see the `LICENSE-APACHE` file.

## Questions

Open a GitHub Discussion. Bug reports: open an issue.
