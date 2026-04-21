# Deep CI Integration — Parallel Agent Dispatch Plan

**Companion to:** `2026-04-20-deep-ci-integration.md` (the task plan).

This document lays out **waves** of work where agents inside a wave touch disjoint files and can run in parallel without conflicts. Waves run sequentially; within each wave, every agent can be dispatched concurrently.

---

## Wave index

| Wave | Phase | Agents in wave | Parallel? | Depends on |
|---|---|---|---|---|
| 1 | A — foundation | 1 | — | nothing |
| 2 | A — modules | 5 | ✅ parallel | Wave 1 |
| 3 | A — wiring | 1 | — | Wave 2 |
| 4 | B — comments + subcommands | 1 | — | Wave 3 |
| 5 | C — App + release PR + publishing | 4 | ✅ parallel | Wave 4 |
| 6 | C — glue & workflow | 1 | — | Wave 5 |
| 7 | D — dep updates + saga | 1 | — | Wave 6 |
| 8 | E — GitLab + Bitbucket + Gitea | 3 | ✅ parallel | Wave 6 (Phase C trait surface is what they need; D is not required) |
| 9 | E — cross-forge glue | 1 | — | Wave 8 |

**Total: 9 waves, ~17 agent dispatches.** Compared to 90 sequential tasks, this is roughly a 4–5× wall-clock compression.

---

## Wave 1 — Phase A foundation (1 agent, sequential)

**Why one agent:** Tasks A1–A8 all build up `crates/versionx-forge-trait/src/*`, and each module depends on prior modules (e.g., `context.rs` references `Capabilities`). Parallelizing would cause merge conflicts in `lib.rs` and create import ordering issues.

### Agent 1.1 — `forge-trait foundation`

**Scope:**
- Add `crates/versionx-forge-trait` to workspace.
- Implement tasks A1 through A8 from the plan, sequentially.
- Add `bitflags = "2.6"` to workspace dependencies.

**Files owned:**
- `Cargo.toml` (workspace root — add member + dep)
- `crates/versionx-forge-trait/**` (entire crate)

**Does NOT touch:** Any other crate.

**Done when:**
- `cargo check -p versionx-forge-trait` succeeds.
- `cargo test -p versionx-forge-trait` passes all unit tests.
- Eight commits on main (one per task A1..A8) following Conventional Commit style.

---

## Wave 2 — Phase A module implementations (5 agents, parallel)

**Why parallel:** Each agent owns a disjoint set of files inside `crates/versionx-github/src/`. They all depend on Wave 1's trait crate being available, but not on each other. They produce independent commits that merge trivially.

### Agent 2.1 — `github Cargo + lib.rs scaffold`

**Scope:** Task A9 plus the `lib.rs` scaffold from Task A10 step 2 and Task A10 step 3 (creating empty module stubs so the rest of the crate compiles).

**Files owned:**
- `crates/versionx-github/Cargo.toml`
- `crates/versionx-github/src/lib.rs`
- Empty stubs: `crates/versionx-github/src/{annotations,app,check_run,client,context,dispatch,identity,merge,pr_comment,publish,release_pr,token}.rs` (each a `//! stub` single-line file)

**Does NOT touch:** Any body of the stubbed modules.

**Done when:** `cargo check -p versionx-github` compiles (with the stubs).

---

### Agent 2.2 — `github token + client`

**Scope:** Tasks A11 and A12.

**Files owned:**
- `crates/versionx-github/src/token.rs`
- `crates/versionx-github/src/client.rs`

**Does NOT touch:** Anything else in the workspace.

**Depends on:** Agent 2.1 having created the empty stubs.

**Done when:** `cargo test -p versionx-github token` and `cargo test -p versionx-github client` both pass.

---

### Agent 2.3 — `github context + capability probe`

**Scope:** Tasks A10 (real implementation of `context.rs`) and A13 (live probe).

**Files owned:**
- `crates/versionx-github/src/context.rs`
- `crates/versionx-github/tests/context_probe.rs`

**Does NOT touch:** `token.rs`, `client.rs` (Agent 2.2), `annotations.rs` (Agent 2.4), etc.

**Depends on:** Agents 2.1 + 2.2 (uses `token::discover` and `ResolvedToken`).

**Done when:** `cargo test -p versionx-github context` + `cargo test -p versionx-github --test context_probe` pass.

---

### Agent 2.4 — `github annotations + snapshots`

**Scope:** Tasks A14 and A21.

**Files owned:**
- `crates/versionx-github/src/annotations.rs`
- `crates/versionx-github/tests/annotation_snapshots.rs`

**Does NOT touch:** Any other file.

**Depends on:** Agent 2.1's stub for `annotations.rs` to exist.

**Done when:** `cargo test -p versionx-github annotations` + snapshot test pass.

---

### Agent 2.5 — `github check runs + status fallback`

**Scope:** Tasks A15 and A16.

**Files owned:**
- `crates/versionx-github/src/check_run.rs`
- `crates/versionx-github/tests/check_run.rs`

**Does NOT touch:** `context.rs`, `token.rs`, `annotations.rs`.

**Depends on:** Agents 2.1 + 2.2 (uses `GhClient`).

**Done when:** `cargo test -p versionx-github --test check_run` and `cargo test -p versionx-github check_run::unit_tests` pass.

---

### Agent 2.6 — `core integrations module`

**Scope:** Task A18.

**Files owned:**
- `crates/versionx-core/src/integrations.rs`
- `crates/versionx-core/src/lib.rs` (add one `pub mod` line — coordinate with other agents if this lib.rs is touched by anything else; in this wave, no one else edits it)

**Does NOT touch:** Any versionx-github file.

**Depends on:** Wave 1 only.

**Done when:** `cargo check -p versionx-core` compiles, reporter traits compile.

---

### Wave 2 merge gate

All 5 agents return summaries. Verify:

1. No file conflicts (each agent owns disjoint paths).
2. `cargo check --workspace` green.
3. `cargo test --workspace` green.
4. Commit log shows ~10–12 commits from the wave.

---

## Wave 3 — Phase A CLI wiring (1 agent, sequential)

**Why one agent:** Tasks A17, A19–A26 mostly touch `crates/versionx-cli/src/main.rs` — one giant file. Parallel agents would merge-conflict on every hunk.

### Agent 3.1 — `cli forge wiring + github detect subcommand + Phase A docs`

**Scope:** Tasks A17 through A26 (forge meta-crate, CLI startup forge detection, `versionx github detect`, reporter wiring into release propose, Phase-A e2e smoke, fmt/clippy pass, docs updates).

**Files owned:**
- `crates/versionx-forge/**`
- `crates/versionx-cli/Cargo.toml`
- `crates/versionx-cli/src/main.rs`
- `crates/versionx-github/src/reporters.rs`
- `crates/versionx-github/src/lib.rs` (adds `pub mod reporters`)
- `crates/versionx-github/tests/phase_a_e2e.rs`
- `website/docs/contributing/architecture.md`
- `website/docs/integrations/github-actions.md`
- Root `Cargo.toml` (new workspace member)

**Does NOT touch:** Any other crate's internals.

**Depends on:** Wave 2 complete.

**Done when:** All Phase A end-gate checks pass.

---

## Wave 4 — Phase B (1 agent, sequential)

**Why one agent:** All 7 Phase B tasks either touch the same files (sticky-comment client and its tests, reporters rewrite) or are small enough that the overhead of parallelism exceeds the gain. And `reporters.rs` is the final integration point — only one agent can sensibly own the rewrite.

### Agent 4.1 — `sticky comments + escape-hatch subcommands`

**Scope:** Tasks B1 through B7.

**Files owned:**
- `crates/versionx-github/src/pr_comment.rs`
- `crates/versionx-github/src/templates.rs`
- `crates/versionx-github/src/lib.rs` (add two module declarations)
- `crates/versionx-github/src/reporters.rs` (full rewrite with state)
- `crates/versionx-github/Cargo.toml` (adds `versionx-core` back-dep)
- `crates/versionx-github/tests/pr_comment_list.rs`
- `crates/versionx-github/tests/phase_b_e2e.rs`
- `crates/versionx-cli/src/main.rs` (adds Comment + CheckRun subcommands)
- `website/docs/**` (docs regen via `cargo xtask docs`)

**Depends on:** Wave 3.

**Done when:** Phase B end-gate checks pass.

---

## Wave 5 — Phase C parallel (4 agents)

**Why parallel:** Four independent slices of Phase C touch disjoint file territory.

### Agent 5.1 — `github App identity`

**Scope:** Task C1.

**Files owned:**
- `crates/versionx-github/src/app.rs`
- `crates/versionx-github/tests/app.rs`
- `crates/versionx-github/tests/fixtures/test-app-key.pem`

**Does NOT touch:** `release_pr.rs`, `publish.rs`, `pr_comment.rs`.

**Depends on:** Wave 4 (needs the crate to exist; doesn't share code with Phase B).

**Done when:** `cargo test -p versionx-github --test app` passes.

---

### Agent 5.2 — `release PR client`

**Scope:** Tasks C2 through C10, plus C17 (GitHub Release creation).

**Files owned:**
- `crates/versionx-github/src/release_pr.rs`
- `crates/versionx-github/tests/release_pr.rs`
- `crates/versionx-github/tests/release_pr_<op>.rs` for each of C3–C10 (one test file per operation)
- `crates/versionx-github/tests/release_pr_github_release.rs`
- `crates/versionx-github/Cargo.toml` (add `urlencoding = "2"`)
- Root `Cargo.toml` (add to workspace deps — coordinate with other Wave 5 agents; only one should edit)

**Does NOT touch:** `app.rs`, `publish.rs`, `pr_comment.rs`.

**Depends on:** Wave 4.

**Done when:** Every release-PR operation test passes.

---

### Agent 5.3 — `publish drivers`

**Scope:** Tasks C11, C12, C13, C14.

**Files owned:**
- `crates/versionx-github/src/publish.rs`
- `crates/versionx-github/tests/publish_node.rs`
- `crates/versionx-github/tests/publish_rust.rs`
- `crates/versionx-github/tests/publish_python.rs`
- `crates/versionx-github/tests/publish_oci.rs`

**Does NOT touch:** `app.rs`, `release_pr.rs`, `pr_comment.rs`.

**Depends on:** Wave 4.

**Done when:** All four driver unit tests pass (integration tests against live registries are `#[ignore]`-gated).

---

### Agent 5.4 — `[github] config schema additions`

**Scope:** Task C15 (schema only — routing wiring lands in Wave 6).

**Files owned:**
- `crates/versionx-config/src/schema.rs` (add `GithubConfig` + `GithubPublishConfig`)
- `crates/versionx-config/src/lib.rs` (add re-exports)

**Does NOT touch:** Any versionx-github or versionx-core file.

**Depends on:** Wave 4 (no actual dep on B, but sequencing keeps commit history tidy).

**Done when:** `cargo check -p versionx-config` + existing schema tests pass.

---

### Wave 5 merge gate

- `cargo check --workspace` green.
- Agents 5.1, 5.2, 5.3 do not collide on `Cargo.toml` — only Agent 5.2 edits it (for `urlencoding`); Agents 5.1 and 5.3 use only already-in-workspace deps.
- Combined test suite runs clean.

---

## Wave 6 — Phase C glue & workflow (1 agent, sequential)

**Why one agent:** Tasks C15 routing wire-up, C16 (`versionx github publish`), C18 (reusable workflow), C19 (App registration doc), C20 (fmt/clippy/docs) all touch `main.rs` or shared paths.

### Agent 6.1 — `Phase C integration`

**Scope:** Tasks C15 routing, C16, C18, C19, C20.

**Files owned:**
- `crates/versionx-core/src/commands/release/apply.rs` (or wherever core apply lives — route publish through config)
- `crates/versionx-cli/src/main.rs` (adds `github publish` subcommand + wires reporters into dep-update verb; wires publish routing call site)
- `.github/workflows/release.yml`
- `docs/spec/12-github-app-registration.md`
- `website/docs/**` (docs regen)

**Depends on:** Wave 5 complete.

**Done when:** Phase C end-gate checks pass. Demo repo pushes a tag end-to-end using `uses: KodyDennon/versionx/.github/workflows/release.yml@phase-c`.

---

## Wave 7 — Phase D (1 agent, sequential)

**Why one agent:** Tasks D1–D10 interleave: the saga protocol (D6) uses `GhDispatchClient` from D1 and `GhAutoMergeClient` from D2, both of which are in the same crate. The reusable-workflow yaml files (D4/D5/D7/D8) are small; their pairing with the Rust implementations is tight. Trying to parallelize creates coordination overhead that exceeds the time savings.

### Agent 7.1 — `Phase D orchestrator`

**Scope:** Tasks D1 through D10.

**Files owned:**
- `crates/versionx-github/src/dispatch.rs`
- `crates/versionx-github/src/merge.rs`
- `crates/versionx-github/tests/dispatch.rs`
- `crates/versionx-github/tests/merge.rs`
- `crates/versionx-core/src/saga/**` (new saga module)
- `crates/versionx-state/src/**` (saga-table migration)
- `crates/versionx-cli/src/main.rs` (`update --mode pr`, `saga` CLI surface)
- `.github/workflows/update.yml`
- `.github/workflows/policy.yml`
- `.github/workflows/saga.yml`
- `.github/workflows/install.yml`
- `.github/workflows/e2e.yml`
- `scripts/e2e-sandbox-setup.sh`
- `crates/versionx-github/tests/e2e/**`
- `website/docs/**` (docs regen)

**Depends on:** Wave 6.

**Done when:** Phase D end-gate checks pass.

---

## Wave 8 — Phase E forge implementations (3 agents, parallel)

**Why parallel:** GitLab, Bitbucket, and Gitea are fully independent crates. They don't share Rust code beyond what's in `versionx-forge-trait` (already done in Wave 1). They don't touch each other's files, each other's tests, or each other's docs.

### Agent 8.1 — `gitlab forge`

**Scope:** Tasks E1 through E15.

**Files owned (exclusively):**
- `crates/versionx-gitlab/**` (entire new crate)
- `website/docs/integrations/gitlab.md`
- `.gitlab/ci/**` (reusable GitLab CI snippets)

**Shared files it writes to (coordinate):**
- `Cargo.toml` root — add workspace member + dep
- `crates/versionx-forge/Cargo.toml` + `src/lib.rs` — add GitLab re-export and `detect()` branch
- `crates/versionx-config/src/schema.rs` — add `[gitlab]` block

For the shared files, Agent 8.1 appends its lines at the established insertion points; Agents 8.2 and 8.3 do the same; collisions resolve to three independent non-overlapping hunks. In practice, have one of the three agents merge all three schema/forge additions in a final pass; see Wave 9.

**Done when:** `versionx gitlab detect` prints context inside a GitLab CI job on a sandbox GitLab project.

---

### Agent 8.2 — `bitbucket forge`

**Scope:** Tasks E16 through E30.

**Files owned (exclusively):**
- `crates/versionx-bitbucket/**`
- `website/docs/integrations/bitbucket.md`
- `bitbucket-pipelines/**` (reusable pipeline YAML snippets)

**Shared files:** same deferred-merge pattern as Agent 8.1.

**Done when:** `versionx bitbucket detect` works inside a Bitbucket Pipelines job.

---

### Agent 8.3 — `gitea forge`

**Scope:** Tasks E31 through E45.

**Files owned (exclusively):**
- `crates/versionx-gitea/**`
- `website/docs/integrations/gitea.md`
- `.gitea/workflows/**` (reusable snippets)

**Shared files:** same deferred-merge pattern.

**Done when:** `versionx gitea detect` works inside a Gitea Actions workflow.

---

### Wave 8 merge gate

1. Each of the three agents delivers their crate + docs page.
2. Shared-file collisions are resolved as follows:
   - Each agent produces its "additions to workspace Cargo.toml" and "additions to forge detect()" and "additions to config schema" as a separate commit that cherry-picks cleanly.
   - Wave 9's agent folds all three into the shared files in one pass.

---

## Wave 9 — Phase E cross-forge glue (1 agent, sequential)

### Agent 9.1 — `cross-forge integration + final green`

**Scope:** Tasks E46, E47, E48, E49. Fold all three Wave 8 agent contributions into shared files.

**Files owned:**
- Root `Cargo.toml` (merge workspace members + deps from Wave 8 agents)
- `crates/versionx-forge/Cargo.toml` + `src/lib.rs` (merge re-exports + detect() branches in priority order: GitHub → GitLab → Bitbucket → Gitea → None)
- `crates/versionx-config/src/schema.rs` (merge `[gitlab]` + `[bitbucket]` + `[gitea]` blocks)
- `crates/versionx-forge-trait/tests/identity_cross_forge.rs`
- `website/docs/integrations/ci-integration.md` (new overview page — rename/restructure existing `github-actions.md`)

**Depends on:** Wave 8 complete.

**Done when:** Phase E end-gate checks pass.

---

## Dispatch commands (copy-paste ready)

Below are the exact `Task(...)` invocations for each wave. Fill in the prompt bodies from this doc + the implementation plan.

### Wave 1

```
Agent 1.1 — forge-trait foundation
Prompt: "Execute tasks A1–A8 from docs/superpowers/plans/2026-04-20-deep-ci-integration.md.
Own: Cargo.toml workspace edits + crates/versionx-forge-trait/** entirely.
Do NOT touch any other crate. Follow TDD in each task. Commit per task with Conventional Commit messages.
Verify: cargo check -p versionx-forge-trait && cargo test -p versionx-forge-trait pass.
Return: list of commits created + any issues encountered."
```

### Wave 2 (5 parallel)

```
Agent 2.1 — github Cargo.toml + stubs
Agent 2.2 — github token + client
Agent 2.3 — github context + probe
Agent 2.4 — github annotations + snapshots
Agent 2.5 — github check runs + fallback
Agent 2.6 — core integrations
```

Each gets a prompt referencing its own file-ownership stanza above + its task numbers in the implementation plan.

### Wave 3

```
Agent 3.1 — CLI forge wiring + github detect + Phase A docs
```

### Wave 4

```
Agent 4.1 — sticky comments + escape-hatch subcommands (all of Phase B)
```

### Wave 5 (4 parallel)

```
Agent 5.1 — github App identity
Agent 5.2 — release PR client (all operations)
Agent 5.3 — publish drivers (node/rust/python/oci)
Agent 5.4 — [github] config schema
```

### Wave 6

```
Agent 6.1 — Phase C glue (publish routing, CLI subcommand, release.yml, App registration runbook)
```

### Wave 7

```
Agent 7.1 — Phase D end-to-end
```

### Wave 8 (3 parallel)

```
Agent 8.1 — GitLab forge
Agent 8.2 — Bitbucket forge
Agent 8.3 — Gitea forge
```

### Wave 9

```
Agent 9.1 — Cross-forge glue + final green
```

---

## Conflict-prevention rules

1. **Root `Cargo.toml`**: only one agent per wave touches it. Where multiple waves add workspace members (Wave 1 adds `versionx-forge-trait`, Wave 3 adds `versionx-forge`, Wave 8 adds the three forge crates), each wave's sequential agent handles the edit.
2. **`lib.rs` of each crate**: adding `pub mod X` lines is usually safe because different agents add different lines, but we pin ownership to one agent per wave anyway.
3. **`main.rs` of `versionx-cli`**: always owned by one agent per wave. CLI subcommand additions are serialized.
4. **`website/docs/**`** files that existed before the plan (overview pages, Contributing, architecture): owned by the wave's final sequential agent; parallel agents only create new files.
5. **CI workflows at `.github/workflows/`**: each wave that adds one owns that file exclusively.

Any agent that detects it needs to edit a file outside its declared ownership must stop, surface the conflict, and wait for the controller to resolve (likely by splitting the task or deferring to the wave's final agent).

---

## Wall-clock estimate

With humans fully available to review between waves:

- Wave 1: ~4h
- Wave 2 (parallel): ~4h (slowest agent sets the pace — 2.3 or 2.5)
- Wave 3: ~3h
- Wave 4: ~6h
- Wave 5 (parallel): ~8h (slowest — 5.2 release-PR client has 8 sub-operations)
- Wave 6: ~4h
- Wave 7: ~12h
- Wave 8 (parallel): ~20h (each forge is Phase-A-sized; slowest sets pace)
- Wave 9: ~6h

**Total wall clock: ~67h across 9 waves.** Fully sequential (no parallelism) would be roughly 2.5–3× longer. The big wins are Waves 2, 5, and 8 where independent file territory lets us stack work.

---

## Running this

The controller (you, or this session if it picks it up) dispatches each wave's agents simultaneously via multiple `Task(...)` calls in a single message, waits for all to return, reviews the summaries for conflicts, runs `cargo xtask ci` and `npm run build` in `website/` to gate the wave, then proceeds to the next.

Between waves, pause to:
1. Review each agent's commit log.
2. Run `cargo xtask ci` to verify the tree is green.
3. Push so CI on GitHub can confirm.
4. Regenerate docs (`cargo xtask docs`) if the wave touched any source-of-truth file (CLI args, config schema, error enum, MCP tool, RPC method).
