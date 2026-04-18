# 06 — Multi-repo & Monorepo Coordination

## Scope
How Versionx handles single repos, monorepos with internal workspaces, repos that reference external repos (submodules/subtrees/vendored), and "fleets" of independent repos viewed as a coordinated unit.

**Key principle** (from user direction): Versionx **reflects and works with whatever setup the user already has**. We do not impose a mechanism. Submodule users get submodule support; subtree users get subtree support; virtual-monorepo users get virtual monorepos. All strategies are first-class; users pick per-link what fits.

## Contract
After reading this file you should be able to: add an external repo dependency of any supported kind, coordinate a release across multiple repos, and understand how Versionx's view of a multi-repo workspace is assembled.

---

## 1. The four multi-repo modes

| Mode | Git mechanism | When to use |
|---|---|---|
| **Internal monorepo** | N/A — same git repo | Related packages, shared CI, single PR workflow |
| **Submodule** | `git submodule` | External code pinned by SHA, fetched on clone |
| **Subtree** | `git subtree` or `git-subrepo` | Vendored external code, possibly with bidirectional sync |
| **Virtual monorepo** | N/A — N separate repos grouped logically | Teams with many independent repos who want a unified view/policy/release |

Versionx supports all four simultaneously. A single workspace can have internal monorepo packages AND submodule links AND a virtual fleet grouping — they're not exclusive.

---

## 2. Internal monorepo (single repo, multiple packages)

### 2.1 Detection
A repo where multiple package manifests exist under different paths:
```
acme-monorepo/
├── versionx.toml
├── pnpm-workspace.yaml            # Node workspace
├── packages/
│   ├── ui/package.json
│   ├── utils/package.json
│   └── api/
│       ├── Cargo.toml             # Rust crate in the same monorepo
│       └── Cargo.lock
└── services/
    └── worker/
        ├── pyproject.toml         # Python service in the same monorepo
        └── uv.lock
```

### 2.2 Configuration
```toml
[ecosystems.node]
package_manager = "pnpm"
root = "."
workspaces = ["packages/*"]

[ecosystems.rust]
root = "packages/api"

[ecosystems.python]
root = "services/worker"
```

### 2.3 Operations
- `versionx sync` walks all three ecosystems, parallelized.
- `versionx release propose` computes bumps per package, respecting cross-package dependencies.
- Task runners delegate to existing tools (turbo, nx, make) — Versionx doesn't replace them in v1.

### 2.4 Cross-language internal deps
Rust crate depends on a locally-generated protobuf from the Node package? That's a task-runner concern, not a Versionx concern. Versionx ensures the versions are in sync; the user's build tool wires them together.

---

## 3. Submodule mode

### 3.1 Config
```toml
[links.shared-ui]
type = "submodule"
path = "vendor/shared-ui"
url = "https://github.com/acme/shared-ui.git"
track = "main"                     # or "tag:v*" or "commit:<sha>"
update = "pr"                      # "pr" | "auto" | "manual"
```

### 3.2 Operations
- `versionx links sync` runs `git submodule update --init --recursive` and resolves current SHAs into `versionx.lock`.
- `versionx links check-updates`: fetches the tracked ref of each submodule, compares to pinned SHA, produces a list of available updates.
- `versionx links update <name>`:
  - If `update = "pr"`: creates a branch, updates submodule SHA, opens PR via GitHub App.
  - If `update = "auto"`: commits directly (for repos on main-trunk-auto-merge workflows — dangerous, off by default).
  - If `update = "manual"`: only updates local working copy; user commits.

### 3.3 Recursive submodules
Supported natively (git handles recursion). Versionx's lockfile records only the top-level submodule SHA; the submodule's own submodules are git's concern.

### 3.4 Authentication
Versionx never prompts for git creds — relies on the user's configured git credentials (SSH keys, credential helper). GitHub App installations can optionally provide installation tokens for GitHub-hosted submodules.

---

## 4. Subtree mode

### 4.1 Config
```toml
[links.design-tokens]
type = "subtree"
path = "packages/tokens"
url = "https://github.com/acme/tokens.git"
track = "main"
bidirectional = true               # can push patches upstream
squash = true                      # squash upstream history when pulling
```

### 4.2 Mechanism
Versionx uses `git subtree` (built into git) by default. Optionally `git-subrepo` if installed and configured per-link (`mechanism = "subrepo"`).

### 4.3 Operations
- `versionx links pull <name>`:
  - `git subtree pull --prefix=<path> <url> <track>` (with `--squash` if configured).
  - Updates `versionx.lock` with the merged upstream SHA.
- `versionx links push <name>`:
  - Only allowed if `bidirectional = true`.
  - `git subtree push --prefix=<path> <url> <branch>`.
  - Typically pushes to a fork or a branch requiring PR on the upstream.

### 4.4 Conflict handling
Subtree conflicts are plain git conflicts — Versionx surfaces them clearly but defers to the user's git workflow.

---

## 5. Virtual monorepo mode

### 5.1 What it is
A logical grouping of N independent repos that Versionx treats as a single "workspace" for purposes of querying, policy, and coordinated release — without modifying the repos themselves.

### 5.2 Where `versionx-fleet.toml` lives

**Dedicated ops repo.** The fleet config lives in its own ops/platform repo (e.g., `acme/platform-ops`), not in any member. This repo typically also holds shared policies, GitHub Actions reusable workflows, and release-set definitions. Members reference policies via `inherit = ["fleet://acme-platform/baseline"]` — the `fleet://` scheme resolves against the fleet config's URL.

Rationale:
- Clean separation: no member repo has awkward primacy.
- Aligns with enterprise IaC / GitOps patterns.
- Easy to apply branch-protection, approval policies, and code-ownership rules to the fleet config itself.
- `versionx fleet init --remote <url>` clones the ops repo and wires everything up.

### 5.3 Config (`versionx-fleet.toml`)
```toml
[fleet]
name = "acme-platform"
schema_version = "1"

[members.portal-frontend]
url = "git@github.com:acme/portal-frontend.git"
clone_path = "frontend"            # where to clone locally
tags = ["frontend", "customer-facing"]

[members.portal-api]
url = "git@github.com:acme/portal-api.git"
clone_path = "api"
tags = ["backend", "customer-facing"]

[members.shared-libs]
url = "git@github.com:acme/shared.git"
clone_path = "shared"
tags = ["library"]

[sets.customer-portal]
members = ["portal-frontend", "portal-api", "shared-libs"]
release_mode = "coordinated"

[policies]
files = [".versionx/policies/fleet.policy.toml"]
```

### 5.4 Operations
- `versionx fleet init`: clones all members into the declared paths.
- `versionx fleet sync`: runs `versionx sync` in each member in parallel.
- `versionx fleet status`: dashboard-style table of all members (git status, outstanding plans, policy violations).
- `versionx fleet query "node < 20"`: cross-cutting query — find members violating a constraint.
- `versionx fleet release propose --set customer-portal`: coordinated release (see `05-release-orchestration.md §9`).

**Config Precedence (Local Wins):**
If a member repo's `versionx.toml` conflicts with the `versionx-fleet.toml` settings:
1. **Local Wins**: The member repo's `versionx.toml` is the source of truth for its own runtimes, adapters, and tasks.
2. **Fleet Policy Overlay**: `versionx-fleet.toml` can enforce *additional* policies, but it cannot override the member's pinned runtime version unless the member repo explicitly opts in via `inherit = ["fleet://..."]`.
3. **Audit Trail**: Conflicts are logged as `warn` events during `fleet sync`.

### 5.5 When to use vs. a real monorepo
Virtual monorepos are for cases where:
- You can't actually merge the repos (org boundaries, compliance, history too large).
- Independent release cadences matter, but you still want unified policy/visibility.
- Different teams own different repos but share standards.

A monorepo is still usually better when feasible. Virtual mode is the honest middle ground when it isn't.

---

## 6. Ref mode (lightweight pinning without git-level integration)

### 6.1 Config
```toml
[links.grpc-protos]
type = "ref"
url = "https://github.com/acme/protos.git"
track = "v2"
resolved_sha = "abc123..."          # managed by Vers
# No `path` — this link doesn't place code in the tree.
```

### 6.2 Use case
You want to *track* that your repo depends on another repo at a specific SHA (for bookkeeping, policy, and release coordination) but you don't vendor the code. Maybe you're consuming it as an npm package, a published binary, or a git dependency in Cargo.

`ref` links don't mutate your working tree. They show up in:
- `versionx links check-updates` (tells you when upstream has new versions).
- `versionx fleet query` (cross-repo queries still work).
- Policy evaluation (you can require that tracked refs stay within N days of `main`).

---

## 7. The unified "workspace" data structure

Internally, Versionx resolves any of the above into a single `Workspace`:

```rust
pub struct Workspace {
    pub root: Utf8PathBuf,
    pub root_config: VersionxConfig,
    pub packages: Vec<Package>,            // internal packages (monorepo)
    pub links: Vec<ResolvedLink>,          // submodule/subtree/virtual/ref
    pub runtimes: ResolvedRuntimes,
    pub policies: Vec<LoadedPolicy>,
}

pub struct Package {
    pub path: Utf8PathBuf,
    pub ecosystem: Ecosystem,
    pub manifest: Manifest,
    pub native_lockfile: Option<NativeLockfile>,
}

pub struct ResolvedLink {
    pub name: String,
    pub kind: LinkKind,
    pub url: String,
    pub state: LinkState,                  // SHAs, upstream status, etc.
}
```

Every Versionx command operates on a `Workspace`. This uniformity is what makes mixing modes possible.

---

## 8. Cross-repo release coordination

Already covered in `05-release-orchestration.md §9`. Summary of the multi-repo-specific pieces:

- **Release sets** are declared in `versionx-fleet.toml`.
- **Dependency edges** between repos (repo A depends on repo B's published package) are auto-detected by scanning manifests and cross-referencing fleet members' `[github]` or package names.
- **Ordering**: topological; leaves first.
- **Atomicity**: best-effort; dry-run gate; post-failure yank where possible.

---

## 9. Shared configs & policy across repos

Fleet members can inherit policies from a central location:
```toml
# in member repo's versionx.toml
[policies]
inherit = ["fleet://acme-platform/baseline"]
```

The `fleet://` scheme resolves to a policy file in the fleet config's repo, fetched and cached.

Similarly:
```toml
[policies]
inherit = ["org://acme/baseline"]             # org-level registry
inherit = ["https://..../policy.toml"]        # explicit URL
```

---

## 10. GitHub org-wide awareness

When installed as a GitHub App at the org level, Versionx can:
- Auto-discover repos with `versionx.toml` and register them into the org's fleet view.
- Post org-wide dashboards (via web UI or API) showing all repos' status.
- Enforce org-wide policies as required status checks.

Auto-discovery is opt-in and per-repo (repo must include a `versionx.toml` for the app to do anything).

---

## 11. Migration scenarios

### 11.1 "We have 30 repos, want to add Versionx gradually"
- Start with one repo. Add `versionx.toml`. Commit. Works.
- Add more repos one at a time.
- When ready for cross-repo features, create a `versionx-fleet.toml` in an ops repo. Members opt in by being listed.

### 11.2 "We're splitting a monorepo into multiple repos"
- Pre-split: monorepo has `versionx.toml` with internal packages.
- Split: each new repo gets its own `versionx.toml` extracted from the relevant section.
- Create `versionx-fleet.toml` in a separate ops repo listing them.
- Virtual monorepo mode restores cross-cutting visibility.

### 11.3 "We're consolidating multiple repos into one monorepo"
- Before: `versionx-fleet.toml` with members.
- Merge repos into one (using `git subtree add` or standard monorepo migration).
- Update the now-combined repo's `versionx.toml` to declare internal packages.
- Delete (or shrink) the fleet config.
- Versionx releases work at either granularity throughout.

---

## 12. Limits and trade-offs

### 12.1 Scale limits
- Internal monorepo: tested up to 200 packages. Beyond that, dep-graph resolution is still fast (<1s) but wall-clock operations depend on the underlying tools.
- Virtual monorepo: tested up to 100 members. Parallel ops cap at `--jobs`; state DB handles thousands without issue.

### 12.2 Cross-repo atomicity
We said it in `05-release-orchestration.md`, saying it again: git pushes are not transactional across remotes. "Coordinated" release is best-effort atomic. Policy: dry-run gate + rollback procedure on partial failure. No pretending otherwise.

### 12.3 Offline mode
- Submodule/subtree operations require network to fetch updates, not to verify installed state.
- Virtual fleet operations assume local clones; `versionx fleet sync` fetches from remotes but individual per-repo commands work offline.

---

## 13. Testing

### 13.1 Fixture repos
- Empty single repo.
- Single-ecosystem single repo.
- Multi-ecosystem monorepo (Node + Rust + Python).
- Monorepo with internal cross-package deps.
- Repo with submodule.
- Repo with subtree, bidirectional.
- Virtual fleet of 3 members with coordinated release.
- Mixed: monorepo with internal packages AND external submodule AND participating in a virtual fleet.

### 13.2 Workflow tests
- End-to-end cross-repo release (coordinated) with one member intentionally failing — verify clean rollback.
- Submodule update → PR → merge flow via GitHub App (mocked).
- Subtree bidirectional round-trip.

---

## 14. Non-goals

- **Not a git-replacement or a git-lite.** We use git; we don't reimplement it beyond minimal helpers.
- **Not reimplementing submodule/subtree.** We drive the git commands; if they have limitations, we inherit them.
- **Not a DAG build system.** Task execution and caching across packages is deferred to v2.
- **Not imposing a mono-vs-multi repo opinion.** Both are valid; we support both equally.
