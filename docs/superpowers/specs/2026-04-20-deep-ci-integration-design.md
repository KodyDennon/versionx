# Deep CI integration — design

**Date:** 2026-04-20
**Status:** Design approved; ready for implementation plan
**Author:** Kody + Claude (brainstorm)
**Scope:** Build deep CI integration into Versionx so that when users adopt it in their own repos, the CLI auto-detects the CI environment, drives the full release / update / policy / saga flow, and publishes to registries — without users having to wire anything beyond a single reusable workflow line. First-class on GitHub; forge-agnostic architecture so GitLab / Bitbucket / Gitea follow on the same trait surface.

---

## 1. Goals & non-goals

### Goals

1. Users adopt Versionx with **one line of workflow YAML** and get a full release pipeline, policy-gated PRs, and dependency-update PRs out of the box.
2. The `versionx` CLI itself detects CI context and emits native UI (annotations, check runs, sticky PR comments) automatically — no flags, no separate orchestrator tool.
3. Authentication **auto-discovers** the best available token (GitHub App → PAT → `GITHUB_TOKEN`) and degrades capabilities gracefully with clear warnings when features are gated.
4. **Publishing is built in.** Versionx drives `npm publish` / `cargo publish` / `twine upload` / OCI publishes itself (with per-ecosystem opt-out for users who want their workflow to own publishing).
5. **Cross-repo saga** coordinates multi-repo releases over `workflow_dispatch` with a compensating-rollback path on failure.
6. **Forge-agnostic core.** GitHub is the primary target; GitLab, Bitbucket, and Gitea ride the same trait surface and land in Phase E.
7. **Hosted GitHub App (identity only)** published so users can adopt a stable "Versionx" identity for commits and actions without any SaaS hosting on our side.
8. **Bot-account persona** for users who want a dedicated identity (author name/email + optional commit signing).

### Non-goals

- **Hosted SaaS service.** No webhook receivers, no hosted app runtime. The GitHub App we publish is identity-only.
- **GitHub Enterprise Server-specific testing** in v1. The `octocrab` client accepts custom base URLs; we document but don't commit to GHES parity.
- **GUI / dashboard** for the orchestrator. Everything surfaces through native PR UI (comments, check runs, annotations) and the existing TUI.
- **Replacing Renovate / Dependabot** everywhere. We ship a full dep-update flow; teams can still mix with Renovate.
- **Replacing native publish tools.** We drive them; we don't reimplement resolvers or registry clients.
- **Package-registry hosting.** Versionx never hosts artifacts.

---

## 2. Core decisions (locked)

| Decision | Choice | Rationale |
|---|---|---|
| Integration shape | CLI auto-detects CI context + escape-hatch `versionx github/gitlab/…` subcommands + reusable workflows | One binary, smart mode, power-user escape hatches, minimal YAML for 95% of users |
| Crate split | `versionx-forge-trait` + `versionx-forge-{github,gitlab,bitbucket,gitea}` + `versionx-forge` meta | Forge-agnostic from day one; GitHub is primary in v1 |
| Core coupling | Core defines `Reporter` traits; frontends wire the right forge impl | Keeps Design Principle 2 (core-library) honored |
| Token discovery | App → PAT → `GITHUB_TOKEN` → none (graceful degrade) | Zero-config for 90% of users; power for the rest |
| Release flow default | `release-pr` (release-please-style), with `direct` opt-in | Safer default; both fully supported |
| Publishing | Versionx drives by default, per-ecosystem `workflow` / `skip` overrides | Users get "just works" with tokens set; can override per ecosystem |
| Cross-repo saga | `workflow_dispatch` fan-out + compensating dispatch on failure | Transportable; no SaaS required |
| Forge expansion | GitHub full in v1; GitLab/Bitbucket/Gitea as Phase E | Manageable scope; trait layer makes them tractable |
| GitHub App | We register an identity-only public App | Cleanest token + identity story; no hosting |
| Bot-account persona | Optional `[identity]` config with signing | Works across every forge, via the shared identity module |

---

## 3. Architecture

### Crate layout

```
crates/
├── versionx-forge-trait/          (NEW)
│   └── src/
│       ├── lib.rs
│       ├── context.rs              ForgeContext trait + shared types (RepoRef, GitRef, PullRequest)
│       ├── annotations.rs          Annotation formatter trait + file/line/col/title struct
│       ├── check_run.rs            CheckRun trait (start/update/complete)
│       ├── pr_comment.rs           StickyComment trait (marker-based upsert)
│       ├── release_pr.rs           ReleasePr trait (open/sync/merge/close)
│       ├── dispatch.rs             ForgeDispatch trait (workflow_dispatch analog)
│       ├── identity.rs             Identity + signing
│       ├── publish.rs              PublishDriver trait + ecosystem enum
│       ├── capabilities.rs         Capabilities bitflags (forge-neutral)
│       └── testkit.rs              Reusable test harness every forge plugs into
│
├── versionx-github/                (EXISTING skeleton — fill in)
│   └── src/
│       ├── lib.rs
│       ├── context.rs              GitHubContext::detect()
│       ├── token.rs                App JWT / PAT / GITHUB_TOKEN discovery
│       ├── client.rs               octocrab wrapper + retry + rate-limit
│       ├── app.rs                  App JWT → installation token exchange
│       ├── annotations.rs          ::error::/::warning::/::notice:: emitters
│       ├── check_run.rs            Check-run create/update, fallback to commit-status
│       ├── pr_comment.rs           Sticky comment via HTML marker
│       ├── release_pr.rs           Release-PR maintenance (open/sync/merge-triggers-apply)
│       ├── dispatch.rs             workflow_dispatch + repository_dispatch
│       ├── publish.rs              per-ecosystem driver routing
│       ├── merge.rs                Auto-merge via enablePullRequestAutoMerge GraphQL
│       └── identity.rs             Persona application + signing
│
├── versionx-gitlab/                (NEW, Phase E)
├── versionx-bitbucket/             (NEW, Phase E)
├── versionx-gitea/                 (NEW, Phase E)
│
├── versionx-forge/                 (NEW meta-crate)
│   └── src/lib.rs                  pub use + fn detect() -> Option<Box<dyn ForgeContext>>
│
├── versionx-core/
│   └── src/
│       └── integrations.rs         (NEW module) — Reporter traits core uses; no forge vocabulary
│
└── versionx-cli/                   Wires the forge layer into every verb
```

### The critical boundary

`versionx-core` never depends on any `versionx-forge-*` crate directly. Core defines a handful of thin `*Reporter` / `PublishDriver` traits with no forge-specific vocabulary. `versionx-cli` constructs the right impl at startup based on `versionx_forge::detect()`.

This honors the existing Design Principle 2 ("core is a library; everything else is a frontend") and means a future `versionx-forge-jenkins` or `versionx-forge-circleci` fits in without touching core.

### Data flow for `versionx release plan` in CI

```
versionx-cli main
  → versionx_forge::detect() → Some(Box<dyn ForgeContext>)
  → construct concrete Reporter impls backed by the forge context
  → call versionx_core::commands::release::plan(...)
    → core computes plan
    → core calls reporter.announce_plan(plan)
       → forge impl: create check run (in_progress)
                   : upsert sticky PR comment (marker versionx:release-plan)
                   : emit ::notice:: annotation
    → core returns plan
  → on success: reporter.finish_plan(ok)
     → check run → success; comment body finalized with result
  → on error: reporter.finish_plan(err)
     → check run → failure; annotation emitted; comment body updated with diagnostic
```

### Escape-hatch subcommands

Added to `versionx-cli`, routed through `versionx-forge::detect()`:

```
versionx github comment       # upsert a sticky comment
versionx github check-run     # create/update a check run
versionx github release-pr    # sync | merge | close
versionx github publish       # apply a stored plan's publish step
versionx github dispatch      # fire workflow_dispatch / repository_dispatch
versionx github detect        # print context (token source, caps, repo, PR, commit)

# Same shape under `versionx gitlab`, `versionx bitbucket`, `versionx gitea`.
```

### Reusable workflows

Shipped in `.github/workflows/` of this repo, `on: workflow_call` + `on: workflow_dispatch`:

- `release.yml` — plan + release-PR (or direct) + apply + publish + dispatch downstream.
- `update.yml` — scheduled or on-demand dep updates with grouping + auto-merge.
- `policy.yml` — PR-time policy gate.
- `saga.yml` — multi-repo saga fan-out target.
- `install.yml` — pinned `versionx` binary install with cache.

Downstream:

```yaml
jobs:
  release:
    uses: KodyDennon/versionx/.github/workflows/release.yml@v1
    secrets: inherit
```

---

## 4. Token discovery & capability matrix (GitHub)

### Discovery order

Picked at CLI startup, first match wins:

1. **GitHub App** — `VERSIONX_GH_APP_ID` + `VERSIONX_GH_APP_INSTALLATION_ID` + `VERSIONX_GH_APP_PRIVATE_KEY` (or `_PATH`). Exchanged for a short-lived installation token; refreshed before the 60-minute TTL.
2. **Personal Access Token** — `VERSIONX_GH_TOKEN`, then `GH_TOKEN`, then `GITHUB_TOKEN` iff it looks like a PAT (prefix heuristic).
3. **Actions-provided `GITHUB_TOKEN`** — ambient token in Actions (prefix `ghs_`).
4. **None** — local/unauth mode. GitHub ops become no-ops; CLI continues.

Logged on first use. `versionx github detect` prints current context explicitly.

### Capability matrix

| Feature | Needed token perm | Default `GITHUB_TOKEN` | PAT | App |
|---|---|---|---|---|
| CI annotations | — | ✅ | ✅ | ✅ |
| Read repo / PRs / commits | `contents:read`, `pull-requests:read` | ✅ | ✅ | ✅ |
| Post / update PR comments | `pull-requests:write` | ✅ (if workflow grants) | ✅ | ✅ |
| Create / update check runs | `checks:write` | ✅ | ❌ (PATs can't create check runs) | ✅ |
| Create / update release PR | `contents:write` + `pull-requests:write` | ✅ | ✅ | ✅ |
| Pushed-tag triggers downstream workflows | non-Actions identity push | ❌ (silent-drop) | ✅ | ✅ |
| Cross-repo `workflow_dispatch` | `actions:write` across repos | ❌ | ✅ (scoped) | ✅ |
| Auto-merge enablement | `pull-requests:write` + branch protection | ✅ | ✅ | ✅ |
| Publish to `ghcr.io` | `packages:write` | ✅ (if workflow grants) | ✅ | ✅ |

When only the default `GITHUB_TOKEN` is present, the tag-push-triggers-downstream gap is bridged by invoking publishes + dispatches in the same Actions run that merged the PR (no indirection needed via tag).

### `GitHubContext` type

```rust
pub struct GitHubContext {
    pub repo: RepoRef,
    pub default_branch: String,
    pub current_ref: GitRef,
    pub pull_request: Option<PullRequest>,
    pub commit_sha: String,
    pub actor: String,
    pub run_id: Option<u64>,
    pub token: TokenSource,
    pub capabilities: Capabilities,
    pub client: octocrab::Octocrab,
}

pub enum TokenSource { App, Pat, GithubToken, None }
```

`GitHubContext::detect()` probes `/meta` + `/rate_limit` + at least one scoped endpoint per capability; caches result for the process.

### Auto-configure hint

The shipped reusable workflows declare the full `permissions:` block users need. `versionx github detect` prints the tailored block for the current repo's intended use.

---

## 5. Per-feature specs

### 5.1 CI annotations

Every `thiserror`-backed error, policy violation, and noteworthy event routes through `versionx-github::annotations` when `GitHubContext::in_actions()`. Formats Actions-recognized syntax:

```
::error file=versionx.toml,line=14,col=5::unknown runtime `rusty`
::warning title=Outdated dependency::axios 1.6.0 → 1.7.7
::notice title=Release plan::my-app@0.2.0 (minor)
```

Max line length 65,536 chars. Lines sanitized to strip embedded `%`, `\n`, `\r` per Actions spec. GitLab / Bitbucket / Gitea use their own annotation conventions through the same trait; emitters live in each forge crate.

### 5.2 Sticky PR comments

One sticky comment per (PR, verb) pair, identified by an HTML marker (`<!-- versionx:release-plan -->`). Lifecycle:

1. First run for PR+verb: list comments, look for marker, create if absent.
2. Subsequent runs: edit in place. No spam on force-push.
3. Closed/merged PRs: leave comments alone.
4. Stale-plan handling: mark existing comment "⚠ stale" and append a new one.

Canonical body shapes live in `versionx-forge-trait::pr_comment::templates` with `insta` snapshot coverage.

### 5.3 Check runs

Per-verb check runs, gating PR merges where required:

| Verb | Check name | Success when |
|---|---|---|
| `release plan` | `versionx / release` | Plan produced, policy clean |
| `update --plan` | `versionx / deps` | Plan produced, no disallowed bumps |
| `policy eval` | `versionx / policy` | No deny rules fired |
| `sync` | `versionx / sync` | Lockfile verified clean |

`in_progress` → `success` / `failure` / `neutral`. `output.annotations[]` populated from the same stream as §5.1. Missing `checks:write` falls back to commit statuses.

### 5.4 Release PR flow (default)

Trigger: push to default branch.

1. Compute `versionx release plan`.
2. Nothing to release → close any open release PR, exit.
3. Find single open PR with label `versionx:release`. If none, create branch `versionx-release/v<next>` and open PR. Title: `chore(release): v0.8.0`. Body: plan summary + merge checklist.
4. Exists → update branch: re-bump, regen changelog, amend commit, force-push with `--force-with-lease`.
5. On merge: post-merge workflow (or same-run handler if `GITHUB_TOKEN`-only) triggers `versionx release apply`, tags, pushes tags, publishes per §5.7.

Edge cases handled:

- Concurrent merges: `--force-with-lease` + retry once; surface conflicts as check-run failure.
- `GITHUB_TOKEN`-only: publish + dispatch in the same Actions run that merged the PR.
- Stale prerequisites: re-verify before amending; regenerate if HEAD moved.

### 5.5 Direct release flow

`[github] release-flow = "direct"`. Every merge that bumps → immediate tag + publish. Sticky comment posted to the merged PR summarizing what landed.

### 5.6 Dependency update PRs

Scheduled cron or manual trigger.

1. `versionx update --plan` with grouping rules from `[github.auto-merge]` + `[update.groups]`.
2. Split plan into groups (default: one PR per ecosystem+bump-level).
3. Per group: branch `versionx-deps/<group-id>`, apply subset, commit, open PR with per-package deltas + registry-changelog links, label `versionx:deps`.
4. `auto-merge: safe` → enable auto-merge only when patch-only AND policy green.
5. Existing same-group PRs rebased if newer versions arrive.

### 5.7 Publishing

Per `[github.publish.<ecosystem>]`:

- `"versionx"` (default) → Versionx drives `npm publish` / `cargo publish` / `twine upload` / OCI push with the corresponding token env var.
- `"workflow"` → emit `.versionx/out/published-artifacts.json`; user's workflow handles publishing.
- `"skip"` → do nothing.

Publishes run in dependency-DAG order. Per-ecosystem concurrency: npm high, crates.io low. Exponential backoff on rate-limit. On success: `::notice::` annotation + GitHub Release created via `POST /repos/{}/{}/releases` with the changelog section.

### 5.8 Cross-repo saga

For multi-repo workspaces where repo A's release fans out to B/C/D:

1. A's `release apply` reads `[github.saga.downstream]`.
2. For each downstream: fire `workflow_dispatch` on `release.yml@v1` with inputs `{trigger-ref, trigger-plan-blake3, upstream-version, saga-id}`.
3. Downstream `release.yml` gates on upstream dispatch via `versionx release plan --upstream=<ref>`.
4. Compensating rollback: saga ID threads through every dispatch. On failure, A fires compensating `workflow_dispatch` on `saga-compensate.yml@v1` in every downstream that already got the forward dispatch.

### 5.9 Auto-merge

```toml
[github.auto-merge]
release-pr  = "manual"
dep-updates = "squash-when-safe"
```

`"squash-when-safe"` enables auto-merge only when group is patch-only AND policy green. Uses GraphQL `enablePullRequestAutoMerge`. Respects branch protection.

### 5.10 Bot-account persona

```toml
[identity]
name             = "versionx-bot"
email            = "bot@example.com"
signing          = "ssh"
signing-key-env  = "VERSIONX_SIGN_KEY"
co-authored-by   = "github-actions[bot] <41898282+github-actions[bot]@users.noreply.github.com>"
```

Applied to every commit Versionx makes. SSH signing via `gitoxide` (built in). GPG signing via the user-installed `gpg` binary. Defaults to unsigned. Works across every forge via `versionx-forge-trait::identity`.

### 5.11 Hosted GitHub App (identity only)

Versionx registers a public **"Versionx"** GitHub App.

- Permissions: `contents:write`, `pull-requests:write`, `issues:write`, `checks:write`, `actions:write`, `packages:write`, `metadata:read`.
- No webhooks. No hosted service. No billing.
- Public install URL documented in Get Started.
- App ID published as a known constant in `versionx-github/src/app.rs`.

Auth flow (inside Versionx):

1. `VERSIONX_GH_APP_ID` + `VERSIONX_GH_APP_INSTALLATION_ID` + private key loaded.
2. Sign a JWT at startup.
3. Exchange for installation token via `/app/installations/{id}/access_tokens`.
4. Refresh before the 60-minute TTL on long-running operations.

Commits / comments / check runs attributed to **"Versionx"** in the GitHub UI, not `github-actions[bot]`.

---

## 6. `[github]` config schema

```toml
[github]
token-env     = "GITHUB_TOKEN"
release-flow  = "pr"

[github.comments]
release-plan = "sticky"
update-plan  = "sticky"
policy       = "on-failure"

[github.checks]
release = "versionx / release"
deps    = "versionx / deps"
policy  = "versionx / policy"
sync    = "versionx / sync"

[github.publish.node]
mode      = "versionx"
token-env = "NPM_TOKEN"
access    = "public"

[github.publish.rust]
mode      = "versionx"
token-env = "CARGO_REGISTRY_TOKEN"

[github.publish.python]
mode = "workflow"

[github.publish.oci]
mode      = "versionx"
registry  = "ghcr.io"
token-env = "GITHUB_TOKEN"

[github.auto-merge]
release-pr  = "manual"
dep-updates = "squash-when-safe"

[github.saga]
downstream = [
  { repo = "KodyDennon/consumer-a", workflow = "release.yml", ref = "main" },
  { repo = "KodyDennon/consumer-b", workflow = "release.yml", ref = "main" },
]
compensate-on-failure = true

[github.annotations]
enabled = "auto"

[identity]
name    = "versionx-bot"
email   = "bot@example.com"
signing = "none"
```

GitLab / Bitbucket / Gitea get parallel `[gitlab]` / `[bitbucket]` / `[gitea]` blocks with analogous shape.

### Zero-config defaults

No `[github]` and no config → release PR flow, sticky comments on `release plan` + `update --plan`, check runs on every mutating verb, Versionx-driven publishing where tokens are present (others skipped with `::notice::`), auto-merge off everywhere, no saga.

---

## 7. Reusable workflows

Each shipped at `.github/workflows/*.yml` in this repo.

### `release.yml`

Inputs:

| Input | Default | Description |
|---|---|---|
| `mode` | `"release-pr"` | `release-pr` / `direct` / `apply` |
| `scope` | `"workspace"` | `workspace` / `fleet` / `members:a,b,c` |
| `plan-artifact` | `""` | For `mode: apply` |
| `publish` | `true` | Stop at tag + upload artifacts if false |
| `ecosystems` | `"all"` | Limit publishing |

Jobs: `install → detect → plan → (release-pr or apply) → publish → dispatch-downstream`.

### `update.yml`

| Input | Default | Description |
|---|---|---|
| `scope` | `"workspace"` | Same shape |
| `grouping` | `"ecosystem+bump-level"` | Grouping strategy |
| `auto-merge` | `"safe"` | `never` / `safe` / `always` |
| `labels` | `"versionx:deps"` | Extra labels |

Default cron `0 6 * * 1` when called from a scheduled workflow.

### `policy.yml`

| Input | Default | Description |
|---|---|---|
| `fail-on` | `"deny"` | `deny` / `warn` / `never` |
| `scope` | `"workspace"` | — |

Fails on policy deny. Always creates a check run.

### `saga.yml`

Inputs: `trigger-ref`, `trigger-plan-blake3`, `upstream-repo`, `upstream-version`, `saga-id`.

### `install.yml`

Installs + caches a pinned `versionx` binary. Callable alone or from the others.

---

## 8. Testing strategy

Four tiers per forge:

- **Unit** — token parsing, capability derivation, annotation formatting, sticky-comment marker round-trip, release-PR branch derivation.
- **Property** — sticky comment round-trips, release-PR title/branch determinism, publish ordering on arbitrary DAGs.
- **Snapshot (`insta`)** — canonical PR comment bodies, check-run `output.summary`, saga dispatch inputs, annotation stderr.
- **Integration (`wiremock`)** — full REST-call sequences for every verb against a fake GitHub API.

**E2E** — gated behind `VERSIONX_E2E_GH=1`, runs nightly against a disposable `versionx-e2e` sandbox repo. Exercises release-PR → merge → publish (Verdaccio in-workflow) → saga fan-out → compensate.

Most shared logic lives in `versionx-forge-trait::testkit`; every forge impl plugs in the same harness.

CI matrix adds a `gh-integration` job per PR and a scheduled `gh-e2e` job. E2E for GitLab/Bitbucket/Gitea follows in 1.1.

---

## 9. Rollout phasing

Five phases. Each ends with a version tag, docs auto-update, and a release note.

**Phase A — Forge trait + GitHub context / annotations / check runs**
Lands: `versionx-forge-trait` crate, `versionx-github` filled in with `GitHubContext::detect`, token discovery, capability matrix, annotation emitter, check-run client, `versionx github detect` subcommand, unit tests. At end: CI runs annotate + create check runs.

**Phase B — Sticky PR comments + escape-hatch subcommands**
Lands: `pr_comment.rs`, `versionx github comment`, reporter trait wired into `release plan` / `update --plan` / `policy eval`, snapshot tests. At end: PRs get rich live-updating comments.

**Phase C — Release PR + direct release + publishing + App identity**
Lands: `release_pr.rs`, `publish.rs`, `app.rs`, `versionx github release-pr` + `versionx github publish`, `[github]` config schema, wiremock integration tests, `release.yml` reusable workflow, public GitHub App registration. At end: users adopt Versionx releases with a single `uses:` line.

**Phase D — Dep updates + saga + auto-merge + remaining workflows**
Lands: `dispatch.rs`, `merge.rs`, `update.yml` / `policy.yml` / `saga.yml` / `install.yml`, cross-repo saga + compensating dispatch, E2E test infrastructure. At end: full orchestrator on GitHub.

**Phase E — GitLab / Bitbucket / Gitea**
Lands: `versionx-gitlab` / `versionx-bitbucket` / `versionx-gitea` impls of the trait, forge-agnostic `[identity]` persona tested cross-forge, parallel `[gitlab]` / `[bitbucket]` / `[gitea]` config blocks, parallel escape-hatch subcommand trees. At end: the orchestrator experience lands on every major forge.

---

## 10. Acceptance criteria

Delivered when:

1. A user with zero configuration can adopt Versionx on GitHub by adding `uses: KodyDennon/versionx/.github/workflows/release.yml@v1` and normal PR merges produce a live-updating release PR, policy-gated dep PRs, and registry publishes.
2. `versionx github detect` prints a complete context (token source, capabilities, repo, PR, commit) in under 500ms on a cold run.
3. Every mutating verb produces a check run in GitHub Actions.
4. All four test tiers pass for `versionx-github`; the `wiremock` integration suite covers every REST endpoint the crate calls.
5. Saga dispatches succeed across three downstream repos end-to-end and compensating rollback completes when a downstream fails.
6. Publishing to npm (via Verdaccio-in-workflow), crates.io (via alternative-registry test instance), test.pypi.org, and `ghcr.io` succeeds for the test-repo release flow without leaking artifacts to public registries.
7. Bot-account persona commits show correct author + email + (optional) signature across all four forges in Phase E.
8. The "Versionx" public GitHub App is registered, linked from the docs, and working for at least one real user.
9. Zero-config defaults unchanged when the user provides no `[github]` block.
10. `cargo xtask docs` regenerates the new config pages + CLI subcommand pages + reusable workflow reference without drift.

---

## 10a. Security considerations

- **Private key handling for the App JWT flow.** Keys are loaded from env or a path, decoded into an in-memory `rsa::RsaPrivateKey`, and never logged. If `VERSIONX_GH_APP_PRIVATE_KEY` is the direct PEM blob, it's wiped from the process env after load. Key path variant leaves the file where it is.
- **Token rotation.** Installation tokens last 60 minutes; Versionx refreshes at 50 minutes on long-running operations. No tokens persist to disk.
- **PR-comment markdown injection.** User-controlled strings (commit messages, PR titles, changelog entries) rendered inside PR comments are passed through a conservative Markdown sanitizer that strips raw HTML tags except a known allow-list (`<details>`, `<summary>`, `<code>`, etc.). Mermaid blocks disabled inside comments.
- **Registry tokens.** Each publish driver reads its token from the configured env var immediately before invoking the native tool, passes via stdin or env to the child, and never logs the value.
- **Workflow dispatch payload size.** Capped at 50 KB; larger payloads get uploaded as an artifact and the dispatch receives a reference.
- **Saga ID reuse.** UUIDv7 generated per saga; never reused. Compensating dispatches are idempotent — duplicate compensate calls for the same saga-ID are no-ops.

---

## 11. Explicit non-goals (v1)

- Hosted SaaS / webhook receiver / billing.
- GitHub Enterprise Server-specific testing + bug fixes.
- Forge-specific dashboards or UI.
- Replacing Renovate/Dependabot globally.
- Custom package registries we own.

---

## 12. Open implementation questions

Surface during the implementation plan, not the design:

- Exact `octocrab` version required for the App JWT flow (the released 0.49 may need a feature flag).
- Retry policy defaults for each ecosystem's publish call (npm rate-limit behavior documented; crates.io rate-limits harder; PyPI has surge limits).
- Whether to ship `install.yml` as a standalone action (composite-style) in addition to the workflow form.
- Whether saga compensating workflows should be the same `release.yml` with a `mode: compensate` input, or separate `saga-compensate.yml`.
- Exact branch-protection interactions for auto-merge — some required-check configurations confuse the `enablePullRequestAutoMerge` mutation.
- Whether to publish a Versionx OCI image to `ghcr.io/kodydennon/versionx` for users who want to run the binary via container.
