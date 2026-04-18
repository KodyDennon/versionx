# Versionx

> **Versionx** is a cross-platform, cross-language, cross-package-manager version manager and release orchestrator written in Rust. Binary: `versionx`.

Versionx unifies runtime/toolchain management (asdf/mise replacement), dependency management (npm/pip/cargo/etc.), SemVer release orchestration (changesets/release-please style), multi-repo coordination (submodules/subtrees/virtual monorepos), policy enforcement, and first-class AI-agent integration via MCP — all behind a single progressive-disclosure interface.

**The wedge:** cross-repo atomic release orchestration with plan/apply safety, polyglot version handling, and AI-as-client architecture. No existing tool sits at this intersection.

---

## Status

**Pre-0.1.0 scaffold.** The workspace structure, CI, and release pipeline are in place. The actual implementation is underway — see [`docs/spec/11-version-roadmap.md`](docs/spec/11-version-roadmap.md) for the concrete 0.1 → 1.0 plan.

You can clone, `cargo check`, and run `cargo test`, but nothing is wired up yet.

## Docs

The full vision specification lives in [`docs/spec/`](docs/spec/):

- [`00-README.md`](docs/spec/00-README.md) — index, north star, design principles
- [`01-architecture-overview.md`](docs/spec/01-architecture-overview.md) — crates, process model, transport surfaces
- [`02-config-and-state-model.md`](docs/spec/02-config-and-state-model.md) — `versionx.toml`, lockfile, SQLite schema
- [`03-ecosystem-adapters.md`](docs/spec/03-ecosystem-adapters.md) — `PackageManagerAdapter` trait, tier plan
- [`04-runtime-toolchain-mgmt.md`](docs/spec/04-runtime-toolchain-mgmt.md) — mise/asdf replacement
- [`05-release-orchestration.md`](docs/spec/05-release-orchestration.md) — PR-title default, AI-as-client changelog
- [`06-multi-repo-and-monorepo.md`](docs/spec/06-multi-repo-and-monorepo.md) — submodule/subtree/virtual/ref
- [`07-policy-engine.md`](docs/spec/07-policy-engine.md) — declarative TOML + Luau, waivers with expiry
- [`08-github-integration.md`](docs/spec/08-github-integration.md) — Actions first, hosted App deferred
- [`09-programmatic-and-ai-api.md`](docs/spec/09-programmatic-and-ai-api.md) — SDK, JSON-RPC, HTTP, MCP
- [`10-mvp-and-roadmap.md`](docs/spec/10-mvp-and-roadmap.md) — v1.0 MVP cut and rationale
- [`11-version-roadmap.md`](docs/spec/11-version-roadmap.md) — per-version 0.1 → 1.4 release plan

## Development

```bash
# Prereqs: rustc 1.88+ (pinned in rust-toolchain.toml), git.
git clone https://github.com/kody/versionx
cd versionx
cargo check --workspace
cargo test --workspace
cargo xtask ci   # fmt + clippy + test
```

Workspace layout:

```
crates/
  versionx-core/            # THE library
  versionx-cli/             # `versionx` binary
  versionx-tui/             # `versionx tui`
  versionx-daemon/          # `versiond` daemon
  versionx-web/             # local-only axum UI
  versionx-mcp/             # MCP server (rmcp)
  versionx-shim/            # <200 KB trampoline binary
  versionx-adapter-*/       # Node / Python / Rust (more in 1.x)
  versionx-runtime-*/       # Toolchain installers
  versionx-config/          # versionx.toml
  versionx-lockfile/        # versionx.lock
  versionx-state/           # SQLite (rusqlite + WAL)
  versionx-policy/          # Declarative + Luau
  versionx-release/         # SemVer / CalVer / PR-title / changesets
  versionx-tasks/           # Native task runner (phased)
  versionx-multirepo/       # Submodule / subtree / virtual / ref
  versionx-git/             # gix for reads, git2 for writes
  versionx-github/          # Octocrab wrapper
  versionx-events/          # Structured event bus
  versionx-sdk/             # Public Rust SDK
```

## License

Licensed under [Apache License, Version 2.0](./LICENSE-APACHE).

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in Versionx by you shall be licensed as above, without any additional terms or conditions.

## Security

Report vulnerabilities per [SECURITY.md](./SECURITY.md). Please do not open public issues for suspected vulnerabilities.
