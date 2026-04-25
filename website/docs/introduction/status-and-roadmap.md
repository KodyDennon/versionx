---
title: Status & roadmap
description: Versionx is a public 0.1 alpha with real foundations and a lot of roadmap still ahead.
sidebar_position: 3
---

# Status & roadmap

Versionx is **0.1 alpha, publicly testable**. The repo already has a substantial
Rust workspace and a working CLI/MCP foundation, but the public story needs to
stay honest: some surfaces are ready for outside alpha use today, and many of
the broader automation/distribution features are still roadmap items.

## What's shipped today

- **Workspace discovery.** Detects Node, Python, Rust, and mixed workspaces without config.
- **Config bootstrap.** `versionx init` generates a usable `versionx.toml`.
- **Lockfile flow.** `versionx sync` creates `versionx.lock` and records runtime state.
- **Dependency updates.** `versionx update` ships for Node, Python, and Rust with `--plan`, optional package targeting, and lockfile refresh.
- **Release planning.** `release plan` / `propose`, `approve`, `apply`, `rollback`, `snapshot`, and `prerelease` work in the CLI.
- **Policy engine.** `policy init/check/explain/list/update/verify` and expiring waivers are implemented.
- **MCP server.** `versionx mcp serve` and `versionx mcp describe` expose the current agent surface.
- **Daemon and shell hook.** `versionx daemon ...` and `install-shell-hook` are wired for warm caching.
- **30 crates and a real CI/docs pipeline.** This is a serious codebase, not just a spec dump.

## What is not shipped yet

- Update plan approval/apply artifacts beyond the current `versionx update --plan` dry-run
- Published reusable GitHub Actions
- Broad package-manager install channels (Homebrew, Scoop, npm, PyPI)
- A fully hardened outside-user multi-repo story
- A docs site where every page is already aligned to the current alpha surface

## Road to 1.0

1.0 ships when:

1. The current alpha commands are hardened and documented end to end.
2. Tier-1 ecosystems (Node, Python, Rust) have a clean outside-user workflow.
3. Dependency updates and broader release automation land as real commands, not doc promises.
4. Package distribution beyond GitHub Releases is actually live.
5. Windows parity closes the remaining rough edges.
6. Outside users have successfully exercised the tool on real repos.

Target: late 2026. No committed date.

## Post-1.0 (1.1 / 1.2 / 2.0)

- **1.1** — Go + Ruby adapters and runtimes reach Stable. OCI adapter lands.
- **1.2** — Tier-2 JVM adapter (maven/gradle). Local task cache. Remote state DB (Postgres backend). Optional bearer-auth for the HTTP API.
- **2.0** — Remote task cache. Hosted GitHub App (maybe — still a business question).

The full per-version plan with gates and demos lives in [`docs/spec/11-version-roadmap.md`](https://github.com/KodyDennon/versionx/blob/main/docs/spec/11-version-roadmap.md).

## What's deliberately deferred

- **Hosted GitHub App.** Reusable Actions are enough for 1.0. SaaS infrastructure burden isn't worth the user experience delta yet.
- **Remote-exposed HTTP API.** Loopback-only in 1.0. Remote is a 1.2 question with real auth and rate-limiting.
- **Bundled LLM.** Versionx will not ship a model. Ever.
- **Telemetry.** Not in 1.0. Not in 2.0. Not ever.

## See also

- [Roadmap](/roadmap) — the honest alpha-to-1.0 timeline.
- [Design principles](/introduction/design-principles) — why the roadmap is shaped this way.
- [`docs/spec/11-version-roadmap.md`](https://github.com/KodyDennon/versionx/blob/main/docs/spec/11-version-roadmap.md) — the authoritative internal roadmap.
