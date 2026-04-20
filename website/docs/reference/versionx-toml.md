---
title: versionx.toml reference
description: Every key, type, default, and since-version for the Versionx configuration file.
sidebar_position: 3
---

# `versionx.toml`

:::info
Most of this page is **auto-generated** from the `versionx-config` crate. If a key looks wrong, fix it in `crates/versionx-config/src/schema.rs` and re-run `cargo xtask docs-config`.
:::

`versionx.toml` is the primary configuration file. One lives at the root of every repo that uses Versionx.

## Top-level sections

```toml
[workspace]         # repo identity, topology, members
[runtimes]          # language/toolchain pins
[ecosystems.node]   # per-ecosystem knobs
[ecosystems.python]
[ecosystems.rust]
[release]           # release strategy and options
[release.linked]    # linked-bump groups
[release.ignore]    # packages never released
[policies]          # local policies + inheritance
[tasks]             # task definitions
[env]               # environment variables scoped to this workspace
```

## `[workspace]`

| Key | Type | Default | Since | Description |
|---|---|---|---|---|
| `name` | string | derived from dir | 0.1 | Human-friendly workspace name. |
| `topology` | `"single" \| "submodule" \| "subtree" \| "virtual" \| "ref"` | `"single"` | 0.4 | Multi-repo shape. See [Multi-repo & monorepos](/guides/multi-repo-and-monorepos). |
| `members` | list of glob strings | `[]` | 0.4 | Paths that constitute this monorepo. |
| `inherit` | list of refs | `[]` | 0.5 | Fleet or policy configs to inherit from. |
| `tool-versions` | bool | `false` | 0.6 | Read pins from `.tool-versions` in addition to `[runtimes]`. |

## `[runtimes]`

Arbitrary key/value pairs. Key = runtime name, value = version or channel.

```toml
[runtimes]
node = "22.11.0"
python = "3.13"
rust = "stable"
pnpm = "9.15.4"
uv = "0.5.8"
```

Recognized runtimes in 0.7: `node`, `python`, `rust`, `pnpm`, `yarn`, `uv`. Additional runtimes in later versions — see [Roadmap](/roadmap).

## `[ecosystems.<name>]`

Per-ecosystem switches. All keys are optional.

```toml
[ecosystems.node]
manager = "pnpm"           # override auto-detection
hoist = "shamefully"       # pnpm hoisting style
frozen-lockfile = true     # CI mode

[ecosystems.python]
manager = "uv"             # pip / poetry / uv
virtualenv = ".venv"

[ecosystems.rust]
target = "x86_64-unknown-linux-gnu"
features = ["full", "otel"]
```

## `[release]`

| Key | Type | Default | Since | Description |
|---|---|---|---|---|
| `strategy` | `"pr-title" \| "commits" \| "changesets" \| "calver" \| "manual"` | `"pr-title"` | 0.3 | See [Orchestrating a release](/guides/orchestrating-a-release). |
| `initial-version` | semver | `"0.1.0"` | 0.3 | Version for the first release. |
| `base-branch` | string | `"main"` | 0.3 | Branch releases cut from. |
| `access` | `"public" \| "private"` | `"public"` | 0.3 | Default publish access. |
| `tag-component` | bool | `true` for monorepos | 0.4 | Include package name in tag. |
| `pre-major` | `"patch" \| "minor"` | `"patch"` | 0.5 | Bump behavior before 1.0. |
| `extra-files` | list of paths | `[]` | 0.6 | Additional files whose `version:` line gets bumped. |
| `changelog-path` | string | `"CHANGELOG.md"` | 0.3 | Where to write entries. |
| `changelog-format` | `"keepachangelog" \| "none" \| custom` | `"keepachangelog"` | 0.4 | Changelog style. |

### `[release.linked]`

```toml
[release.linked]
groups = [
  ["@my-app/core", "@my-app/cli"],
]
```

Packages in the same group always bump together.

### `[release.ignore]`

```toml
[release.ignore]
packages = ["internal-test-helpers"]
```

Packages that are never released.

## `[policies]`

```toml
[policies]
inherit = ["fleet://acme-platform/baseline"]
```

| Key | Type | Default | Since | Description |
|---|---|---|---|---|
| `inherit` | list of refs | `[]` | 0.5 | Policies inherited from fleet or other configs. |
| `local-dir` | string | `".versionx/policies"` | 0.5 | Where to look for local policies. |

See [Policy & waivers](/guides/policy-and-waivers).

## `[tasks]`

```toml
[tasks.test]
cmd = "cargo test --workspace"

[tasks.lint]
cmd = "cargo clippy --workspace -- -D warnings"

[tasks.e2e]
depends = ["build"]
cmd = "./scripts/e2e.sh"
```

| Key | Type | Default | Since | Description |
|---|---|---|---|---|
| `cmd` | string | — | 0.5 | Command to run. |
| `depends` | list of task names | `[]` | 0.5 | Tasks that must run first (topo-sorted). |
| `env` | table | `{}` | 0.5 | Extra environment for the task. |
| `cwd` | string | workspace root | 0.6 | Working directory override. |

Run via `versionx run --task <name>`.

## `[env]`

```toml
[env]
AWS_REGION = "us-east-1"
DATABASE_URL = { file = ".env.local", key = "DATABASE_URL" }
```

Scoped to Versionx-driven subprocesses (adapters, runtimes, tasks). Does **not** modify your shell.

## Validation

`versionx config validate` runs the same schema checks the core uses at load time. Failures print the exact line and a suggestion.

## See also

- [`versionx.lock` reference](./versionx-lock)
- [Environment variables](./environment-variables)
- [Design principles](/introduction/design-principles) — the rationale for "TOML-first, Luau when needed."
