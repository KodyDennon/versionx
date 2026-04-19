# 09 — Programmatic & AI API

## Scope
How humans and AI agents drive Versionx programmatically. Four surfaces: a **Rust SDK**, a **JSON-RPC daemon protocol**, an **HTTP/REST API**, and a **Model Context Protocol (MCP) server**. All four are thin frontends over `versionx-core`.

## Contract
After reading this file you should be able to: call any Versionx capability from a Rust program, a shell script (via JSON output), a web service (via loopback HTTP), or an AI agent (via MCP) — and understand the plan/apply safety model that makes AI-driven workflows safe.

---

## 1. The unified capability surface

Every Versionx capability exists as a function in `versionx-core`. The four transport surfaces are code-generated or hand-maintained translations of those functions:

```
versionx-core (source of truth)
├─ Rust SDK        (direct import)
├─ JSON-RPC daemon (IPC, 1:1 with core)
├─ HTTP / REST     (local API, REST-ified)
└─ MCP server      (AI-agent-optimized, prompts + resources)
```

No capability exists in one surface but not another. Parity is enforced by a code-gen step that reads trait impls and generates transport layer boilerplate (with hand-overrides allowed for idiomatic REST shapes).

---

## 2. The plan/apply model — the safety spine

This is the single most important design in this doc for AI-agent safety. Every mutating operation in Versionx is a two-phase:

### 2.1 Phase 1: Plan
```
versionx <verb> --plan-only [--output plan.json]
```
Or programmatically:
```rust
let plan = versionx_core::commands::propose_sync(&ws).await?;
```

**Plan IDs & hashes:**
- **Algorithm**: BLAKE3 (fast, keyed, parallelizable, SIMD-accelerated).
- **ID generation**: `blake3::hash(canonical_json_representation_of_plan)`.
- **Integrity**: Every plan includes a `pre_requisite_hash` (the `config_hash` from the current lockfile at plan time).
- **Expiration**: Plans carry an `expires_at` timestamp; Core refuses to apply if `now() > expires_at`.
- **TTL**: configurable per repo via `[release.plan_ttl]` in `versionx.toml`, default 24h, allowed range 1h–30d. `--ttl <duration>` overrides per-command.

### 2.2 Phase 2: Apply
```
versionx <verb> --apply plan.json
```
Or:
```rust
let outcome = versionx_core::commands::apply_plan(&plan).await?;
```

Apply:
- Verifies the plan's hash matches current state prerequisites.
- Executes the steps.
- Refuses to apply if the plan is stale (underlying state changed).

### 2.3 Why this matters for AI
An AI agent proposes a plan → human (or a more-trusted system) reviews JSON → approves → applies. The AI never needs write permissions; the applier does. This composes with GitHub-style approval flows, MCP `elicit` workflows, CI gating, and policy checks.

Every mutating MCP tool has a `_plan` variant that produces a plan and an `_apply` variant that consumes one. For interactive agents with elicitation support, there's also a convenience `_propose_and_apply` that emits the plan into the agent's context for review before applying.

---

## 3. Rust SDK (`versionx-sdk`)

### 3.1 Scope
Public Rust library for embedding Versionx in other tools or writing custom frontends.

### 3.2 Example
```rust
use versionx_sdk::{Versionx, SyncOptions};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let versionx = Versionx::open(".").await?;
    let plan = versionx.plan_sync(SyncOptions::default()).await?;
    println!("Would install {} packages", plan.summary.install_count);
    let outcome = versionx.apply(plan).await?;
    println!("Installed in {:?}", outcome.duration);
    Ok(())
}
```

### 3.3 API shape
- `Versionx` is a handle wrapping a workspace.
- Methods mirror CLI verbs: `plan_sync`, `apply_plan`, `propose_release`, `approve_release`, `fleet_query`, etc.
- All async (tokio).
- Errors are typed (`VersionxError` enum via `thiserror`).
- Stability: semver-committed from 1.0.

### 3.4 Stream events
```rust
let mut events = versionx.events().subscribe();
while let Some(evt) = events.recv().await {
    println!("{:?}", evt);
}
```

**Event stream structure:**
Every event is a JSON object with at least:
- `timestamp`: RFC3339.
- `id`: Unique event ID.
- `kind`: Dot-separated category (e.g., `adapter.exec.stdout`).
- `level`: `trace` | `debug` | `info` | `warn` | `error`.
- `data`: Payload specific to the kind.

**Storage**: For the `runs` table, the stream is collected and compressed via **Zstd** into a single binary blob.

---

## 4. JSON-RPC daemon protocol

### 4.1 Transport
- Unix domain socket (`$XDG_RUNTIME_DIR/versionx/daemon.sock`) on Linux; Application Support path on macOS.
- Named pipe on Windows (`\\.\pipe\versionx-daemon-<user>`).
- Length-prefixed JSON frames (4-byte BE length + payload).
- **0600 permissions** / SDDL restricting to owner SID. Trust the OS; no auth token locally.

### 4.2 Protocol: JSON-RPC 2.0
```json
→ {"jsonrpc": "2.0", "id": 1, "method": "sync", "params": {"cwd": "/path/to/repo"}}
← {"jsonrpc": "2.0", "id": 1, "result": {"plan_id": "...", "status": "queued"}}
← {"jsonrpc": "2.0", "method": "event", "params": {"kind": "adapter.exec", "...": "..."}}
← {"jsonrpc": "2.0", "method": "event", "params": {"kind": "sync.complete", "...": "..."}}
```

Long-running methods stream server-sent notifications on the same connection until the final `result` message.

### 4.3 Methods
Published as an OpenRPC schema at `$XDG_DATA_HOME/versionx/schema/rpc.json`. Generated from `versionx-core` annotations.

Method naming: `<domain>.<verb>`:
- `config.load`, `config.save`, `config.validate`
- `sync.plan`, `sync.apply`
- `release.propose`, `release.approve`, `release.apply`
- `runtime.install`, `runtime.uninstall`, `runtime.list`
- `links.sync`, `links.update`
- `policy.evaluate`, `policy.test`
- `fleet.query`, `fleet.sync`
- `state.query`, `state.history`

### 4.4 Auth (local-only in v1.0)
Connections to the socket are trusted (UDS 0600 permissions restrict to the user). No additional auth layer.

### 4.5 Remote auth (post-v1.0)
If daemon ever listens on TCP:
- Bearer token from `$XDG_CONFIG_HOME/versionx/daemon.token` (created on first remote-listen).
- Optional mTLS via `--tls-cert` / `--tls-key`.
- For MCP: OAuth 2.1 + Client ID Metadata Documents (CIMD) per 2025-11-25 spec.

---

## 5. HTTP / REST API

### 5.1 Framework
`axum` on **loopback only in v1.0**. OpenAPI spec auto-generated via `aide`.

### 5.2 Shape (abbreviated)
```
GET    /health
GET    /repos                         → list known repos
GET    /repos/{id}                    → repo detail
POST   /repos/{id}/sync:plan          → create a sync plan
POST   /repos/{id}/sync:apply         → apply a plan
GET    /plans/{id}                    → get a plan
POST   /releases:propose              → propose release
POST   /releases/{id}:approve         → approve release plan
POST   /releases/{id}:apply           → apply (execute) release
GET    /fleet/query?q=...             → cross-repo query
GET    /events                        → SSE stream of events
GET    /policies                      → list active policies
POST   /policies/{id}:test            → test a policy against current state
```

### 5.3 Why `:verb` in paths
The colon separator is Google's AIP-136 recommendation for non-REST actions.

### 5.4 Auth (local-only in v1.0)
No auth on loopback. Trust the OS user.

### 5.5 Pagination
Cursor-based for list endpoints. `next_cursor` in response body; pass as `?cursor=` on next call.

### 5.6 Remote mode (post-v1.0)
Remote exposure would require: bearer tokens, rate limiting, per-route scopes, CORS discipline. Not in v1.0.

---

## 6. MCP server — the AI-native face

### 6.1 Why MCP
AI agents (Claude Code, Codex, Cursor, Qwen, future agents) speak MCP. Exposing Versionx as an MCP server makes every capability available to agents with:
- Typed tool signatures.
- Structured results (for agent reasoning).
- Resources (readable data like config files, state queries).
- Prompts (preloaded workflows).

### 6.2 Server architecture
```
versionx-mcp crate implements MCP via the official rmcp Rust SDK (≥1.5).
 └─ Runs inside versiond (if daemon running) OR as standalone `versionx mcp serve`
Transports: stdio (primary) + loopback HTTP (for persistent sessions)
```

**No bundled LLM.** The MCP server exposes context and accepts commands; the calling agent's LLM does reasoning. This is the architectural core of "AI as client, not component."

### 6.3 Tool budget & design

**Hard cap: ≤10 tools.** Research on MCP tool design found that tool-count discipline materially improves agent accuracy. GitHub Copilot cut from 40 → 13 with measurable gains; Block went 30+ → 2 after rebuilding their server three times.

Tool families (each has optional `_plan`/`_apply` variants for mutating ops):

**Read-only (single tool each):**
- `versionx_status` — overall state
- `versionx_list` — unified list (repos, packages, outdated, runtimes) with typed filters
- `versionx_inspect` — deep inspection of a specific thing (plan, policy violation, release)
- `versionx_query_fleet` — cross-repo query

**Mutating (each has `_plan` + `_apply`):**
- `versionx_sync_*` — install/update
- `versionx_release_*` — propose / bump / publish
- `versionx_upgrade_*` — bump deps
- `versionx_runtime_*` — install/uninstall toolchain
- `versionx_links_*` — submodule/subtree/ref operations

**Write-safe (no plan/apply split — these are just file writes):**
- `versionx_write` — one write-verb router taking `kind: "changeset" | "policy" | "waiver"` + payload

Total: **10 tools**, each with descriptive parameter schemas (research found 97.1% of MCP tool descriptions have quality issues — we take descriptions seriously).

### 6.4 Output discipline

- Every response includes **both** human-readable text content **and** `structuredContent` with a schema — older clients handle text, newer clients parse structured.
- Large outputs (plans, diffs, logs) return a **summary text** + `resource_link` URI; the agent fetches the full payload via a resource read if needed.
- Verbose mode via explicit `verbosity` param (`"summary" | "detail" | "full"`), default `summary`.

### 6.5 Resources

- `versionx://config` — current `versionx.toml` contents.
- `versionx://lockfile` — `versionx.lock`.
- `versionx://state/repos` — paginated list of known repos.
- `versionx://state/runs/{run_id}` — run detail.
- `versionx://plans/{plan_id}` — plan JSON.
- `versionx://policies/{policy_id}` — policy file.

Resources are read-only URIs the agent can reference without burning tokens re-fetching. **But every resource is also mirrored as a tool** because Cursor and many other clients don't surface resources well — don't assume the client can read them.

### 6.6 Prompts

Preloaded workflows agents can invoke as slash commands (primarily Claude Code; Cursor support uneven):
- `propose_release` — "Review diffs since last release and propose a release plan."
- `audit_dependency_freshness` — "Find all outdated deps and categorize by risk."
- `remediate_policy_violation` — "Given this violation, suggest the minimal fix."

Shipping 2-3 high-value prompts; not the primary UX.

### 6.7 Elicitation (progressive enhancement)

For MCP clients that support `elicit` (Claude Code, VS Code Copilot, partially Claude Desktop):
- `versionx_release_propose_and_apply` computes plan → emits `elicit` request with plan diff → waits for approval → applies.

**Fallback**: if the client doesn't support elicitation (Cursor's support is inconsistent, Cline lacks it), the tool returns the plan for review and requires a second explicit tool call to apply. No feature is gated on elicitation.

### 6.8 Sampling — deliberately unused for core flows

MCP sampling (`sampling/createMessage`) lets the server ask the client's LLM to reason. Client support is uneven (Claude Desktop best, Cursor partial, Cline open feature request). We **do not** put any core Versionx feature behind sampling — it's unreliable. Voice-aware changelog generation uses the inverse pattern: the agent pulls context from versionx and does its own reasoning.

### 6.9 Safety model for AI

Three defenses:

1. **Plan/apply split**: AI can freely produce plans; applying requires either human approval or a machine-identity token with apply scope. Default MCP config: AI gets `plan` scope only.
2. **Policy gate**: every plan runs through policy before apply. AI's plans are no more trusted than a human's.
3. **Audit log**: every MCP tool call and every plan-apply pair is recorded in `state.runs` with the agent identity (`versionx-mcp://<client-name>/<session-id>`).

**Identity caveat**: MCP's `clientInfo` is client-self-reported and trivially spoofed. We log it for audit but never use it for authorization. OAuth 2.1 + CIMD are the real-identity path, deferred to remote MCP (v1.2+).

### 6.10 Tool output sanitization (prompt injection defense)

Commit messages, changelogs, ticket descriptions, dependency README snippets — anything user-controlled that flows through versionx's MCP responses — is:
- Wrapped in fenced blocks with explicit `Untrusted content from <source>:` markers.
- Long content truncated with `resource_link` to full payload.
- Never interpolated unfenced into tool description fields or system messages.

This is the OWASP LLM Top 10 #1 risk; we take it seriously after Supabase Cursor agent's 2025 token-leak incident and the MCPTox benchmark showing 72.8% attack success on stock servers.

---

## 7. Output formats — structured everywhere

Every CLI command supports:
- **Default**: human-readable, colored, paged.
- **`--output json`**: single JSON object on stdout, no colors, no decoration.
- **`--output ndjson`**: newline-delimited JSON events (for streaming).
- **`--output template --template <file>`**: Tera template rendering (user-supplied).

`versionx --help-json` emits the full command tree as JSON so MCP can introspect capabilities without parsing `--help` text.

Example JSON shape for `versionx status --output json`:
```json
{
  "schema_version": "1",
  "versionx_version": "1.0.0",
  "workspace": {
    "root": "/home/k/repos/acme",
    "packages": [...],
    "runtimes": {"node": "22.12.0", "python": "3.12.2"},
    "links": [...]
  },
  "state": {
    "lockfile_present": true,
    "lockfile_synced": true,
    "outstanding_changesets": 2,
    "policy_warnings": 0,
    "policy_denies": 0
  }
}
```

JSON shapes are versioned with a `schema_version` field; breaking changes require a schema bump.

---

## 8. Example AI agent interactions

### 8.1 Agent: "Update this project's dependencies safely"
```
1. Agent calls versionx_status → sees current state.
2. Agent calls versionx_list { kind: "outdated" } → gets list with severity annotations.
3. Agent calls versionx_upgrade_plan with filter rules.
4. Agent reads back versionx_inspect { kind: "plan", id } for human-readable summary.
5. Agent emits the plan to the user (via MCP elicit, or returns it and asks for approval).
6. User approves.
7. Agent calls versionx_upgrade_apply { plan_id }.
8. Agent calls versionx_status again → verifies no new violations.
9. Agent reports outcome.
```

### 8.2 Agent: "Prepare a release for this monorepo"
```
1. Agent calls versionx_status.
2. Agent reads versionx://state/releases (last release).
3. Agent reads git log since last release (via its own git tools).
4. Agent calls versionx_release_plan → gets structured context including voice samples.
5. Agent's LLM generates voice-aware changelog prose.
6. Agent calls versionx_write { kind: "changelog_draft", plan_id, body: "..." }.
7. Agent presents final plan (diff + prose) to user.
8. User approves → agent calls versionx_release_apply { plan_id }.
```

### 8.3 Agent: "Add a new policy across the fleet"
```
1. User describes the rule in natural language.
2. Agent drafts a policy.toml.
3. Agent calls versionx_inspect { kind: "policy_test", draft, fixture: "<synthetic>" }.
4. Agent iterates until test passes.
5. Agent opens a PR in the fleet's ops repo with the file.
6. Human reviews and merges; fleet members pick it up on next sync.
```

---

## 9. Client libraries

### 9.1 Python
Thin binding over HTTP or RPC. Published as `versionx-client` on PyPI.

### 9.2 TypeScript/JavaScript
Same. Published as `@versionx/client` on npm.

### 9.3 Others
Community; we publish the OpenAPI spec and protocol docs.

---

## 10. Stability & versioning

- `versionx-sdk` 1.0 → semver-committed public API.
- JSON-RPC schema → versioned with `"protocol_version": "1"` on handshake.
- HTTP API → `v1` path prefix; deprecation policy = 12-month notice.
- MCP tool signatures → pinned to MCP spec `2025-06-18` minimum, advertising `2025-11-25` opportunistically. Migration cost: one breaking change per year.
- Structured output JSON → `schema_version` on every payload.

Breaking changes require a major version bump across all four surfaces simultaneously.

---

## 11. Testing

- SDK has its own test suite.
- JSON-RPC has protocol compliance tests (golden request/response fixtures).
- HTTP API has `schemathesis` or equivalent fuzz testing against the OpenAPI spec.
- MCP has fixture-based tests of tool calls and prompt workflows; tested against rmcp's test harness.
- E2E: an "all-surfaces" test that performs the same operation through each transport and asserts identical outcomes.

---

## 12. Non-goals

- **Not a general-purpose agent framework.** We're one tool in an agent's toolkit, not a platform for building agents.
- **Not an LLM provider.** We don't host or bundle LLMs; we integrate with whatever the user has configured via MCP or BYO-API-key.
- **Not a workflow engine.** Temporal/Airflow/etc. are complementary; Versionx runs inside workflows, doesn't replace them.
- **Not remote-exposed by default.** v1.0 MCP + HTTP are loopback only. OAuth/CIMD for remote is v1.2+.
