# 07 — Policy Engine

## Scope
The policy layer (L5). Defines the policy DSL (declarative TOML + sandboxed Luau), evaluation model, scope resolution, and how policies interact with releases, syncs, and CI.

## Contract
After reading this file you should be able to: write a policy file, understand when and how it evaluates, and know exactly how policy decisions gate or warn on Versionx operations.

---

## 1. Purpose

Teams managing fleets need enforceable rules like:
- "No repo may use Node < 20."
- "Production repos must use OIDC for npm publishing."
- "Breaking changes require a changeset file with rationale > 200 chars."
- "Dependencies flagged as CVE-high in the last 7 days cannot be added."
- "Only repos tagged `customer-facing` require a privacy review on release."

Versionx's policy engine unifies these across ecosystems, release flows, and multi-repo setups.

---

## 2. Design decisions

### 2.1 DSL: declarative TOML + Luau (sandboxed) as escape hatch

- **Declarative TOML** for common cases (version constraints, allowlists, presence checks). Covers ~90% of real policies.
- **Luau (via mlua)** for custom logic where declarative isn't enough. Luau is Roblox's hardened sandboxable Lua dialect — the only Lua variant with first-class sandboxing (`Lua::sandbox(true)`, `set_interrupt`, `set_memory_limit`).

**Why not Rego, Cedar, or CUE?**
- Rego (OPA) is industry-standard but teams find it error-prone and hard to onboard. Evaluated `regorus` crate as an alternative — kept in mind for future "Rego interop" mode but not default.
- Cedar (AWS) is fast + formally verified but targeted at authz, not general policy.
- CUE is constraint-oriented but niche.
- Plain Lua 5.4 can't be truly sandboxed — Redis had 3 critical Lua VM CVEs in 2025 (CVE-2025-49844 UAF RCE, -46817, -46818). Luau's sandbox is production-battle-tested.

### 2.2 Policies are inputs, not state
Policy files are read from disk per evaluation. No policy DB. Evaluation results are cached, not the policies themselves.

### 2.3 Evaluation is pure
Given (config, lockfile, plan, policies), evaluation is a pure function. No side effects. Deterministic. Testable.

### 2.4 Warn by default, deny explicitly
The default verdict for any rule is `warn`. Denying requires explicit `severity = "deny"` on each rule. This prevents policy-by-accident from breaking teams.

---

## 3. Policy file format

One policy file = one TOML file. Repos may have many. Files live:
- `.versionx/policies/<name>.policy.toml` — repo-scoped.
- `fleet://<fleet>/policies/<name>.policy.toml` — fleet-scoped (referenced via `inherit`).
- `org://<org>/policies/<name>.policy.toml` — org-scoped.
- `https://...` — remote, cached + checksum-verified.

### 3.1 Schema

```toml
[policy]
id = "node-minimum-version"                   # unique within this file
name = "Node.js minimum version"
description = "All repos must use Node.js 20 or newer."
severity = "deny"                             # "deny" | "warn" | "info"
scope = "all"                                 # "all" | tag-glob | path-glob | repo-list
sealed = false                                # if true, downstream can't disable

# === Trigger: when does this policy evaluate? ===
[policy.triggers]
on_sync = true
on_release = true
on_ci = true
on_new_dep = false

# === Rule: declarative check ===
[policy.rule]
type = "runtime_version"
runtime = "node"
min = "20.0.0"
# optional: max, allowlist, denylist

# === Message shown when violated ===
[policy.message]
summary = "Node version {actual} is below the required minimum {required}."
remediation = "Update [runtimes] node in versionx.toml to 20 or later."
```

### 3.2 Multiple policies per file
```toml
[[policies]]
id = "..."
# ...

[[policies]]
id = "..."
# ...
```

### 3.3 Inheritance & Lockfiles
```toml
# In member repo's versionx.toml
[policies]
inherit = [
  "fleet://acme-platform/baseline",
  "org://acme/security",
]
files = [".versionx/policies/*.policy.toml"]       # local additions
```
Rules merge. Local files can override inherited rules **only** if `override = true` and the inherited rule's `sealed = false`. Sealed rules cannot be disabled downstream.

**Policy Lockfile (`versionx.policy.lock`):**
To prevent a central policy update from breaking CI across the fleet:
- Inherited policies are pinned to their content SHA in a separate `versionx.policy.lock`.
- `versionx policy update` explicitly pulls the latest policies and updates the lockfile.
- CI runs `versionx policy verify` to ensure the local policy cache matches the lockfile.

---

## 4. Built-in rule types

The `type = "..."` field selects a built-in rule with a fixed schema. These cover most common cases without Luau.

### 4.1 `runtime_version`
```toml
[policy.rule]
type = "runtime_version"
runtime = "node" | "python" | "rust" | ...
min = "20.0.0"
max = "22.99.99"                              # optional
allowlist = ["22.12.0", "22.0.0"]             # optional, exact versions only
denylist = ["18.0.0", "18.0.1"]               # optional
```

### 4.2 `dependency_version`
```toml
[policy.rule]
type = "dependency_version"
ecosystem = "node"
package = "react"
constraint = ">=18.0.0"
```

### 4.3 `dependency_presence`
```toml
[policy.rule]
type = "dependency_presence"
ecosystem = "python"
package = "requests"
allowed = false                               # fail if present
reason = "Use httpx instead per ADR-0014"
```

### 4.4 `advisory_block`
```toml
[policy.rule]
type = "advisory_block"
severity_min = "high"                         # "low" | "medium" | "high" | "critical"
max_age_days = 7                              # block if advisory exists for > 7 days
exceptions = ["GHSA-xxxx-yyyy-zzzz"]          # known-accepted advisories
```

### 4.5 `release_gate`
```toml
[policy.rule]
type = "release_gate"
require_changeset = true
changeset_min_chars = 100
require_ci_green = true
require_trusted_publishing = true             # OIDC required for publishing
require_approvers = 1
```

### 4.6 `commit_format`
```toml
[policy.rule]
type = "commit_format"
pattern = "conventional"                      # or a custom regex
apply_to = "pr_title"                         # "commits" | "pr_title" | "both"
```

### 4.7 `lockfile_integrity`
```toml
[policy.rule]
type = "lockfile_integrity"
require_committed = true
require_synced = true                          # lockfile matches manifest
require_signed = false                         # optional: require signed commits for lockfile
```

### 4.8 `link_freshness`
```toml
[policy.rule]
type = "link_freshness"
link_kind = "submodule" | "subtree" | "ref" | "any"
max_age_days = 30
```

### 4.9 `provenance_required`
```toml
[policy.rule]
type = "provenance_required"
ecosystems = ["node", "python"]
min_attestations = ["sigstore", "slsa@1"]
```
Blocks publishes without valid provenance attestations. Reads in-toto statements where available.

### 4.10 `custom` (escape hatch to Luau)
```toml
[policy.rule]
type = "custom"
script = "policies/no-legacy-api.luau"
timeout_ms = 100                              # default 100ms; max 5000ms
memory_limit_mb = 32                          # default 32MB; max 128MB
```

---

## 5. Luau scripting environment

When declarative rules aren't enough, Luau provides a sandboxed scripting layer. **Not Lua 5.4** — only Luau offers the hardened sandbox we need.

### 5.1 Sandbox hardening

- `Lua::sandbox(true)` enforced at VM creation.
- **Stripped globals**: no `io`, `os`, `package`, `debug`, `dofile`, `loadfile`, `require`.
- **CPU budget**: `set_interrupt` checked every N instructions (default 100ms wall clock).
- **Memory budget**: `set_memory_limit` set to 32MB default (configurable per policy up to 128MB).
- **Filesystem**: read-only access to an explicit allowlist of paths (workspace root, policy dir). No writes.
- **Network**: rate-limited and cached HTTP GET only (no POST, no arbitrary protocols). Rate cap: 5 req/eval, 50 req/min global.

### 5.2 Pre-loaded modules (the stdlib exposed to scripts)

- `semver`: semantic version parsing and comparison.
- `path_utils`: UTF-8 path manipulation (no FS writes).
- `http`: cached, rate-limited HTTP GET.
- `json`: encoding/decoding JSON.
- `crypto`: hashing only (Blake3, SHA-256).
- `git` (read-only): `git.log(n)`, `git.show(sha)`, `git.diff(a, b)`. No commit/push/write operations.

### 5.3 Exposed globals (read-only)

```luau
-- workspace.root        : string
-- workspace.packages    : table[] with .path, .ecosystem, .manifest, .version
-- workspace.runtimes    : table of runtime -> version
-- workspace.links       : table[]
-- plan                  : current plan being evaluated (nil at sync time)
-- release               : current release plan (nil outside release)
-- git.log(n)            : function returning last n commit objects
-- http.get(url)         : cached HTTP GET; rate-limited per eval
```

### 5.4 Example script

```luau
function check(ctx)
  for _, pkg in ipairs(ctx.workspace.packages) do
    if pkg.ecosystem == "node" then
      local react = pkg.manifest.dependencies["react"]
      if react and react:match("^[0-9]") and tonumber(react:match("^(%d+)")) < 18 then
        return {
          verdict = "deny",
          message = "React version in " .. pkg.path .. " is below 18",
          remediation = "Upgrade react to 18 or later",
        }
      end
    end
  end
  return { verdict = "allow" }
end
```

### 5.5 Testing Luau policies
```bash
versionx policy test policies/no-legacy-react.luau --fixture tests/fixtures/legacy-react/
```

### 5.6 Threat model documented

Policies from fleet-inherited or remote sources are **untrusted** by default. Vx pins them by SHA (`versionx.policy.lock`), runs them in the Luau sandbox, and documents the sandbox escape surface explicitly:
- `mlua` upstream security advisories are tracked; pinned version range only.
- `versionx policy audit` reports the SHA + source + last-reviewed-by of every inherited policy.

---

## 6. Evaluation model

### 6.1 When policies evaluate
- **`on_sync`**: before `versionx sync` applies its plan.
- **`on_release`**: after a release plan is proposed, before it can be approved.
- **`on_ci`**: on every CI run when Versionx is invoked.
- **`on_new_dep`**: when `versionx install` or `versionx upgrade` adds/updates a dependency.
- **`on_schedule`**: cron-driven for ambient checks (via daemon).

Multiple triggers per policy allowed. `triggers.on_*` defaults are documented per rule type.

### 6.2 Scope resolution
A policy's `scope` determines which repos it applies to in multi-repo contexts:
- `"all"` — every member.
- Tag-based: `tags = ["customer-facing"]` — members tagged thus.
- Path glob: `paths = ["apps/*"]` — packages under these paths in a monorepo.
- Explicit list: `repos = ["portal-api", "portal-frontend"]`.

### 6.3 Verdict aggregation
Multiple policies evaluate in parallel. Final verdict:
- Any `deny` → overall `deny`; operation blocks.
- No `deny`, any `warn` → overall `warn`; operation continues with warning.
- No `deny`, no `warn` → `allow`.

The user sees all warnings/denials grouped by policy ID, with remediation text. JSON output includes machine-readable verdicts.

### 6.4 Caching
Evaluations are cached in `state.policy_evaluations` keyed by (policy_id, workspace_hash, plan_hash). Cache invalidated on config/policy/lockfile change. Cache miss rate is a diagnostic exposed in `versionx policy stats`.

---

## 7. Policy authoring UX

### 7.1 Scaffolding
```bash
versionx policy init                              # creates .versionx/policies/ with a template
versionx policy add runtime_version --runtime node --min 20
```

### 7.2 Testing
```bash
versionx policy test                              # evaluate all policies against current state
versionx policy test --plan <plan.json>           # evaluate against a hypothetical plan
versionx policy explain <policy-id>               # show which facts triggered the verdict
```

### 7.3 Importing from other tools
```bash
versionx policy import --from renovate            # translate Renovate rules
versionx policy import --from dependabot          # translate Dependabot config
```
Best-effort; output marked "needs review".

---

## 8. Waivers — first-class expiry

Sometimes a violation is accepted. Versionx requires **mandatory expiry by default** (org can override); this matches Snyk's widely-praised model, not Dependabot's no-expiry approach.

```toml
# .versionx/waivers.toml
[[waivers]]
policy_id = "dependency_version:react-18"
reason = "Migrating to React 18 in Q2; see JIRA-1234."
expires_at = "2026-06-01"                   # REQUIRED by default
approved_by = "engineering-leads@acme"
approval_ticket = "JIRA-1234"
```

### 8.1 Mandatory expiry (default)
- Waivers without `expires_at` are **rejected** by `versionx policy verify`.
- Org policy can opt out: `[policies.waivers] expires_required = false`.
- Expired waivers:
  - Warn 7 days before expiry (in PR comments, `versionx status`, dashboards).
  - Error on the day of expiry (the underlying violation becomes active).
- `reason` is required and free-text.
- `approved_by` is required (enum of team identifiers or email).

### 8.2 Behavior
- Waivers suppress the specific policy violation until expiry.
- Appear in reports as **"waived"** not **"passed"** — visible, not hidden.
- `versionx waiver list` shows all active waivers with days-remaining.
- `versionx waiver audit` produces a compliance-friendly audit log with: waiver id, reason, approved_by, approval_ticket, created_at, expires_at, active status.

### 8.3 Creation workflow
- `versionx waiver add --policy <id> --reason "..." --expires 2026-06-01 --ticket JIRA-1234`
- Writes to `.versionx/waivers.toml`; opens in editor for review.
- Can require manual approval workflow via GitHub Actions template (PR with waiver → required reviewers → merge).

---

## 9. CI integration (and why there's no GitHub App in v1.0)

Since v1.0 drops the hosted GitHub App, policy enforcement runs entirely in CI:

```yaml
# .github/workflows/versionx-policy.yml
- uses: acme/versionx-install-action@v1
- run: versionx policy check --output annotations
  # Posts GitHub check-run annotations from within the workflow.
```

Versionx's `policy check` command in CI context:
- Posts GitHub check-run annotations (via `GITHUB_TOKEN` — limits: 50 annotations per API request, 65535-char summary, paginated).
- Writes `versionx-policy.sarif` for GitHub Advanced Security ingestion.
- Fails the workflow on any `deny` verdict.

A hosted GitHub App (reacting to webhooks without per-repo workflow setup) is a post-v1.0 question. See `08-github-integration.md`.

---

## 10. Fleet dashboards (via TUI/web UI, not hosted service)

The TUI and local web UI expose:
- Per-repo policy compliance grid.
- Aging violations (open for N days).
- Expiring waivers in the next 30 days.
- Policy evaluation latency (P50/P95).
- Which policies deny most often (candidates for tightening remediation UX).

Queryable via `versionx fleet query`:
```bash
versionx fleet query "policy_violations.severity = deny"
versionx fleet query "waivers.expires_at < now() + interval '30 days'"
```

---

## 11. Performance

- Declarative rules evaluate in <1ms each.
- Luau rules budgeted to 100ms wall clock (default); max 5s.
- Full fleet policy eval (100 repos × 20 policies) target: <10s in parallel.
- Cache hit rate target: >80% on repeated CI runs.

---

## 12. Non-goals

- **Not a full policy language** (Rego, Cedar). Luau covers the expressiveness we need without forcing a new mental model.
- **Not runtime enforcement** — Versionx gates operations it runs; it doesn't block arbitrary git pushes or package installs outside its control.
- **Not an ACL system** — who can approve waivers is enforced by GitHub repo permissions / branch protection, not Versionx.
- **Not a compliance framework** — Versionx policies help comply with frameworks (SOC2, ISO) but aren't a full compliance product.
