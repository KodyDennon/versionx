# 05 — Release Orchestration

## Scope
How Versionx decides version bumps, generates changelogs, creates tags, publishes packages, and coordinates releases across multiple packages or repos. Supports PR-title parsing (default), Conventional Commits, Changesets-style intent files, manual bumps, and AI-assisted proposals via MCP — user's choice per repo.

**This is the Versionx wedge.** Cross-repo atomic release orchestration with plan/apply safety, polyglot version handling, and AI-as-client is the defensible feature no other tool has. Everything else is in service of this.

## Contract
After reading this file you should be able to: configure any supported release strategy, understand how AI proposals are generated and approved, and trace a full `versionx release` execution from planning through publish.

---

## 1. Release model

Versionx supports four release strategies, chosen per-repo:

| Strategy | How bumps are decided | Best for |
|---|---|---|
| `pr-title` **(default)** | Parse the merged PR title as conventional commit | Squash-merge teams, most GitHub workflows |
| `conventional` | Parse commit messages since last tag | Linear history, commit-discipline teams |
| `changesets` | Contributors write intent files | Monorepos with many contributors, human-curated notes |
| `manual` | Human edits `version` directly | Simple single-package repos |

**AI-assist** is an **orthogonal modifier** — any strategy can be overlaid with `ai_assist = "mcp"` (agent drives) or `ai_assist = "byo-api"` (versionx calls user's configured LLM API). AI proposes; humans approve.

Configuration (from `versionx.toml`):
```toml
[release]
strategy = "pr-title"            # default; see table above
ai_assist = "mcp"                # "mcp" | "byo-api" | "off"
versioning = "semver"            # "semver" | "calver" | "custom"
tag_template = "v{version}"      # or "{package}@{version}" for monorepos
changelog = "CHANGELOG.md"
plan_ttl = "24h"                 # 1h–30d
push_mode = "prompt"             # "prompt" (TTY) | "explicit" (--push/--no-push required)
```

### 1.1 Why PR-title is the default

Research finding: **squash-merge destroys Conventional Commits signal** in most GitHub workflows. The trunk history becomes one commit per PR; classic commit parsing loses fidelity.

The modern answer is to enforce PR titles as conventional commits (see `amannn/action-semantic-pull-request`) and parse them at merge. This is what release-please users do in practice and what semantic-release users move to after enough pain.

versionx ships a GitHub Action that validates PR titles pre-merge and an auto-generated workflow template for enforcement.

---

## 2. Versioning schemes

### 2.1 SemVer (default)
Standard `MAJOR.MINOR.PATCH` with optional prerelease (`-beta.1`) and build metadata (`+sha.abc123`). Full [semver.org 2.0.0](https://semver.org/) compliance.

### 2.2 CalVer
Calendar-based (e.g., `2026.04.18`, `2026.4`, `26.4.1`). Configured via template:
```toml
[release]
versioning = "calver"
calver_template = "{YYYY}.{MM}.{patch}"
```
Bump kinds map: "major"/"minor" → tick the date component; "patch" → increment patch suffix.

### 2.3 Custom
Pluggable versioner via a small Rust trait, for unusual schemes.

### 2.4 Cross-ecosystem version translation (load-bearing)

**npm/cargo/pip disagree on version semantics:**
| Range | npm | Cargo | pip (PEP 440) |
|---|---|---|---|
| `^0.2.3` | `>=0.2.3 <0.3.0` | `>=0.2.3 <0.3.0` | `>=0.2.3 <0.3.0` (Poetry) |
| `^0.0.3` | `>=0.0.3 <0.0.4` (exact patch) | `>=0.0.3 <0.0.4` | same |
| Pre-release `1.0.0-rc.1` | hyphen, dot identifiers | same as npm | `1.0.0rc1` (**no hyphen** — PEP 440) |
| Build metadata `+sha.abc` | allowed, ignored for precedence | allowed | `+abc` = PEP 440 local version label, **different precedence** |

Versionx maintains a **canonical internal version** plus per-ecosystem renderers. Pre-release tags translate (`-rc.1` → `rc1` for PyPI). When a release crosses ecosystems with lossy conversion, versionx emits a **loud warning** and requires confirmation.

---

## 3. PR-title strategy (default)

### 3.1 Parsing
When a PR merges to the release branch:
1. Read the squash-merged commit's message (which equals the PR title when GitHub's "Default to PR title for squash merge commits" is enabled, or the workflow passes the title explicitly via `${{ github.event.pull_request.title }}`).
2. Parse as Conventional Commit.

Mapping:
| Type | Bump |
|---|---|
| `feat:` | minor |
| `fix:` / `perf:` | patch |
| `feat!:` / `fix!:` / footer `BREAKING CHANGE:` | major |
| `chore:`, `docs:`, `style:`, `refactor:`, `test:`, `ci:`, `build:` | none (skipped in changelog by default) |
| Non-conventional | Configurable: include as patch / skip / error |

Scopes (`feat(api):`) become changelog grouping keys.

### 3.2 PR title enforcement
Versionx ships `acme/versionx-pr-title-action` — a GitHub Action that validates PR titles match the conventional-commit regex, with `validateSingleCommit` handling the edge case where GitHub uses the commit message for single-commit PRs.

### 3.3 Monorepo path filtering
For monorepos, each package's release looks only at PRs touching that package's path:
```toml
[release.packages."packages/ui"]
paths = ["packages/ui/**", "packages/shared/**"]
```

### 3.4 Fallback to commits
If the PR title is missing or the workflow isn't wired, versionx falls back to conventional-commit parsing over the git log. Warns loudly.

---

## 4. Conventional Commits strategy (classic)

For teams with commit discipline and linear (non-squash) history.

### 4.1 Parsing
Commits since the last tag (or repo root) are parsed per [Conventional Commits 1.0](https://www.conventionalcommits.org/en/v1.0.0/). Mapping identical to §3.

### 4.2 Limitations
Requires commit discipline. Squash merges destroy signal. versionx provides a `versionx check-commits` pre-push hook to enforce.

---

## 5. Changesets strategy

### 5.1 Philosophy
Explicitly inspired by [@changesets/cli](https://github.com/changesets/changesets). Contributors write intent as they develop; releases aggregate intent.

### 5.2 Workflow
1. Contributor runs `versionx changeset add` → interactive TUI prompts for affected packages, bump type, summary.
2. A file is written: `.versionx/changesets/<slug>.md`.
3. File is committed in the same PR as the code.
4. At release time, `versionx release` reads all changeset files, aggregates bumps per package, updates versions, updates changelogs, deletes the changeset files.

### 5.3 Changeset file format
```markdown
---
"@acme/ui": minor
"@acme/utils": patch
---

Add dark mode support to Button component.

Closes #123.
```

Frontmatter: package identifier → bump kind. Body: human-readable changelog entry (first line = summary, rest = details).

### 5.4 Non-interactive entry (for AI)
`versionx changeset add --package @acme/ui --bump minor --summary "Add dark mode" --details-file notes.md` creates the file programmatically. MCP tool `versionx_changeset_add` wraps this for AI agents.

### 5.5 Validation
`versionx changeset check`:
- Every changeset references real packages.
- No bump-less changesets.
- Optional: require at least one changeset per PR (CI check).
- Rejects `*` package specs for internal workspace deps (changesets issue #1728).

### 5.6 Prerelease and snapshot modes
Changesets' prerelease mode is infamously a footgun. versionx implements:
- **Snapshot releases**: `versionx release snapshot --tag=canary` → throwaway versions like `0.5.0-canary-20260418180000+sha.abc123`. Validates tag against ecosystem-specific regex (rejects underscores, reserved tags like `latest`). Solves Changesets issue #419.
- **Prerelease mode**: `versionx release prerelease enter rc` / `versionx release prerelease exit`. State machine is explicit; single command to exit safely. Audit trail in state DB.

### 5.7 peerDependencies handling
Bumping a peerDependency forces a major bump by default (Changesets #822 behavior). Override per-package:
```toml
[release.packages."@acme/ui"]
peer_dep_policy = "major"  # "major" | "minor" | "patch" | "explicit"
```

---

## 6. Manual strategy

For repos that just want a tag command:
```bash
versionx release bump major
versionx release bump minor
versionx release bump patch
versionx release bump 1.2.3    # explicit
```

Still generates changelog from commits (optional), creates tag, publishes.

---

## 7. AI-assisted overlay (no bundled model, ever)

### 7.1 The core principle
**Versionx does not ship an LLM.** The AI signal comes from one of:
1. **MCP path (`ai_assist = "mcp"`)** — an external agent (Claude Code, Codex, Cursor, Qwen, etc.) connects via MCP and drives the release workflow. The agent's own LLM does all reasoning.
2. **BYO API key path (`ai_assist = "byo-api"`)** — versionx calls the user's configured LLM API directly (for headless CI use, where no agent is in the loop).
3. **No AI (`ai_assist = "off"`)** — deterministic only. Uses git-cliff for changelog skeletons.

Rationale: bundling a model adds ~1.5GB to the binary, locks you into model licensing headaches, and any bundled model is obsolete in 6 months. MCP lets users pair versionx with whatever frontier model they already have.

### 7.2 What the AI produces

Whether via MCP or BYO API, the AI produces a structured proposal:
```json
{
  "per_package": [
    {
      "package": "@acme/ui",
      "current_version": "1.4.2",
      "suggested_bump": "minor",
      "confidence": 0.85,
      "rationale": "New exported Button prop `variant`; no existing API removed.",
      "suggested_changelog": "- Add `variant` prop to Button supporting 'ghost' and 'outline'.\n- Fix Tab keyboard navigation regression."
    }
  ],
  "breaking_changes_detected": [],
  "warnings": ["No changeset file for recent change in `packages/utils`; suggest adding one."]
}
```

### 7.3 Voice-aware changelog prose

This is a genuine differentiator. Research found no existing tool produces non-generic LLM changelogs. Versionx's approach:

**Inputs fed to the LLM (via MCP tool context or BYO API prompt):**
- Full `README.md` (project voice, terminology)
- Full `CHANGELOG.md` (recent style, length, tone)
- Last 10 release notes or git tag messages (voice samples)
- Diff since last tag
- Conventional-commit / changeset signals

**Persistence**: Per-package style samples cached in the state DB `changelog_voice` table — so repeat generations stay consistent.

**Workflow (MCP path, primary)**:
1. Agent calls `versionx_release_propose` → receives structured context (diff, samples, commit metadata).
2. Agent's LLM generates voice-aware prose.
3. Agent calls `versionx_release_set_changelog_draft` with the generated text.
4. versionx writes it to a PR comment (if in a PR workflow) or directly into CHANGELOG.md in a draft commit.
5. Human reviews the PR comment; can edit inline on GitHub; re-trigger regenerates against new signals.
6. On approve, `versionx release apply` commits final CHANGELOG.

**Workflow (BYO API path)**: versionx sends a single prompt to the configured endpoint, receives changelog prose, writes it. No agent loop.

### 7.4 Safety rails

- AI never auto-merges or auto-tags. `ai_assist` at most **proposes**; approval is mandatory.
- Approval is an explicit action: `versionx release approve <plan-id>`, a GitHub PR comment `/approve`, or MCP `versionx_release_apply`.
- Proposed plans expire at `[release] plan_ttl` (default 24h, configurable 1h–30d).
- Each AI proposal is recorded in the `releases.ai_proposed` column with provider identifier (`mcp://claude-code/<session>` or `byo-api://anthropic`).
- **Tool outputs are untrusted w.r.t. prompt injection.** User-content fields (commit messages, changelogs, ticket descriptions) that flow into versionx's MCP responses are sanitized/fenced to prevent injection into the calling agent's reasoning (OWASP LLM Top 10 #1).

### 7.5 Breaking-change signal detection

Beyond PR-title/commits/changesets, a rule-based scorer detects breaking-change likelihood:
- API surface diffs (exported symbols via lightweight TS/Python/Rust parsers).
- Public config/schema changes.
- Removed/renamed endpoints in OpenAPI / gRPC protos.
- Migration files (database schema).
- Breaking changes in tests (a removed test case often signals removed API).

The AI sees this signal in its context and can disagree with a justification.

---

## 8. Release plan — the unified data structure

Every release produces a `ReleasePlan`:

```rust
pub struct ReleasePlan {
    pub id: String,                            // blake3 hash of the plan JSON
    pub repo: RepoRef,
    pub created_at: DateTime<Utc>,
    pub strategy: Strategy,
    pub ai_proposed: bool,
    pub ai_provider: Option<String>,           // "mcp://claude-code/<session>" | "byo-api://anthropic" | None
    pub pre_requisite_hash: String,            // config_hash at plan time
    pub expires_at: DateTime<Utc>,             // created_at + plan_ttl
    pub bumps: Vec<PackageBump>,
    pub changelogs: Vec<ChangelogEntry>,
    pub tags_to_create: Vec<TagSpec>,
    pub commits_to_make: Vec<CommitSpec>,
    pub publish_targets: Vec<PublishTarget>,
    pub warnings: Vec<Warning>,
}
```

### 8.1 Plan lifecycle
1. `versionx release propose` → plan written to `state.plans`, optionally AI-proposed.
2. Human/agent reviews: `versionx release show <id>` or via PR comment / web UI.
3. `versionx release approve <id>` → plan marked approved; `approved_by` recorded.
4. `versionx release apply <id>` → executed. Re-verifies `pre_requisite_hash` matches current state (refuses if state drifted).
5. Post-execution: plan status = `completed` | `failed` | `partial`; rollback procedure attached.

### 8.2 Push behavior

`versionx release apply` stops at local commit by default; pushing is a separate step.

- **TTY + `push_mode = "prompt"`**: interactive "push now? [y/N]" with default **No** for safety.
- **Non-TTY (CI)**: `--push` or `--no-push` **must** be passed explicitly, or versionx errors out. No silent default for CI.
- **`push_mode = "explicit"`**: always require `--push` / `--no-push`, even in TTY.

---

## 9. Monorepo releases

### 9.1 Per-package versioning
Each package in a monorepo can version independently (like pnpm/changesets) or lockstep (like Nx "fixed"). Controlled per `[release.packages.*]`.

### 9.2 Dependency graph
When package A depends on package B, and B bumps, A may need a bump too. Rules:
- A's `dependencies[B]` uses `^` or `~` → auto-bump A's patch (configurable).
- A's `dependencies[B]` is exact → A bumps the same way B did.
- A's `peerDependencies[B]` in breaking range → A bumps major.

Computed from native manifest parsing (`package.json`, `Cargo.toml`, etc.) across workspaces.

### 9.3 Release ordering
Topological sort of the package DAG. Leaves (no intra-repo dependents) release first; roots last. Tie-break alphabetically within a topo level for determinism.

### 9.4 Mixed-ecosystem monorepo
A monorepo with a Rust `cargo` crate, a Node `pnpm` package, and a Python `uv` package gets three publishes, one plan, ordered correctly across ecosystems.

---

## 10. Cross-repo releases (the wedge)

### 10.1 The "release set"
```bash
versionx fleet release propose --set customer-portal
```
Where `customer-portal` is defined in the fleet ops repo's `versionx-fleet.toml`:
```toml
[sets.customer-portal]
repos = [
  "acme/portal-frontend",
  "acme/portal-api",
  "acme/portal-shared",
]
strategy = "coordinated"           # all bump together or none does
```

### 10.2 Coordination modes
- `independent`: each repo's release is independent; versionx runs them in parallel.
- `coordinated`: all repos must agree on feasibility (no policy deny, no failed tests); one commits ⇒ all commit; rollback if any fails.
- `gated`: explicit dependency order; `frontend` won't release until `api` succeeds.

### 10.3 Saga pattern & interactive rollback

True cross-repo ACID isn't possible (git push is not transactional; registries have asymmetric unpublish policies — crates.io yanks-only, PyPI never, npm 72hr window, Maven Central never).

Versionx approximates with a **saga**:
1. **Dry-run all repos** (publish to test registries if configured, or `--dry-run` against the real registry).
2. **Tag all repos locally** (reversible).
3. **Publish in topological order** (leaves first).
4. **If any publish fails**: pause and prompt the user with three options:
   - **Manual rescue**: Spawn a sub-shell in the failing repo's directory with `VERSIONX_RELEASE_STATE` injected. User fixes and resumes.
   - **Auto-revert**: versionx creates and pushes reverting commits (`revert: chore(release): ...`) to all repos that already received a release commit.
   - **Best-effort yank**: Attempt to yank/unpublish the successful uploads where the registry supports it (npm 72hr, crates.io yank, RubyGems yank).

5. **On success**: push all release commits + tags, run post-release hooks.

Honest messaging: "best-effort atomic, not truly atomic." Documented prominently. Failure modes enumerated in the playbook.

### 10.4 Cross-ecosystem coordination
When frontend (Node) and backend (Rust) release together, Versionx:
- Canonicalizes versions internally.
- Renders each per its ecosystem (`-rc.1` for npm, `rc1` for PyPI).
- Warns loudly on lossy conversions.
- Records all forms in `releases` table.

---

## 11. Publish step

After versions are bumped and tags created, Versionx invokes each ecosystem adapter's `publish` method.

### 11.1 Credentials — OIDC first
Versionx **prefers OIDC trusted publishing** everywhere it's supported (all GA as of 2025-2026):

| Registry | OIDC status | Fallback |
|---|---|---|
| npm | GA July 2025 (npm CLI ≥ 11.5.1, Node ≥ 22.14) | `NODE_AUTH_TOKEN`, `.npmrc` `_authToken` |
| PyPI | GA April 2023 | `TWINE_PASSWORD`, keychain |
| crates.io | GA July 2025 (RFC 3691) | `CARGO_REGISTRY_TOKEN`, `~/.cargo/credentials.toml` |
| RubyGems | GA Dec 2023 | API key |
| GitHub Releases | Native `GITHUB_TOKEN` / App install token | n/a |
| Maven Central | **Not supported** — document loudly | User token via Sonatype Central Portal |
| Container registries | OIDC federation to ECR/GCR/etc. | Docker config |

Detection: in CI env (`GITHUB_ACTIONS`, `GITLAB_CI`, `CIRCLECI`), try OIDC first. Fall back to token env var. **Warn loudly on token use** so users can migrate.

Policy can require OIDC (no long-lived tokens) via:
```toml
[release.security]
require_trusted_publishing = true
```

### 11.2 Rate limits (crates.io in particular)
crates.io: 5 new crates burst then 1/10min; 30 new versions burst then 1/min. Workspace publishes with many crates hit limits (issue #1643). versionx's publish executor serializes with sleep per registry's documented limits.

### 11.3 Provenance (npm)
As of npm CLI 11.5.1+, trusted publishing **auto-generates provenance attestations** — `--provenance` flag no longer needed. versionx surfaces a warning when publishing without provenance (e.g., to Maven Central).

---

## 12. Dry-run and preview

Every release command supports `--dry-run`:
- Computes the plan.
- Executes adapter plans in "preview" mode (most adapters support `--dry-run` natively).
- Does not mutate git.
- Does not publish.
- Emits the plan + predicted outcome.

Used in CI for PR previews showing "if this PR merges, here's what will be released."

---

## 13. Rollback

Releases are recorded. `versionx release rollback <release-id>` attempts:
1. Revert version-bump commits (or prepare a reverting commit).
2. Delete tags (with warning — published artifacts remain).
3. Unpublish from registries where permitted (usually only very-recent publishes).
4. Restore changeset files if the release consumed them.

Documented as **best-effort**; published packages generally cannot be truly unpublished. Docs emphasize: "rollback is for fixing mistakes quickly, not retroactive history manipulation."

---

## 14. Observability

Every release emits events:
- `release.propose.start`, `release.propose.complete` (with plan ID)
- `release.approve`, `release.reject`
- `release.bump.applied`, `release.changelog.written`
- `release.tag.created`, `release.commit.pushed`
- `release.publish.start { target }`, `release.publish.complete`
- `release.failed { stage, error }`, `release.rollback.*`

These flow to the TUI, MCP progress notifications, CI logs, and the state DB for audit.

---

## 15. Testing

- Unit: PR-title parser, commit parser, changeset parser, dep-graph bump computation, version translator (npm ↔ PEP 440 ↔ semver).
- Property: any release plan, applied to a fresh clone of its source state, produces the declared outcome.
- Fixture: a "recipe book" of monorepo scenarios (one package, fixed vs independent, breaking change, circular deps error) with captured plans.
- E2E: full release into a test npm registry (verdaccio) + test PyPI + test cargo. Cross-repo E2E with one member intentionally failing — verify saga rollback.

---

## 16. Non-goals

- **Not a changelog curation UI.** The changelog file is plain markdown; edit it post-hoc.
- **Not an LLM host.** versionx exposes context to AI; AI lives elsewhere (MCP client or BYO API).
- **Not a git provider replacement.** We use git + GitHub/GitLab APIs; we don't host.
- **Not enforcing any particular branching model.** Trunk-based, GitFlow, release branches — all work.
