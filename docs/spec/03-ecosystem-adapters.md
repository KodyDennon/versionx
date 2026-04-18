# 03 — Ecosystem Adapters

## Scope
How Versionx interfaces with package managers. Defines the `PackageManagerAdapter` trait, the shell-out execution strategy, per-ecosystem contracts, and the tier-1/2/3 support policy.

## Contract
After reading this file you should be able to implement a new ecosystem adapter (e.g., for Elixir/Hex or Dart/pub) by following the trait and testing pattern — without needing to touch `versionx-core`.

---

## 1. Philosophy

Versionx does **not** reimplement package resolvers. We drive the real tools. This means:

- ✅ We get correctness for free — `npm`, `pip`, `cargo` are battle-tested.
- ✅ We inherit ecosystem idioms automatically.
- ✅ New tool versions work without a Versionx release (mostly).
- ❌ We pay a subprocess cost per operation.
- ❌ We have to parse tool output, which can break across versions.

To manage the downside, every adapter commits to:
1. Using **structured output formats** when the tool offers them (`--json`, `--porcelain`, etc.).
2. **Pinning tested tool version ranges** in the lockfile; unsupported versions warn loudly.
3. **Golden output fixtures** captured per supported tool version, verified in CI.

---

## 2. The `PackageManagerAdapter` trait

Lives in `versionx-adapter-trait`. Every adapter implements it. Async, fallible, fully typed.

```rust
#[async_trait]
pub trait PackageManagerAdapter: Send + Sync {
    /// Stable identifier: "npm", "pnpm", "yarn", "pip", "uv", "poetry", ...
    fn id(&self) -> &'static str;

    /// Ecosystem this adapter belongs to: Ecosystem::Node, Ecosystem::Python, ...
    fn ecosystem(&self) -> Ecosystem;

    /// Detect whether this adapter should handle the given directory.
    /// Adapters are ordered by priority; first match wins.
    async fn detect(&self, ctx: &DetectContext) -> DetectResult;

    /// Check that the underlying tool is installed and satisfies version constraints.
    async fn check_tool(&self, ctx: &AdapterContext) -> Result<ToolCheck, AdapterError>;

    /// Parse the project manifest(s) into a normalized view.
    async fn read_manifest(&self, ctx: &AdapterContext) -> Result<Manifest, AdapterError>;

    /// Read the ecosystem-native lockfile (if any), return its hash + dep list.
    async fn read_native_lockfile(&self, ctx: &AdapterContext) -> Result<Option<NativeLockfile>, AdapterError>;

    /// Produce an execution plan for the given intent. NEVER executes.
    async fn plan(&self, ctx: &AdapterContext, intent: &Intent) -> Result<Plan, AdapterError>;

    /// Execute a plan step. Streams events on the event bus.
    async fn execute(&self, ctx: &AdapterContext, step: &PlanStep) -> Result<StepOutcome, AdapterError>;

    /// List installed packages with versions (for audit, policy, queries).
    async fn list_installed(&self, ctx: &AdapterContext) -> Result<Vec<InstalledPackage>, AdapterError>;

    /// Propose upgrades for outdated packages.
    async fn outdated(&self, ctx: &AdapterContext) -> Result<Vec<OutdatedPackage>, AdapterError>;

    /// Run a security audit using the tool's native audit or an external source.
    async fn audit(&self, ctx: &AdapterContext) -> Result<Vec<Advisory>, AdapterError>;

    /// Publish a package (release time). Returns the published coordinates.
    async fn publish(&self, ctx: &AdapterContext, req: &PublishRequest) -> Result<PublishOutcome, AdapterError>;

    /// Reset/clean cached state (e.g., `node_modules`, `__pycache__`).
    async fn clean(&self, ctx: &AdapterContext) -> Result<(), AdapterError>;

    /// Optional: adapter-specific commands that don't fit the above.
    async fn run_extension(&self, ctx: &AdapterContext, ext: &Extension) -> Result<serde_json::Value, AdapterError> {
        Err(AdapterError::NotImplemented)
    }
}
```

### Key supporting types (abbreviated)

```rust
pub struct AdapterContext {
    pub cwd: Utf8PathBuf,
    pub config: EcosystemConfig,
    pub env: HashMap<String, String>,
    pub events: EventSender,
    pub cache: CacheHandle,
    pub runtime: RuntimeHandle,       // gives access to the pinned runtime's binary paths
    pub dry_run: bool,
}

pub enum Intent {
    Sync,                              // install to match manifest+lockfile
    Install { spec: PackageSpec, dev: bool },
    Remove { name: String },
    Upgrade { spec: Option<PackageSpec>, kind: UpgradeKind },
    LockOnly,                          // regenerate lockfile without touching installed files
}

pub struct Plan {
    pub steps: Vec<PlanStep>,
    pub summary: PlanSummary,          // adds/removes/upgrades counts, etc.
    pub warnings: Vec<Warning>,
}

pub struct PlanStep {
    pub id: String,                    // stable hash for idempotency
    pub action: StepAction,            // Install / Remove / Upgrade / Link / Rebuild
    pub command_preview: String,       // human-readable "npm install foo@1.2.3 --save-dev"
    pub affects_lockfile: bool,
    pub reversible: bool,
}
```

Implementers focus on **correct output parsing** and **deterministic planning**. All event emission, error normalization, and subprocess management is provided by a shared helper crate.

---

## 3. Shell-out execution — the plumbing

All subprocess execution goes through a single `spawn` helper:

```rust
pub async fn spawn(
    program: &str,
    args: &[&str],
    ctx: &AdapterContext,
    opts: SpawnOptions,
) -> Result<Output, AdapterError>;
```

The helper handles:
- Resolving `program` via the pinned runtime (not system PATH).
- Scrubbing environment: clears `NODE_OPTIONS`, `PYTHONPATH`, etc. unless whitelisted.
- Streaming stdout/stderr to the event bus as they arrive.
- Timeouts, cancellation on ctrl-c.
- Per-ecosystem concurrency locks (no two `npm install` in same dir).
- Windows quoting nightmares (uses `windows_args` or equivalent).
- Exit code + stderr → structured `AdapterError`.

No adapter ever calls `Command::new` directly. PR reviews reject it.

---

## 4. Support tiers

| Tier | Guarantee | Ecosystems | First shipped |
|---|---|---|---|
| **Tier 1** | Native adapter, full trait implementation, fixture-tested, golden-path CI | Node (npm/pnpm/yarn), Python (pip/uv/poetry), Rust (cargo) | **v1.0 — all three in parallel** |
| **Tier 2** | Native adapter, full trait, CI on latest tool version only | Go, Ruby, OCI | v1.1 |
| **Tier 2** | Native adapter, full trait, CI on latest tool version only | JVM (maven/gradle) | v1.2 |
| **Tier 3** | Community adapter, best-effort, may shell out to less-structured tools | .NET/NuGet, PHP/Composer, Dart/pub, Elixir/Hex, Swift/SPM, Haskell/Cabal | post-v2 |

v1.0 ships Node + Python + Rust as Tier 1 **in parallel**. This is a bigger Phase 1 than typical (triples test fixtures and CI matrix) but it's the minimum needed to demonstrate the polyglot-release wedge. See `10-mvp-and-roadmap.md`.

### Tier promotion criteria
A Tier 2 adapter graduates to Tier 1 when:
- CI covers the latest 3 stable tool versions.
- Golden fixtures exist for install/update/lockfile/publish.
- 30 days in production with no P1 bugs from the adapter.
- A dedicated maintainer.

---

## 5. Per-ecosystem adapter contracts

Each subsection below is the implementation brief for one adapter.

### 5.1 Node (`versionx-adapter-node`)

Supports three package managers: **npm**, **pnpm**, **yarn** (classic and berry).

**Manifest**: `package.json`. Parse with `serde_json`. Extract: name, version, deps/devDeps/peerDeps/optionalDeps, `packageManager` field, `workspaces`, `engines`.

**Native lockfile**: `package-lock.json` | `pnpm-lock.yaml` | `yarn.lock`. Hash the whole file with blake3; don't try to normalize the content.

**Tool selection precedence**:
1. `packageManager` field (e.g., `"pnpm@8.15.0"`) → wins.
2. Lockfile presence (unambiguous case).
3. `versionx.toml` explicit override.
4. Multiple lockfiles present → error, require disambiguation.

**Package manager installation** (not corepack):
Node 25+ **stops shipping corepack by default**, and early 2025 had multiple corepack signature-verification breakages in CI. Versionx installs pnpm/yarn/npm-version-overrides **directly** as managed runtimes (see `04-runtime-toolchain-mgmt.md §4.7`):
- Read `packageManager` field → install exact pinned version into `$XDG_DATA_HOME/versionx/runtimes/pnpm/<version>/` or equivalent.
- Shim into `$XDG_DATA_HOME/versionx/shims/` so `pnpm` resolves to the right version per-repo.
- Never rely on corepack being present. If a user has corepack enabled, we coexist (our shim shadows theirs when `versionx activate` is sourced).
- `pnpm` v10+ manages its own version via `manage-package-manager-versions` — respected when present.

**Key commands**:
| Intent | npm | pnpm | yarn (classic) | yarn (berry) |
|---|---|---|---|---|
| Sync | `npm ci` | `pnpm install --frozen-lockfile` | `yarn install --frozen-lockfile` | `yarn install --immutable` |
| Install | `npm install <spec>` | `pnpm add <spec>` | `yarn add <spec>` | `yarn add <spec>` |
| Upgrade | `npm update` | `pnpm update` | `yarn upgrade` | `yarn up` |
| Publish | `npm publish` | `pnpm publish` | `yarn publish` | `yarn npm publish` |
| Audit | `npm audit --json` | `pnpm audit --json` | `yarn audit --json` | `yarn npm audit --json` |

**Workspaces**: if config declares `workspaces` (or `pnpm-workspace.yaml` is present for pnpm v10+), adapter enumerates member `package.json` files and reports a workspace-aware manifest.

**Publishing**: supports `.npmrc` with `_authToken`; accepts `NODE_AUTH_TOKEN` env var. **Prefers OIDC trusted publishing** (GA on npm since July 2025 with npm CLI ≥ 11.5.1, Node ≥ 22.14) — auto-attaches provenance. Falls back to token with a loud warning. See `05-release-orchestration.md §10`.

### 5.2 Python (`versionx-adapter-python`)

Supports **pip**, **uv**, **poetry**. (Recommend uv as default; it's fast and lockfile-first, and Astral is riding a long momentum curve.)

**Manifest**: `pyproject.toml` preferred. `requirements.txt` supported for pip-only projects.

**Native lockfile**:
- uv: `uv.lock`
- poetry: `poetry.lock`
- pip: no native lockfile; `pip-compile`-generated `requirements.lock` supported as a convention.

**Virtualenvs — delegate, don't own.** uv and poetry already manage venvs well with their own opinions about location (`.venv` at project root by default). Versionx **delegates venv creation to the active PM** (`uv sync` / `poetry install` handle this). Only for pip-only projects without a venv manager does Versionx create one at `$XDG_CACHE_HOME/versionx/venvs/<repo-hash>/`. Per-repo override via `[ecosystems.python] venv_manager = "uv|poetry|versionx"`.

**Workspaces**: uv workspaces (stable as of 2025) with `[tool.uv.workspace] members = [...]` in the root `pyproject.toml` are read natively. Poetry's monorepo story is less mature; we support it where it exists.

**Key commands**:
| Intent | uv | poetry | pip |
|---|---|---|---|
| Sync | `uv sync --frozen` | `poetry install --sync` | `pip install -r requirements.lock` |
| Install | `uv add <spec>` | `poetry add <spec>` | `pip install <spec>` + regen lockfile |
| Publish | `uv publish` (supports OIDC trusted publisher) | `poetry publish --build` | `twine upload dist/*` |

**Python interpreter selection**: delegated to the Python runtime installer (see `04-runtime-toolchain-mgmt.md`). Adapter receives the resolved interpreter path in `ctx.runtime`.

**Windows gotcha:** python-build-standalone does **not** ship `pip.exe`/`pip3.exe` on Windows. Always invoke via `python -m pip`. Versionx generates `pip.exe` / `pip3.exe` shims during runtime install to paper over this.

### 5.3 Rust (`versionx-adapter-rust`)

Single package manager: **cargo**.

**Manifest**: `Cargo.toml`. Workspaces via `[workspace]` member globs. Parse with `toml_edit` to preserve formatting on round-trip.

**Native lockfile**: `Cargo.lock`.

**Key commands**:
| Intent | Cargo |
|---|---|
| Sync | `cargo fetch` (+ `cargo build --offline` on demand) |
| Install | `cargo add <spec>` |
| Upgrade | `cargo update` or `cargo upgrade` (cargo-edit) |
| Publish | `cargo publish` (supports `--registry` for alternate registries) |
| Audit | `cargo audit` (optional tool, detected) |

**Workspaces**: for monorepo releases, Versionx reads the `[workspace]` section and produces per-crate release plans.

**Publishing**: supports `cargo publish` with `CARGO_REGISTRY_TOKEN`. **Prefers OIDC trusted publishing** (GA on crates.io since July 2025 via `rust-lang/crates-io-auth-action` — 30-min scoped tokens). For workspace publishes with many crates, serializes with sleep to respect crates.io's rate limits (5 new crates burst then 1/10min; 30 new versions burst then 1/min).

### 5.4 Go (`versionx-adapter-go`) — v1.1

**Manifest**: `go.mod`. Parse with `golang.org/x/mod` equivalent Rust crate (or shell out to `go mod edit -json`).

**Native lockfile**: `go.sum`.

**Key commands**:
| Intent | Go |
|---|---|
| Sync | `go mod download` + `go mod verify` |
| Install | `go get <spec>` + `go mod tidy` |
| Upgrade | `go get -u <spec>` |
| Publish | Tag-based (no `publish` command); Versionx drives the tag creation |
| Audit | `govulncheck ./...` (if installed) |

**Workspaces**: `go.work` files supported — enumerate `use` directives.

### 5.5 Ruby (`versionx-adapter-ruby`) — v1.1

Single package manager: **bundler**.

**Manifest**: `Gemfile` (+ `<package>.gemspec` for publishable gems).

**Native lockfile**: `Gemfile.lock`.

**Key commands**:
| Intent | Bundler |
|---|---|
| Sync | `bundle install --frozen` |
| Install | `bundle add <spec>` |
| Upgrade | `bundle update <spec>` |
| Publish | `gem build *.gemspec && gem push *.gem` |
| Audit | `bundler-audit check` (if installed) |

**Publishing**: supports OIDC trusted publishing to RubyGems (GA Dec 2023).

### 5.6 JVM (`versionx-adapter-jvm`) — v1.2

Supports **Maven** and **Gradle** (both Groovy DSL and Kotlin DSL).

**Maven**:
- Manifest: `pom.xml` (parse with `quick-xml`).
- No native lockfile by default; supports `dependency-lock-maven-plugin` if present.
- Commands: `mvn dependency:resolve`, `mvn install`, `mvn deploy`.

**Gradle**:
- Manifest: `build.gradle` or `build.gradle.kts` (treated as opaque; we extract dependencies via `gradle dependencies --configuration runtimeClasspath` with JSON output where possible).
- Native lockfile: `gradle.lockfile` (Gradle's dependency locking feature).
- Commands: `gradle dependencies`, `gradle publish`.

**Caveat**: JVM builds are slow. Adapter exposes a `--skip-resolve` opt-out for interactive loops.

**Publishing**: Maven Central requires user tokens via Sonatype Central Portal (OSSRH sunset June 30, 2025). OIDC trusted publishing is **not yet supported** at Maven Central for publishes — document this limitation loudly when publishing with Versionx's OIDC-preferred defaults.

### 5.7 OCI (`versionx-adapter-oci`) — v1.1

Different beast — this adapter manages **container image dependencies**, not a package manager in the traditional sense. Included because you asked.

**Manifest**: `Dockerfile` (parse `FROM` lines) + optional `versionx.toml [oci]` block listing additional pinned images.

**Native lockfile**: not a thing natively. Versionx synthesizes `versionx.oci.lock` with resolved digests for every `FROM` reference.

**Key commands**:
| Intent | Tool |
|---|---|
| Sync | Resolve image tags → digests via `skopeo inspect` or `crane digest` |
| Install | n/a (no install; just resolve) |
| Upgrade | Re-resolve tags, compute new digests |
| Publish | `docker push` / `crane push` |
| Audit | `trivy image` (if installed) |

**Pin format**: `image@sha256:...` in the lockfile; `image:tag` in the Dockerfile. 

**Rewriting Logic**: 
Versionx rewrites the `FROM` lines in the Dockerfile on `sync` to include the immutable digest, while preserving the human-readable tag in a comment for auditability and future upgrades.
```dockerfile
# Before
FROM node:20-alpine

# After
FROM node@sha256:d52187... # vers: node:20-alpine
```
(Opt-out via `[oci] rewrite_dockerfile = false` in `versionx.toml`.)

---

## 6. Testing adapters

Every adapter ships with:

### 6.1 Fixture tests
`tests/fixtures/<scenario>/` directories containing a representative project. Test runs the adapter, captures its stdout/parse result, compares against a `.expected.json`. Update with `cargo test -- --update-fixtures`.

### 6.2 Golden tool-version matrix
CI runs each Tier-1 adapter against multiple tool versions:
- Node: npm latest + npm 8; pnpm 8 + 9; yarn 1 + 4.
- Python: uv 0.1+, poetry 1.7+, pip 23+.
- Rust: cargo stable + beta.

A matrix failure is a release blocker.

### 6.3 Offline mode
Adapters must support `--offline` correctly. Test suite includes offline runs with a pre-populated cache.

### 6.4 Property tests
For planning logic: any plan applied to a state, then re-planned, must produce an empty plan (idempotence).

---

## 7. Extensions — the escape hatch

Not every ecosystem op fits the core trait. The `run_extension` method is a JSON-in/JSON-out escape hatch:

```rust
let result = adapter.run_extension(&ctx, &Extension {
    name: "npm.dedupe",
    params: json!({}),
}).await?;
```

Registered extensions are listed in each adapter's doc. Used sparingly. Policy: if an extension is used in the same way in 3+ places, promote it to the trait.

---

## 8. Adapter authoring checklist

For anyone (human or AI agent) writing a new adapter:

- [ ] Create `crates/versionx-adapter-<n>/` with `Cargo.toml` depending on `versionx-adapter-trait`.
- [ ] Implement the trait. Start with `detect`, `check_tool`, `read_manifest`, `plan`, `execute`.
- [ ] Add golden fixtures under `tests/fixtures/`.
- [ ] Add to `versionx-adapters` meta-crate re-exports.
- [ ] Register in `versionx-core::adapters::registry()`.
- [ ] Add CI matrix entry in `.github/workflows/adapters.yml`.
- [ ] Document in this spec file (Tier 3 promotion PR).

---

## 9. Non-goals

- **Not a package registry.** We do not host packages.
- **Not reimplementing resolvers.** If `npm install` has a bug, we have the same bug.
- **Not a bundler/transpiler orchestrator.** That's a task runner concern (deferred).
- **Not a dependency graph visualizer** beyond what's needed for monorepo releases. (UI can be added later on top of the data.)
