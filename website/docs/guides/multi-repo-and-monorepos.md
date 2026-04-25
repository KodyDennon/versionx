---
title: Multi-repo & monorepos
description: Coordinate across submodules, subtrees, virtual monorepos, and independent repos. How atomic releases work across boundaries.
sidebar_position: 4
---

# Multi-repo & monorepos

You'll learn:

- The four workspace topologies Versionx supports.
- How to coordinate commands across many repos.
- How atomic cross-repo releases work via the saga protocol.

**Prerequisites:** Versionx [installed](/get-started/install).

## The four topologies

Versionx recognizes these workspace shapes:

1. **Single repo** — one git root, one `versionx.toml`. Default.
2. **Submodule monorepo** — one outer repo, many `git submodule`-linked inner repos.
3. **Subtree monorepo** — one outer repo, inner content imported via `git subtree`.
4. **Virtual monorepo** — many independent repos described in a shared fleet config.
5. **Ref monorepo** — branches of the same repo represent different packages (rare; for migration scenarios).

The workspace layer lives in `versionx-multirepo`. Each topology has its own handler; the top-level API is the same.

## Declaring the topology

In `versionx.toml`:

```toml
[workspace]
topology = "submodule"              # or subtree / virtual / ref / single
members = ["apps/*", "packages/*"]
```

For virtual monorepos, reference the fleet config:

```toml
[workspace]
topology = "virtual"
inherit = ["fleet://acme-platform/baseline"]

[members]
app-api = { git = "https://github.com/acme/app-api",  ref = "main" }
app-web = { git = "https://github.com/acme/app-web",  ref = "main" }
worker  = { git = "https://github.com/acme/worker",   ref = "main" }
```

## Running commands across repos

The current alpha exposes fleet-oriented commands through the `fleet` surface
rather than a universal `--scope` flag:

```bash
versionx fleet status
versionx fleet members
versionx fleet query --help
versionx fleet sync
```

## Atomic cross-repo releases (the saga protocol)

To release a feature that spans several repos:

```bash
versionx fleet release --help
```

The multi-repo saga protocol exists in the codebase, but the outside-user alpha
story is still maturing. Treat it as advanced/experimental until the single-repo
hardening work finishes.

1. **Prepare** — verify every member can be released (git clean, lockfiles synced, policy green).
2. **Commit** — on every member in order defined by the dep graph.
3. **Tag** — every member.
4. **Push** — every member. Any failure triggers compensating rollbacks.
5. **Record** — state DB captures the saga ID so the run is auditable.

If the saga fails at step 4 after 2 of 3 pushes succeed, Versionx runs compensating commands on the successful pushes to either delete the tag or mark the run as failed — your policy decides. See [Policy & waivers](/guides/policy-and-waivers).

## Fleet config

Living in a dedicated ops repo (e.g., `acme/platform-ops`):

```toml
# platform-ops/versionx-fleet.toml
[fleet]
name = "acme-platform"

[runtimes]
node = "22"
python = "3.13"

[policies]
default = "acme-platform/baseline.policy.toml"

[members]
# Every repo in the fleet lists itself here.
app-api = { git = "https://github.com/acme/app-api" }
app-web = { git = "https://github.com/acme/app-web" }
```

Members opt in with `[workspace] inherit = ["fleet://..."]` in their own `versionx.toml`.

## TUI dashboard

For interactive work:

```bash
versionx tui
```

The Dashboard view shows every repo Versionx is aware of, their release status, outstanding updates, policy state, and current runtime pins. Drill in to see the event log for any member. See [Daemon & TUI](/guides/daemon-and-tui).

## Troubleshooting

- **Member not discovered.** Check the fleet config and use `versionx fleet members` / `versionx fleet query` to inspect what the CLI sees.
- **Different members have conflicting pins.** Use `versionx fleet status` and inspect the member configs directly; richer drift tooling is still part of the roadmap.

## See also

- [Orchestrating a release](/guides/orchestrating-a-release) — release semantics for a single repo.
- [Policy & waivers](/guides/policy-and-waivers) — enforce rules that span members.
