# 01 — Architecture Overview

## Scope
High-level architecture: how the codebase is organized, what processes run where, how data flows between them, and what transport surfaces exist. This file is the map. Subsystem files go deep on specific boxes.

## Contract
After reading this file you should be able to answer: "If a user types `versionx sync`, what crates execute, what files get read/written, what daemons get contacted, and what events fire?"

---

## 1. Crate layout

Single Cargo workspace. Every box is a published crate.

```
versionx/
├── Cargo.toml                      # workspace root
├── crates/
│   ├── versionx-core/              # THE library. Everything else depends on this.
│   ├── versionx-cli/               # `versionx` binary (clap-based CLI)
│   ├── versionx-tui/               # `versionx tui` (ratatui)
│   ├── versionx-daemon/            # `versiond` long-running process
│   ├── versionx-web/               # slim web UI (axum + htmx)
│   ├── versionx-mcp/               # MCP server (built on rmcp official SDK)
│   │
│   ├── versionx-adapters/          # meta-crate re-exporting all adapters
│   ├── versionx-adapter-trait/     # the PackageManagerAdapter trait + test kit
│   ├── versionx-adapter-node/      # npm/pnpm/yarn
│   ├── versionx-adapter-python/    # pip/uv/poetry
│   ├── versionx-adapter-rust/      # cargo
│   ├── versionx-adapter-go/        # go modules (Tier 2, v1.1)
│   ├── versionx-adapter-ruby/      # bundler (Tier 2, v1.1)
│   ├── versionx-adapter-jvm/       # maven/gradle (Tier 2, v1.2)
│   ├── versionx-adapter-oci/       # container images (v1.1+)
│   │
│   ├── versionx-runtime-trait/     # RuntimeInstaller trait
│   ├── versionx-runtime-node/      # installs Node + shims pnpm/yarn directly (no corepack)
│   ├── versionx-runtime-python/    # installs CPython via python-build-standalone
│   ├── versionx-runtime-rust/      # wraps rustup (never sets RUSTC)
│   ├── versionx-runtime-go/
│   ├── versionx-runtime-jvm/       # Temurin default via foojay Disco API
│   ├── versionx-runtime-ruby/      # rv prebuilt primary, ruby-build fallback
│   │
│   ├── versionx-shim/              # tiny static shim binary (Volta-style argv[0] dispatch)
│   │
│   ├── versionx-config/            # TOML schema, validation, migration (toml_edit)
│   ├── versionx-lockfile/          # versionx.lock read/write
│   ├── versionx-state/             # SQLite state DB (rusqlite + WAL), Postgres backend (remote, v1.2)
│   ├── versionx-policy/            # policy DSL parser + Luau evaluator (mlua, sandboxed)
│   ├── versionx-release/           # SemVer, PR-title parser, conventional commits, changesets
│   ├── versionx-tasks/             # native task runner (v1.0: topo+exec; v1.2: local cache; v2.0: remote)
│   ├── versionx-multirepo/         # submodule/subtree/virtual-monorepo handlers
│   ├── versionx-git/               # git operations (gix for reads, git2 for writes)
│   ├── versionx-github/            # GitHub API client (octocrab wrapper, ≥0.49.7)
│   ├── versionx-events/            # structured event bus (tracing + broadcast channel)
│   └── versionx-sdk/               # public Rust SDK (re-exports from core)
└── xtask/                          # cargo-xtask for release, packaging, etc.
```

Note: `versionx-gh-app` is **not** in v1.0. A hosted GitHub App is deferred; the same codebase already runs in GitHub Actions via reusable workflows (see `08-github-integration.md`).

### Dependency rules (enforced by `cargo-deny` + architecture tests)
- `versionx-core` depends only on: adapters, runtimes, config, lockfile, state, policy, release, tasks, multirepo, git, github, events.
- `versionx-cli` / `versionx-tui` / `versionx-daemon` / `versionx-web` / `versionx-mcp` depend on `versionx-core`. They **never** import adapters or state directly.
- Adapters depend only on `versionx-adapter-trait` and common utility crates. Adapters **never** depend on `versionx-core` (one-way dependency).
- No frontend crate depends on another frontend crate.

This produces the clean core/frontend split that keeps open-core monetization viable later without architectural surgery.

---

## 2. Process model

Three possible execution modes. The user never picks explicitly; the CLI figures it out.

### Mode A — Direct (no daemon)
```
user → versionx (CLI binary) → versionx-core → adapters → ecosystem tools
                       ↘ versionx-state (SQLite)
```
Used for: one-off commands in CI (always direct), `--no-daemon` flag, very-first-run.
Startup cost: ~20ms.

### Mode B — Daemon-backed (default for interactive shells)
```
user → versionx (CLI binary) ──IPC──→ versiond (daemon) → versionx-core → adapters
                                               ↘ versionx-state
                                               ↘ file watcher
                                               ↘ event bus
```
Used for: interactive dev loops, TUI, repeated commands, file-watch triggers.

**Lifecycle:**
- **Shell-hook activation.** `eval "$(versionx activate bash)"` in rc file starts the daemon on login and keeps it alive for the session.
- **Per-user.** Socket at `$XDG_RUNTIME_DIR/versionx/daemon.sock` (Linux), `~/Library/Application Support/versionx/daemon.sock` (macOS), `\\.\pipe\versionx-daemon-<user>` (Windows).
- **No system-wide shared daemon in v1.0.** Each user runs their own. Multi-user hosts can add this later via systemd/launchd units.

**IPC Security:**
- **Linux/macOS**: Unix Domain Sockets. Permissions: **0600**, owner-only access. No auth token needed locally — trust the UID.
- **Windows**: Named Pipes secured via SDDL restricting to the user's SID.
- **Protocol**: JSON-RPC 2.0 over length-prefixed framing (4-byte BE length header).
- **Streaming**: Supports server-sent notifications during long-running calls (progress, partial results, elicitation prompts).

### Mode C — Daemon + web UI + MCP (local only in v1.0)
```
user browser ──HTTP──→ versiond (axum) → versionx-core
AI agent    ──MCP───→ versiond            ↓
CLI         ──IPC───→ versiond          (same)
```
Daemon exposes three transports concurrently on loopback. No auth in v1.0 — trust loopback + OS user. Remote-exposed HTTP + OAuth are a v1.2+ question.

MCP supports both stdio (spawn-as-child) and loopback HTTP; stdio is primary for Claude Code/Cursor/Codex integrations. Built on the official `rmcp` Rust SDK.

---

## 3. Library Boundary: `versionx-core`

`versionx-core` is the "brain." It is a stateless-intent engine that produces and applies plans.

**The Contract:**
1. **Stateless Logic**: Core computes what *should* happen based on inputs.
2. **Intent-driven**: Every mutation is a `Plan`. Core produces `Plan`, UI/agent approves `Plan`, Core applies `Plan`.
3. **Internal Registry**: Core maintains the registry of available adapters and runtimes.
4. **Event Source**: All operations stream structured events to a shared bus.

Frontends (`versionx-cli`, `versionx-mcp`, etc.) are prohibited from:
- Directly calling `git` or ecosystem tools.
- Writing to the state DB without going through `versionx-state`.
- Re-implementing resolution or pinning logic.

---

## 4. Data flow: `versionx sync` worked example

Annotated trace of a single command, end to end, in daemon-backed mode.

1. **CLI entrypoint** (`versionx-cli`): parse args with clap. Resolve working directory. Detect daemon socket.
2. **IPC**: send `{"method": "sync", "params": {"cwd": "..."}}` to `versiond`.
3. **Daemon** (`versionx-daemon`): receives RPC, spawns task, calls `versionx_core::commands::sync(SyncRequest { cwd })`.
4. **Core** (`versionx-core::commands::sync`):
   a. `versionx-config::load(cwd)` → walks up for `versionx.toml` with `workspace = true`, or git root, or cwd. Loads `.env` and `.env.local` into the env scope.
   b. `versionx-state::open_or_create(state_path)` → SQLite connection at `$XDG_DATA_HOME/versionx/state.db`.
   c. `versionx-policy::load_applicable(config, state)` → gets any policy files in scope.
   d. For each ecosystem declared in config:
      - `versionx-adapters::for_ecosystem(eco).plan(config.eco)` → returns a `PlanStep` list.
   e. For each runtime declared:
      - `versionx-runtime::for_runtime(rt).ensure_installed(version)` → returns install result or no-op.
   f. Aggregate all plans → single `ExecutionPlan` with Blake3 hash + configurable TTL.
   g. Run `versionx-policy::evaluate(plan, policies)` → returns allow/deny/warn list.
   h. If any deny: return error with structured violations.
   i. Execute plan: adapters run in topological order, events stream on the bus.
   j. `versionx-lockfile::write(new_state)` → atomically rewrites `versionx.lock`.
   k. `versionx-state::record_run(plan, outcome)` → stores run in SQLite for audit/TUI.
5. **Response**: daemon streams events to CLI as they happen (progress bars, warnings). Final result flows back as a `SyncResponse`.
6. **CLI**: renders results. If `--json`, emits one JSON object. Otherwise pretty output.

Every step emits structured tracing spans. With `VERSIONX_LOG=debug` you see the whole thing. With `--output json-events` you get a stream of JSON events suitable for AI agent consumption.

---

## 5. Transport surfaces (the faces of Versionx)

Every user-visible capability lives in `versionx-core`. The transports are thin adapters.

### 5.1 CLI (`versionx`)
- `clap` derive macros. One subcommand per verb.
- Every command supports `--output {human,json,ndjson}`, `--plan` (don't execute, emit plan), `--apply <plan.json>` (execute pre-approved plan).
- `versionx --help-json` emits the full command tree as JSON for MCP/agent consumption.
- Exit codes: 0 ok, 1 user error, 2 config error, 3 policy violation, 4 network/IO, 10+ subsystem-specific.

### 5.2 TUI (`versionx tui`)
- `ratatui` + `crossterm`.
- Views: Dashboard (all tracked repos), Repo Detail, Release Planner, Policy Inspector, Run Log.
- All mutating actions go through the same `versionx-core` calls the CLI uses, so there's no "TUI logic" — just rendering + input.

### 5.3 Daemon RPCs
- JSON-RPC 2.0 over UDS / named pipe.
- Methods map 1:1 to `versionx-core` public functions.
- Streaming methods (like `sync`, `release`) emit notifications during execution.
- Schema published as an OpenRPC document at `$XDG_DATA_HOME/versionx/schema/rpc.json`.

### 5.4 HTTP API (web UI + local automation)
- `axum` on loopback only in v1.0. OpenAPI spec auto-generated via `aide`.
- REST-ish: `GET /repos`, `POST /repos/{id}/sync`, `GET /plans/{id}`, `POST /plans/{id}/apply`.
- SSE endpoint `/events` for streaming run progress.
- **No auth in v1.0** (loopback only). Bearer + OAuth CIMD planned for v1.2+ remote mode.

### 5.5 MCP (AI agents)
- Built on the official `rmcp` Rust SDK (≥1.5).
- Two transports: **stdio** (primary — spawn-as-child for Claude Code/Cursor/Codex) and loopback HTTP (for persistent sessions).
- **Tool count capped at ~10**, workflow-shaped: `plan`/`apply`/`bump`/`inspect`/`list`. Research shows tool-count discipline materially improves agent accuracy.
- Every mutating tool has `_plan` and `_apply` variants plus optional `_propose_and_apply` using elicitation (progressive enhancement; falls back gracefully).
- Resources (`versionx://config`, `versionx://state/repos`, etc.) are published but always mirrored as tools too, because client support for resources is uneven.
- Prompts shipped: `propose_release`, `audit_dependency_freshness`, `remediate_policy_violation`.
- **No bundled LLM.** versionx never calls a model itself via MCP — it serves context and accepts plans; the agent's LLM does the reasoning.

### 5.6 GitHub integration (v1.0)
Reusable GitHub Actions only (`acme/versionx-install-action`, `-sync-action`, `-release-action`, `-policy-action`). They shell out to the `versionx` binary and post PR comments / check runs from within the workflow. A hosted GitHub App (which would react to webhooks without per-repo workflows) is **deferred past v1.0**. See `08-github-integration.md`.

---

## 6. Storage & filesystem layout

### Per-repo (committed to git)
```
<repo>/
├── versionx.toml             # primary config
├── versionx.lock             # resolved state (committed)
├── .versionx/
│   ├── policies/             # optional repo-scoped policies
│   │   └── *.policy.toml
│   ├── changesets/           # optional changeset-style intent files
│   │   └── <kebab-name>.md
│   └── waivers.toml          # policy waivers (mandatory expiry)
└── .versionxignore           # optional, gitignore-syntax
```

### Per-user (not in git) — XDG-compliant
```
Linux:
  $XDG_CONFIG_HOME/versionx/config.toml        # global user defaults
  $XDG_DATA_HOME/versionx/state.db             # SQLite, local state
  $XDG_DATA_HOME/versionx/runtimes/            # installed Node/Python/Rust/etc.
  $XDG_CACHE_HOME/versionx/                    # resolution cache, download cache
  $XDG_STATE_HOME/versionx/logs/
  $XDG_RUNTIME_DIR/versionx/daemon.sock
  $XDG_DATA_HOME/versionx/shims/               # shim dir on PATH

macOS:
  ~/Library/Application Support/versionx/
  ~/Library/Caches/versionx/
  ~/Library/Logs/versionx/

Windows:
  %LOCALAPPDATA%\versionx\
  %LOCALAPPDATA%\versionx\Cache\
```

`$VERSIONX_HOME` env var overrides all of the above to a single dir for users who want the old `~/.vers`-style layout.

### Per-fleet (remote, v1.2+)
- Postgres database with same schema as local SQLite.
- Fleet config lives in a **dedicated ops repo** (e.g., `acme/platform-ops`) containing `versionx-fleet.toml`, shared policies, and release sets.
- Members reference via `[policies] inherit = ["fleet://acme-platform/baseline"]`.

---

## 7. Event bus

All subsystems publish to a single in-process event bus (`tokio::sync::broadcast`). Events are typed (strongly enum'd) and structured.

Subscribers:
- Tracing layer (writes to stderr or file)
- OTLP exporter (if user configured an endpoint — versionx never defaults one)
- Daemon RPC streaming (forwards to active CLI/TUI/web/MCP clients)
- State DB writer (persists run history, Zstd-compressed)

Event categories:
- `config.*` — config loaded, validated, migrated
- `adapter.*` — adapter invocation, output, completion
- `runtime.*` — download, extract, shim install
- `policy.*` — evaluation, violation, warning
- `release.*` — plan, bump, tag, publish
- `git.*` — fetch, push, subtree sync
- `state.*` — write, query, migration
- `mcp.*` — transport-specific

This is load-bearing for observability and for the AI agent story: the MCP server streams these events as progress notifications, so an agent can watch a long-running operation in real time.

---

## 8. Concurrency model

- `tokio` everywhere.
- Adapters run concurrently when dependency-free (DAG topological sort via `petgraph`).
- Bounded concurrency: `--jobs N` flag, defaults to `num_cpus::get().min(8)`.
- Per-ecosystem locks: never two `npm install` in the same dir simultaneously.
- Global state DB writes go through a single writer actor to avoid SQLite WAL contention.

---

## 9. Error handling

- `thiserror` for structured error types at crate boundaries.
- `anyhow` for application-level error chaining in frontends only (never in `versionx-core`).
- Every error carries: kind (enum), context (human), machine-readable code (for `--json`), and suggestion (for the CLI UX).
- User-facing errors always end with an actionable next step. No dead ends.

---

## 10. Cross-platform notes

- Path handling: `camino` (UTF-8 paths) for user-facing paths, `std::path::Path` only at syscall boundaries.
- Shell execution: `tokio::process::Command` wrapped in a `versionx::proc` helper with env scrubbing (no `NODE_OPTIONS`/`PYTHONPATH`/`RUSTC_*` leakage unless whitelisted).
- PATH manipulation for shims: `$XDG_DATA_HOME/versionx/shims` (or platform equivalent) prepended once; shims dispatch to the right binary version.
- **Windows shim strategy**: Volta-style minimal Rust trampoline (`versionx-shim.exe`) copied (or hardlinked where possible) per tool. Dispatches via `argv[0]` lookup against an mmap'd PATH cache. Target <1ms cold overhead. Works without Developer Mode or Admin.
- Line endings: `versionx.lock` always LF; `versionx.toml` respects existing.

---

## 11. What lives where (quick lookup)

| If you're building... | Start in crate | See spec file |
|---|---|---|
| A new ecosystem adapter | `versionx-adapter-<name>` | `03-ecosystem-adapters.md` |
| Runtime installer for a new language | `versionx-runtime-<name>` | `04-runtime-toolchain-mgmt.md` |
| A new CLI subcommand | `versionx-cli` + `versionx-core` | follow existing patterns |
| Release logic | `versionx-release` | `05-release-orchestration.md` |
| Task runner | `versionx-tasks` | `05-release-orchestration.md` (integration), `10-mvp-and-roadmap.md` (phasing) |
| Multi-repo feature | `versionx-multirepo` | `06-multi-repo-and-monorepo.md` |
| Policy rule | `versionx-policy` | `07-policy-engine.md` |
| GitHub Action workflow | `acme/versionx-actions` (separate repo) | `08-github-integration.md` |
| MCP tool | `versionx-mcp` | `09-programmatic-and-ai-api.md` |
