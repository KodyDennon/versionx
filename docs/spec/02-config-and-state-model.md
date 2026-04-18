# 02 — Config & State Model

## Scope
Defines the complete data model: the `versionx.toml` schema, the `versionx.lock` format, the local SQLite state DB schema, the optional remote/fleet state backend, and exactly how progressive disclosure works across these layers.

## Contract
After reading this file you should be able to: write a valid `versionx.toml` for a polyglot monorepo, understand every column in the state DB, and know which operations touch which layers.

---

## 1. The five layers, restated

| Layer | What exists | Who uses it |
|---|---|---|
| L1 | Nothing — zero config | Quick try, first run |
| L2 | `versionx.toml` | Solo devs, simple repos |
| L3 | `versionx.toml` + `versionx.lock` | Any reproducible project (most users live here) |
| L4 | + local SQLite state DB | Anyone using TUI, multi-repo views, cross-repo queries |
| L5 | + remote state + policies | Teams, fleet management |

**Golden rule**: a user at L5 deleting the state DB and policy files falls cleanly back to L3. Git is always the source of truth.

---

## 2. `versionx.toml` — the primary config

TOML. One file per repo (or workspace root). Edited with `toml_edit` to preserve comments/formatting on round-trip.

### 2.1 Minimal example (L2)
```toml
[runtimes]
node = "20"
python = "3.12"
```
That's a complete, valid config. Versionx will detect `package.json` / `pyproject.toml` and drive the right tools.

### 2.2 Full example (L4–L5)
```toml
# === Top-level ===
[versionx]
schema_version = "1"
name = "tides-pilates-platform"                # optional, defaults to dir name
workspace = true                                # this file is a workspace root (relevant for walk-up)

# === Environment ===
[vars]
# Loaded in addition to `.env` and `.env.local` (which are read automatically).
# Explicit [vars] here override .env values. Precedence: shell env > [vars] > .env.local > .env.
NODE_ENV = "development"
DATABASE_URL = "${SECRET_DB_URL}"               # interpolated at load time

# === Runtime pins (the mise/asdf piece) ===
[runtimes]
node = "20.11.1"                                # exact or "20", "^20", "lts"
pnpm = "8.15.0"                                 # Versionx installs this directly (no corepack)
python = "3.12.2"
rust = "1.78.0"

[runtimes.providers]                            # optional: where to install from
node = "nodejs.org"                             # official
python = "python-build-standalone"              # astral-sh builds (formerly indygreg)
rust = "rustup"
ruby = "rv"                                     # prebuilt binaries (spinel-coop/rv)
jvm = "temurin"                                 # via foojay Disco API

# === Ecosystem declarations ===
# Versionx auto-detects most of these. This block is for overrides + opt-outs.
[ecosystems.node]
package_manager = "pnpm"                        # auto: check packageManager field, lockfiles
root = "."                                      # where package.json lives
workspaces = ["apps/*", "packages/*"]           # if monorepo

[ecosystems.python]
package_manager = "uv"
root = "services/api"
venv_manager = "uv"                             # delegate venv creation to uv; "poetry" or "versionx" also allowed

[ecosystems.rust]
root = "crates"

# === Task runner (native, phased) ===
[tasks.build]
run = "turbo build"                             # v1.0: spawns with correct toolchain
# v1.2 adds [tasks.build] inputs/outputs = [...] for content-addressed caching
# v2.0 adds remote cache + sandboxing

[tasks.test]
run = "turbo test"
depends_on = ["build"]

[tasks.lint]
run = "turbo lint"

# === Release policy for THIS repo ===
[release]
strategy = "pr-title"                           # "pr-title" | "conventional" | "changesets" | "manual"
ai_assist = "mcp"                               # "mcp" (via agent) | "byo-api" | "off"
versioning = "semver"                           # "semver" | "calver" | "custom"
tag_template = "v{version}"
changelog = "CHANGELOG.md"
plan_ttl = "24h"                                # 1h–30d; can be overridden per command with --ttl
push_mode = "prompt"                            # "prompt" (TTY) | "explicit" (require --push/--no-push)

[release.ai.byo]                                # optional: headless AI calls without an agent
provider = "anthropic"                          # "anthropic" | "openai" | "gemini" | "ollama"
model = "claude-sonnet-4-6"
api_key_env = "ANTHROPIC_API_KEY"
endpoint_env = "ANTHROPIC_BASE_URL"             # optional for self-hosted / proxies

[release.packages]
# Fine-grained per-package release config (for monorepos)
"packages/ui" = { public = true, registry = "npm" }
"packages/internal" = { public = false }

# === Multi-repo links ===
[links]
# Each link declares how an external repo is integrated. Versionx supports
# whatever mechanism you already use — we don't force one.
[links.shared-ui]
type = "submodule"                              # "submodule" | "subtree" | "virtual" | "ref"
path = "vendor/shared-ui"
url = "https://github.com/acme/shared-ui.git"
track = "main"                                  # branch to follow for updates
update = "pr"                                   # "pr" | "auto" | "manual"

[links.design-tokens]
type = "subtree"
path = "packages/tokens"
url = "https://github.com/acme/tokens.git"
track = "main"
bidirectional = true                            # can push patches upstream

[links.fleet-repos]
type = "virtual"                                # aggregate N repos as a virtual workspace
members = [
  "git@github.com:acme/service-a.git",
  "git@github.com:acme/service-b.git",
]

# === Policy references (L5) ===
[policies]
inherit = ["fleet://acme-platform/baseline"]    # pull from fleet's ops repo
files = [".versionx/policies/*.policy.toml"]    # local files

# === GitHub / CI integration ===
[github]
owner = "acme"
repo = "tides-pilates-platform"
required_checks = ["versionx/policy", "versionx/lock-integrity"]
# Note: v1.0 has no hosted GitHub App. Checks are posted from reusable Actions.

# === Inheritance control (for workspace monorepos) ===
[inherit]
# By default arrays are leaf-wins (replace). Use `append` for explicit merging:
append = ["ecosystems.node.workspaces", "policies.files"]

# === Advanced ===
[advanced]
daemon = "auto"                                 # "auto" | "always" | "never"
jobs = 0                                        # 0 = num_cpus.min(8)
```

### 2.3 Schema rules
- `schema_version` is required at L3+; L2 may omit and defaults to latest.
- Unknown top-level keys are errors (strict validation). Unknown keys inside `[advanced]` are warnings (future-compat).
- All paths are relative to the file location unless absolute.
- Env var interpolation: `"${GITHUB_TOKEN}"` resolved at load time. Missing vars are errors unless `"${VAR:-default}"` form is used. `"${VAR@lazy}"` opts into adapter-invocation-time resolution.
- **`.env` loading**: `.env` and `.env.local` at the workspace root are read automatically. Precedence (highest wins): shell env > `[vars]` block > `.env.local` > `.env`.
- **Inheritance**: `versionx.toml` files lower in the tree inherit from `versionx.toml` at the workspace root. Defaults to **leaf-wins (replace)** semantics. Keys listed in `[inherit] append = [...]` merge instead. Sealed fleet policies cannot be disabled downstream (see `07-policy-engine.md §3.3`).

### 2.4 Zero-config detection (L1)
When no `versionx.toml` exists, Versionx synthesizes one in memory from filesystem signals:

| Signal | Inferred |
|---|---|
| `package.json` with `packageManager: "pnpm@..."` | `node` runtime pinned, pnpm adapter, versionx manages pnpm install |
| `package.json` only | Node with auto-detected pm (lockfile-driven) |
| `pyproject.toml` with `[tool.uv]` | Python + uv |
| `pyproject.toml` with `[tool.poetry]` | Python + poetry |
| `requirements.txt` | Python + pip |
| `Cargo.toml` | Rust + cargo |
| `go.mod` | Go (v1.1) |
| `Gemfile` | Ruby + bundler (v1.1) |
| `pom.xml` | JVM + maven (v1.2) |
| `build.gradle{,.kts}` | JVM + gradle (v1.2) |
| `Dockerfile` | OCI adapter (v1.1+) |
| `.tool-versions` / `.mise.toml` / `.nvmrc` / `.python-version` / `rust-toolchain.toml` | Read for tool pins (read-only; versionx writes to `versionx.toml` on init) |

Running `versionx init` writes the inferred config to disk.

### 2.5 Workspace root detection
`versionx <cmd>` from anywhere in a repo:
1. Walk up from cwd looking for `versionx.toml` with `[versionx] workspace = true`. If found, that's the root.
2. If not found, walk up for any `versionx.toml`. Nearest wins.
3. If not found, use the git root (`git rev-parse --show-toplevel`).
4. If not in git, use cwd with a warning.

---

## 3. `versionx.lock` — the unified lockfile

### 3.1 Purpose
`versionx.lock` is NOT a replacement for ecosystem lockfiles. It is a **meta-lockfile** that records:
1. Exact resolved runtime versions.
2. A hash of each ecosystem's native lockfile (`pnpm-lock.yaml`, `uv.lock`, `Cargo.lock`, etc.).
3. The resolved SHAs of any linked external repos.
4. A content hash of the effective merged config.

This means a single `versionx.lock` change signals "something that affects reproducibility changed", and CI can cache on it.

### 3.2 Format
TOML (human-readable, diff-friendly).

```toml
# DO NOT EDIT — managed by `versionx sync`
schema_version = "1"
generated_at = "2026-04-18T18:00:00Z"
versionx_version = "1.0.0"
config_hash = "blake3:a7f3b2..."                # BLAKE3 for internal fast keys

[runtimes.node]
version = "20.11.1"
source = "nodejs.org"
sha256 = "e3c6a3..."                            # SHA-256 for supply-chain interop (SBOM/sigstore)

[runtimes.python]
version = "3.12.2"
source = "python-build-standalone"
sha256 = "b1d4f8..."

[ecosystems.node]
package_manager = "pnpm@8.15.0"
native_lockfile = "pnpm-lock.yaml"
native_lockfile_hash = "blake3:9e8a2c..."
resolved_at = "2026-04-18T17:59:58Z"

[ecosystems.python]
package_manager = "uv@0.1.24"
native_lockfile = "uv.lock"
native_lockfile_hash = "blake3:4c7d91..."

[links.shared-ui]
type = "submodule"
commit = "a1b2c3d4e5f6..."
resolved_url = "https://github.com/acme/shared-ui.git"
```

**Hashing policy:** BLAKE3 for internal cache keys (faster, SIMD). SHA-256 for any field that crosses into supply-chain tools (SBOM, sigstore, cosign, GitHub release attestations).

### 3.3 Commit status
- **Committed to git.** Always.
- Modified only by `versionx sync`, `versionx upgrade`, `versionx release`.
- CI uses `versionx verify` to assert the lockfile matches reality.

---

## 4. Local state DB (L4)

SQLite at `$XDG_DATA_HOME/versionx/state.db` (Linux), `~/Library/Application Support/versionx/state.db` (macOS), `%LOCALAPPDATA%\versionx\state.db` (Windows). Created on demand. Never required.

### 4.1 Why SQLite
- Single file, no server, zero install.
- WAL mode for concurrent reads during a write.
- **`rusqlite` + manual migrations** via `rusqlite_migration`. Research showed async SQLite (sqlx) adds no value for embedded single-writer workloads and adds a compile-time live-DB requirement.
- Same schema reused for remote Postgres backend (v1.2+) — compatibility via hand-rolled abstraction.
- Connection pragmas: `journal_mode=WAL; synchronous=NORMAL; busy_timeout=5000; foreign_keys=ON`.

### 4.2 Schema

```sql
-- Migration files in versionx-state/migrations/, applied on open.

CREATE TABLE schema_migrations (
  version INTEGER PRIMARY KEY,
  applied_at TEXT NOT NULL
);

-- Every repo Versionx has touched on this machine
CREATE TABLE repos (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  path TEXT NOT NULL UNIQUE,              -- absolute filesystem path
  name TEXT,                              -- from versionx.toml [versionx].name
  remote_url TEXT,                        -- git remote (if any)
  github_id TEXT,                         -- owner/repo if GitHub
  first_seen TEXT NOT NULL,
  last_synced TEXT,
  config_hash TEXT                        -- current [config_hash] from lockfile
);

CREATE INDEX idx_repos_github ON repos(github_id);

-- Runtime installations (the mise/asdf tracking)
CREATE TABLE runtimes_installed (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  tool TEXT NOT NULL,                     -- "node", "python", etc.
  version TEXT NOT NULL,
  source TEXT NOT NULL,
  install_path TEXT NOT NULL,
  sha256 TEXT,
  installed_at TEXT NOT NULL,
  last_used TEXT,
  UNIQUE(tool, version, source)
);

-- Per-repo runtime bindings (which installed version does this repo use)
CREATE TABLE repo_runtimes (
  repo_id INTEGER NOT NULL REFERENCES repos(id) ON DELETE CASCADE,
  tool TEXT NOT NULL,
  runtime_id INTEGER NOT NULL REFERENCES runtimes_installed(id),
  PRIMARY KEY (repo_id, tool)
);

-- Every sync/release/policy-check run, for audit + TUI history
CREATE TABLE runs (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  repo_id INTEGER REFERENCES repos(id) ON DELETE SET NULL,
  command TEXT NOT NULL,                  -- "sync", "release", etc.
  started_at TEXT NOT NULL,
  ended_at TEXT,
  outcome TEXT,                           -- "success", "failure", "cancelled"
  exit_code INTEGER,
  plan_id TEXT,                           -- blake3 hash; nullable for non-plan ops
  plan_json TEXT,                         -- the ExecutionPlan
  events_zstd BLOB,                       -- Zstd-compressed event stream (binary)
  versionx_version TEXT NOT NULL,
  agent_id TEXT                           -- "versionx-mcp://claude-code/<session>" for MCP-originated ops
);

CREATE INDEX idx_runs_repo ON runs(repo_id, started_at DESC);

-- Plans awaiting apply (plan/apply safety)
CREATE TABLE plans (
  id TEXT PRIMARY KEY,                    -- blake3 hash of plan JSON
  repo_id INTEGER REFERENCES repos(id) ON DELETE CASCADE,
  kind TEXT NOT NULL,                     -- "sync", "release", "upgrade", etc.
  plan_json TEXT NOT NULL,
  pre_requisite_hash TEXT NOT NULL,       -- config_hash at plan creation
  created_at TEXT NOT NULL,
  expires_at TEXT NOT NULL,               -- configurable, default +24h
  created_by TEXT,                        -- user/agent identifier
  approved_by TEXT,                       -- set when approved
  approved_at TEXT,
  applied_at TEXT,                        -- set when applied
  status TEXT NOT NULL                    -- "pending", "approved", "applied", "expired", "rejected"
);

CREATE INDEX idx_plans_status ON plans(status, expires_at);

-- Resolution cache (short-lived, rebuilt freely)
CREATE TABLE resolution_cache (
  key TEXT PRIMARY KEY,                   -- hash of (ecosystem, manifest, pm_version)
  result_json TEXT NOT NULL,
  cached_at TEXT NOT NULL,
  expires_at TEXT NOT NULL
);

-- Known links across repos (for the "virtual monorepo" view)
CREATE TABLE links (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  from_repo_id INTEGER NOT NULL REFERENCES repos(id) ON DELETE CASCADE,
  link_name TEXT NOT NULL,
  link_type TEXT NOT NULL,                -- "submodule" | "subtree" | "virtual" | "ref"
  target_url TEXT NOT NULL,
  target_repo_id INTEGER REFERENCES repos(id),
  path TEXT,
  current_commit TEXT,
  tracked_ref TEXT,
  last_checked TEXT,
  UNIQUE(from_repo_id, link_name)
);

-- Policy evaluations (cached for fast re-check)
CREATE TABLE policy_evaluations (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  run_id INTEGER NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
  policy_id TEXT NOT NULL,
  verdict TEXT NOT NULL,                  -- "allow", "deny", "warn"
  message TEXT,
  evaluated_at TEXT NOT NULL
);

-- Release history
CREATE TABLE releases (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  repo_id INTEGER NOT NULL REFERENCES repos(id),
  package TEXT,                           -- null for repo-wide release
  from_version TEXT,
  to_version TEXT NOT NULL,
  bump_kind TEXT NOT NULL,                -- "major" | "minor" | "patch" | "prerelease"
  strategy TEXT NOT NULL,                 -- "pr-title" | "conventional" | "changesets" | "manual" | "ai"
  tag TEXT,
  commit TEXT,
  released_at TEXT NOT NULL,
  released_by TEXT,
  ai_proposed BOOLEAN DEFAULT 0,
  ai_provider TEXT,                       -- "mcp://claude-code/<session>", "byo-api://anthropic", etc.
  approved_by TEXT
);

CREATE INDEX idx_releases_repo ON releases(repo_id, released_at DESC);

-- Changelog voice samples (for AI prose)
CREATE TABLE changelog_voice (
  repo_id INTEGER NOT NULL REFERENCES repos(id) ON DELETE CASCADE,
  package TEXT,                           -- null = repo-level
  sample_source TEXT NOT NULL,            -- "README", "CHANGELOG", "release-<tag>"
  content TEXT NOT NULL,                  -- the text itself
  token_count INTEGER,
  captured_at TEXT NOT NULL,
  PRIMARY KEY (repo_id, package, sample_source)
);
```

### 4.3 What the state DB enables
- `versionx status --all` — one line per known repo
- `versionx tui` — Dashboard view
- `versionx fleet query "node < 20"` — find all repos violating a constraint
- `versionx history <repo>` — recent runs
- Faster cold starts (resolution cache)
- Voice-aware AI changelogs (per-package sample memory)

### 4.4 What it does NOT do
- Store secrets.
- Store git object data (git is the store).
- Be authoritative for config — always re-reads from disk.
- Phone home with any of this data.

---

## 5. State Integrity & Recovery

### 5.1 Git-backed History (Recovery)
Since `state.db` is a cache, losing it shouldn't lose history permanently.
- **`history.versionx.json`**: On every successful release or significant sync, Versionx commits a summary of the run to a hidden git branch `refs/versionx/history`.
- **Syncing**: `versionx state restore` fetches this branch and re-populates the SQLite DB.
- **Pruning**: Only the last 100 runs are kept in the git-backed history to avoid repo bloat; full history stays in SQLite/Postgres.

### 5.2 Compression
The `events_zstd` column uses **Zstd** (Dictionary-mode where possible) to compress structured JSON events.
- **Target**: 10x-20x compression vs raw JSON.
- **Decompression**: Only happens when viewing a specific run in the TUI or Web UI.

### 5.3 Repair Mechanism
`versionx state repair` performs:
1. **Consistency Check**: Verifies all `repo_id` references exist.
2. **Filesystem Audit**: Checks if absolute paths in `repos` still exist; updates or marks as `lost`.
3. **Lockfile Re-sync**: Re-reads `versionx.lock` for all known repos to ensure `config_hash` is current.
4. **Zstd Integrity**: Validates that compressed blobs can be decompressed.
5. **Plan expiry cleanup**: Deletes `plans` rows where `status = "expired"` and `expires_at < now() - 30d`.

---

## 6. Remote state backend (L5, v1.2+)

Same schema, Postgres instead of SQLite. Opt-in via:

```toml
[advanced]
state_backend = "postgres://user:pass@host/db"
# or reference a named env var for secret safety
state_backend_env = "VERSIONX_STATE_URL"
```

### 6.1 Use cases
- CI reading shared fleet state.
- Multiple devs sharing a team dashboard.
- Future managed service storing per-org state.

### 6.2 Consistency model
- Each repo's row has a `last_writer` and optimistic-lock version column.
- Writes use `INSERT ... ON CONFLICT` with version checks.
- Leaves (no intra-repo dependents) release first; roots last.

### 6.3 Migration path
- `versionx state migrate --to postgres://...` — one-shot import of local DB.
- No lock-in: `versionx state export --format sqlite` dumps back to a file.

---

## 7. Policy files

Full details in `07-policy-engine.md`. From a state perspective:

- Policy files live in `.versionx/policies/` per repo, or in a remote registry referenced by URL.
- Fleet-inherited policies pin to content SHA in a separate `versionx.policy.lock` — central updates don't silently break CI across the fleet.
- Policies are **stateless inputs** — they are files, not DB rows.
- Evaluation results are cached in `policy_evaluations` with invalidation on config or policy change.

---

## 8. Changeset and waiver files

Full details in `05-release-orchestration.md` and `07-policy-engine.md`. From a state perspective:

- Contributors write `.versionx/changesets/<name>.md` files describing intent.
- Waivers live in `.versionx/waivers.toml` with **mandatory `expires_at`** (org can override).
- Files are committed to git; they are the source of truth.
- At release time, Versionx reads changesets, aggregates, proposes bumps, then deletes (or archives) them.
- The `releases` table records the outcome.

---

## 9. Progressive disclosure — operational details

### 9.1 Opting INTO higher layers
| Goal | Command |
|---|---|
| Add a lockfile | `versionx sync` (auto-creates) |
| Enable state DB | First interactive command, or `versionx daemon start` |
| Add policies | Create a file in `.versionx/policies/` or `versionx policy init` |
| Connect fleet | `versionx fleet init --remote <url>` |
| Configure AI | `versionx ai configure` (walks BYO-API-key or MCP client setup) |

### 9.2 Opting OUT
| Goal | How |
|---|---|
| No lockfile (not recommended) | `[advanced] lockfile = false` |
| No state DB | `--no-state` flag, or `VERSIONX_NO_STATE=1` env, or delete the file |
| No daemon | `[advanced] daemon = "never"` |

### 9.3 Invariants preserved across layers
- Config + lockfile are always enough to reproduce an install.
- State DB + remote state are always caches/indexes; dropping them is safe.
- Policies can only *deny* or *warn*; they never mutate state silently.
- AI suggestions are always proposals requiring explicit human or programmatic approval.
- **Versionx never phones home.** No telemetry, ever.

---

## 10. Backwards compatibility & migration

- `schema_version` in config and lockfile gets bumped on breaking changes.
- Versionx always reads older schemas; writing always produces the current schema.
- `versionx migrate` is an explicit command that upgrades config files with diff preview.
- `versionx import --from mise` / `--from asdf` reads `.mise.toml` / `.tool-versions` and emits `versionx.toml`.
- State DB migrations apply automatically with version tracking; downgrades are refused (user must export/reimport).

---

## 11. Implementation notes

- **Lockfile hashing**: BLAKE3 for internal fast keys (`config_hash`, cache keys). SHA-256 for SBOM/sigstore interop fields.
- **Config inheritance semantics**: leaf-wins (replace) by default for arrays. Users opt into merge via `[inherit] append = [...]`. Sealed fleet policies cannot be disabled downstream.
- **Env var interpolation**: load-time by default; `"${VAR@lazy}"` opts into adapter-invocation-time resolution.
- **Plan TTL**: configurable per-repo via `[release] plan_ttl = "24h"`; default 24h; allowed range 1h–30d.
- **Remote state auth (v1.2+)**: bearer token + env var. OAuth CIMD deferred to remote MCP rollout.
