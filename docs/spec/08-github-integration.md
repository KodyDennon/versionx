# 08 — GitHub Integration

## Scope
Two integration surfaces: (1) a **portable CI contract** that runs identically in any CI system, and (2) a set of **official reusable GitHub Actions** that give GitHub users a turnkey experience via their existing workflows.

**A hosted GitHub App is deferred past v1.0.** The CLI + reusable Actions cover every v1.0 use case; a hosted webhook-receiving service adds SaaS infrastructure burden and is revisited when user demand justifies it.

**Philosophy**: portable first, GitHub-deep second. The CLI never requires GitHub. Using the official Actions unlocks polished PR check-runs and cross-repo coordination within workflows, not a hosted service.

## Contract
After reading this file you should be able to: run Versionx in GitHub Actions / GitLab CI / CircleCI / Jenkins with the same config, use the official Actions for PR-time checks and release automation, and understand what the deferred GitHub App would add if/when it ships.

---

## 1. Portable CI core

### 1.1 Single binary, same flags everywhere
The `versionx` binary is a static single executable with no runtime dependencies (beyond git and the ecosystem tools it drives). It runs identically in:
- GitHub Actions
- GitLab CI/CD
- CircleCI
- Jenkins (any agent)
- Buildkite
- Bitbucket Pipelines
- Local dev

Every command Versionx exposes is invokable from any of these.

### 1.2 Install in CI
Three options, documented for each platform:

**Option A — GitHub Action / CircleCI Orb / native packages** (preferred where available):
```yaml
- uses: acme/versionx-install-action@v1
  with:
    version: "1.0.0"
```

**Option B — `curl | sh`** (universal):
```bash
curl -fsSL https://versionx.dev/install.sh | sh
```

**Option C — Package managers**:
- Homebrew: `brew install versionx`
- Scoop / winget: `scoop install versionx`
- Cargo: `cargo install versionx`
- npm / PyPI shim packages: `npm i -g versionx` / `pip install versionx` (cargo-dist-generated shims that download the binary)
- apt/rpm: from GitHub Releases or a hosted apt repo (post-v1).

### 1.3 CI environment detection
Versionx detects CI via standard env vars (`CI=true`, `GITHUB_ACTIONS`, `GITLAB_CI`, `CIRCLECI`, `JENKINS_URL`, `BUILDKITE`) and:
- Defaults to non-interactive mode.
- Disables daemon.
- Requires explicit `--push` or `--no-push` on release commands (no prompting in CI).
- Emits structured logs (GitHub Actions annotations, GitLab section markers, etc.).
- Reads credentials from standard env vars per platform.
- **Prefers OIDC** trusted publishing where supported; falls back to token with a loud warning.

### 1.4 Standard CI workflow
```
versionx verify            # assert config + lockfile integrity
versionx sync              # install everything
versionx policy check      # run policies; fail on deny
<your tests>
versionx release propose   # PR-time: compute release plan preview, post check-run
versionx release apply     # release-time: execute plan
```

Each step is portable.

---

## 2. GitHub Actions — first-class support

### 2.1 Official actions
Shipped as part of the project, in `acme/versionx-actions` repo:

- `acme/versionx-install-action@v1` — install the binary with caching (honors SHA-pinning + sigstore verification).
- `acme/versionx-sync-action@v1` — `versionx sync` with caching of `versionx.lock` hash.
- `acme/versionx-policy-action@v1` — run policies, post annotations (respecting the 50-per-request / 65535-char-summary limits).
- `acme/versionx-release-propose-action@v1` — propose release on PR, post rich PR comment.
- `acme/versionx-release-apply-action@v1` — apply release on merge; supports OIDC trusted publishing.
- `acme/versionx-pr-title-action@v1` — validate PR title matches conventional-commit pattern (primary for default PR-title release strategy).

### 2.2 Example workflow
```yaml
name: versionx
on: [pull_request, push]

jobs:
  validate:
    if: github.event_name == 'pull_request'
    runs-on: ubuntu-latest
    steps:
      - uses: acme/versionx-pr-title-action@v1

  sync-and-check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          submodules: recursive
      - uses: acme/versionx-install-action@v1
      - run: versionx verify
      - run: versionx sync
      - run: versionx policy check
      - uses: acme/versionx-release-propose-action@v1
        if: github.event_name == 'pull_request'
        with:
          comment-on-pr: true
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}

  release:
    if: github.ref == 'refs/heads/main'
    needs: sync-and-check
    runs-on: ubuntu-latest
    permissions:
      contents: write
      id-token: write   # for OIDC-based publishing
    steps:
      - uses: actions/checkout@v4
      - uses: acme/versionx-install-action@v1
      - uses: acme/versionx-release-apply-action@v1
        with:
          push: true
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          # No NPM_TOKEN needed if trusted publishing is configured on npm.
```

### 2.3 Reusable workflows
Published at `acme/versionx-workflows`:
- `release.yml` — release on merge to main.
- `scheduled-sync.yml` — nightly dependency sync PR.
- `submodule-update.yml` — weekly submodule updates.
- `cross-repo-release.yml` — coordinated release across a set (uses workflow_dispatch chain).

Org-wide reuse:
```yaml
jobs:
  release:
    uses: acme/versionx-workflows/.github/workflows/release.yml@v1
    with:
      strategy: pr-title
    secrets: inherit
```

---

## 3. PR check runs via Actions (no hosted App)

The Actions post one or more **check runs per PR** using the workflow's `GITHUB_TOKEN`:

### 3.1 `versionx/policy`
- **Summary**: policy eval result.
- **Details**: per-policy verdict table with remediation links.
- **Annotations**: inline code annotations (chunked to respect 50-per-request limit).
- **SARIF output**: `versionx-policy.sarif` for GitHub Advanced Security ingestion.

### 3.2 `versionx/release-preview`
- **Summary**: "If merged, this PR will release: X.Y.Z of @acme/ui, A.B.C of @acme/utils."
- **Details**: full release plan, changelog preview, affected packages.
- **Comment**: single PR comment with hidden marker `<!-- versionx:summary:v1 -->`, updated in place on each push (not spammed). Reliable find-by-marker prevents dupes.

### 3.3 `versionx/lock-integrity`
- **Summary**: whether `versionx.lock` is in sync.
- **Fails** if a manifest changed without a lockfile update.

### 3.4 `versionx/links`
- **Summary**: submodule/subtree/ref status.
- **Warns** when links are stale beyond configured threshold.

Check names are configurable via `[github] check_prefix = "versionx"`.

**Idempotency**: check runs use a stable `external_id` per (PR, check type) to dedupe re-runs.

---

## 4. Auto-merge (via GitHub's native API)

Opt-in per repo:
```toml
[github]
auto_merge = true
auto_merge_conditions = [
  "checks_green",
  "no_policy_denies",
  "label:versionx:auto-merge",
]
auto_merge_strategy = "squash"
```

Versionx uses GitHub's **native auto-merge API** (`enablePullRequestAutoMerge` GraphQL mutation) rather than reimplementing merge queueing. Kodiak/Mergify/Bulldozer lessons apply:
- Signed-commits + rebase is incompatible in GitHub's API; default to `squash`.
- Branch protection rules are not bypassed — Versionx respects them.
- Rulesets (newer than branch protection) can list Versionx bot as a bypass actor for specific protections.

Limitations documented when constraints conflict (e.g., "required signed commits + rebase strategy" → error, suggest squash).

---

## 5. Cross-repo coordination in CI

For the wedge — cross-repo atomic release — without a hosted app, Versionx uses the `workflow_dispatch` chain pattern:

1. Platform ops repo has a release-orchestrator workflow triggered via `workflow_dispatch` or release PR.
2. It calls `versionx fleet release propose --set customer-portal`.
3. For each member repo, it dispatches the member's release workflow with the release plan.
4. Each member workflow receives the plan, runs `versionx release apply`, reports status back via repository_dispatch.
5. Orchestrator tracks success/failure and handles saga rollback per `05-release-orchestration.md §10.3`.

Rough, yes; works today, yes. A hosted App would make this cleaner — see §9 for the deferred story.

---

## 6. Non-GitHub platforms

### 6.1 GitLab — v1.0 is CLI-only
The `versionx` binary runs in GitLab CI with identical flags. No MR comments, no GitLab CI components, no GitLab App integration in v1.0.

Users can write their own `.gitlab-ci.yml`:
```yaml
versionx-sync:
  script:
    - curl -fsSL https://versionx.dev/install.sh | sh
    - versionx verify && versionx sync && versionx policy check

versionx-release:
  rules:
    - if: $CI_COMMIT_BRANCH == "main"
  script:
    - versionx release apply --push
```

Deferred to v1.2: GitLab CI components and MR-comment automation via GitLab API.

### 6.2 Bitbucket, Azure DevOps
CLI-level support (they run the binary). No native integration planned.

### 6.3 Self-hosted git (Gitea, Forgejo)
CLI works. Community integration possible.

---

## 7. Webhooks — generic (no hosted receiver)

For users not on GitHub, Versionx exposes an **outbound webhook** mechanism driven by the CLI:
```toml
[webhooks]
on_release = "https://acme.example/hooks/versionx-release"
on_policy_violation = "https://acme.example/hooks/versionx-violations"
secret_env = "VERSIONX_WEBHOOK_SECRET"
```

Payload is JSON, HMAC-signed. Delivery is at-least-once with exponential backoff. Fired from within the CI workflow that runs `versionx`; there's no hosted Versionx service receiving inbound webhooks.

---

## 8. Authentication model

### 8.1 Local dev
- Git credentials via SSH / credential helper (user's existing setup; Versionx never prompts).
- npm/PyPI/cargo tokens via standard env vars or credential files.
- GitHub PAT if user wants `versionx gh` CLI features (stored in OS keychain).

### 8.2 CI
- **OIDC preferred** where available (GitHub OIDC → trusted publisher on npm/PyPI/cargo/RubyGems).
- Env-var secrets for registries lacking OIDC (primarily Maven Central).
- `GITHUB_TOKEN` / equivalent for git operations and check-run posting.

### 8.3 Secret hygiene
- Versionx logs redact: tokens (known prefixes), URLs with credentials.
- `--debug` mode still redacts; debugging auth needs explicit `--debug-unsafe-log-secrets` flag.
- No secret ever written to `versionx.lock` or state DB.

---

## 9. The deferred hosted GitHub App (post-v1.0)

Versionx's roadmap tracks a hosted GitHub App as a post-v1.0 feature. This section documents what it would add, so users understand what they're **not** getting in v1.0 (and why).

### 9.1 What a hosted App would add
- **Webhook-driven actions** without per-repo workflow setup. A single org-level install reacts to every PR event across all repos.
- **Sub-5s check run posts** when a PR opens (no workflow cold-start).
- **Branded bot identity** (`versionx[bot]` commenter, avatar, labels).
- **Cross-repo coordination without workflow_dispatch chains** — app sees every repo at once.
- **Scheduled cron jobs per-installation** (weekly dep-sync PRs, submodule update PRs, monthly audit reports).
- **Fleet dashboards** at app install page — live org-wide view of compliance.

### 9.2 Why deferred
- Adds SaaS infrastructure burden (hosting, uptime, data residency, auth).
- Every v1.0 use case is covered by CLI + reusable Actions.
- Self-hosting a GitHub App requires ops muscle (Fly.io or Kubernetes deploy, Postgres, webhook signing).
- Open-source maintainers burn out under SaaS scope creep; we want to ship CLI quality first.

### 9.3 Re-evaluation triggers
The App ships when:
- User demand materializes (>N requests per month).
- CLI + Actions workflow proves insufficient for cross-repo scenarios.
- There's a clear maintainer committed to running infrastructure.

Not before. This is a deliberate choice.

### 9.4 Architecture if/when it ships
- Rust binary reusing `versionx-core` (the architecture is ready — see `01-architecture-overview.md`).
- Axum + rmcp for webhook + optional MCP endpoints.
- Octocrab (≥0.49.7) with application-level token-bucket rate limiter (targets 60/min, 400/hr content-creating to leave headroom against GitHub's 80/min, 500/hr limits).
- Per-installation queues to avoid one noisy repo starving others.
- Sticky sessions if horizontally scaled (Streamable HTTP / MCP sessions need per-node affinity).
- Deployable to Fly.io (Firecracker microVMs for stateful work) + Cloudflare Workers for webhook ingest if high scale.
- Licensed same as CLI (Apache 2.0). Self-hostable forever; managed offering a separate decision.

---

## 10. Observability

- Structured logs (OTLP export if user configures an endpoint).
- Metrics exposed at `/metrics` (Prometheus-format) on the daemon's HTTP surface.
- Traces for policy eval, adapter invocation, release publish.
- Dashboards ship as Grafana JSON in the repo.

---

## 11. Testing

- **Unit**: PR-title parser, check-run formatting, policy-to-annotation mapping.
- **Integration**: mock GitHub API via `octocrab`'s test server; full PR-lifecycle replay with Actions.
- **End-to-end**: a dedicated test org with test repos; nightly runs a battery of scenarios (open PR, push updates, merge, release) via workflows.

---

## 12. Non-goals

- **Not a hosted GitHub App in v1.0.** Deferred; maybe later.
- **Not a full dev platform.** No wikis, no project boards, no runner hosting.
- **Not replacing Dependabot/Renovate.** We complement them; users can keep using either or swap to Versionx's sync.
- **Not enforcing branch protection** — that's GitHub's job. We surface violations; we don't override settings.
- **Not a multi-tenant SaaS** at launch. CLI + Actions only.
