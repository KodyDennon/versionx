---
title: Adding a package-manager adapter
description: Step-by-step for contributing a first-party adapter — trait implementation, tier guidelines, tests, registration.
sidebar_position: 4
---

# Adding a package-manager adapter

You'll learn:

- The `PackageManagerAdapter` trait contract.
- What tier a new adapter belongs in and what that implies.
- How adapters are tested and registered.

## The trait

Lives in `versionx-adapter-trait`. Abridged:

```rust
#[async_trait::async_trait]
pub trait PackageManagerAdapter: Send + Sync {
    fn id(&self) -> &'static str;
    async fn detect(&self, cwd: &Utf8Path) -> Result<Detection>;
    async fn status(&self, cwd: &Utf8Path) -> Result<StatusReport>;
    async fn plan_update(&self, cwd: &Utf8Path, opts: &UpdateOptions) -> Result<UpdatePlan>;
    async fn apply(&self, cwd: &Utf8Path, plan: &UpdatePlan) -> Result<UpdateOutcome>;
    async fn audit(&self, cwd: &Utf8Path) -> Result<AuditReport>;
}
```

Detailed trait doc on [docs.rs/versionx-adapter-trait](https://docs.rs/versionx-adapter-trait).

## Tier guidance

| Tier | Meaning | What ships |
|---|---|---|
| 1 | Full support | Adapter + matching runtime installer + tested on CI. |
| 2 | First-class adapter | Adapter only; runtime lands later. |
| 3 | Experimental / niche | Adapter only; user installs runtime themselves. |

Current Tier 1: Node, Python, Rust. Tier 2 planned in 1.1: Go, Ruby, JVM. Roadmap: [Roadmap page](/roadmap).

## Project layout

A new first-party adapter at `crates/versionx-adapter-<name>`:

```
crates/versionx-adapter-<name>/
├── Cargo.toml
├── src/
│   ├── lib.rs           # pub use Adapter
│   ├── adapter.rs       # impl PackageManagerAdapter
│   ├── detect.rs        # directory-probe logic
│   ├── manifest.rs      # manifest parsing
│   └── native.rs        # shell-out helpers
└── tests/
    ├── detect.rs
    ├── status.rs
    ├── plan.rs
    └── apply.rs
```

Add it to the workspace:

```toml
# Cargo.toml (root)
[workspace]
members = [
  # ...
  "crates/versionx-adapter-<name>",
]

[workspace.dependencies]
versionx-adapter-<name> = { path = "crates/versionx-adapter-<name>", version = "0.1.0" }
```

## Register it

Two places:

1. **Meta-crate re-export** in `crates/versionx-adapters/src/lib.rs`:

   ```rust
   pub use versionx_adapter_<name>::Adapter as <Name>Adapter;
   ```

2. **Core registry** (either statically or via feature flag):

   ```rust
   // crates/versionx-core/src/registry.rs
   registry.add(Arc::new(versionx_adapter_<name>::Adapter));
   ```

## Tests

### Detect

Use the harness:

```rust
use versionx_adapter_trait::testkit::AdapterTestHarness;

#[tokio::test]
async fn detects_when_manifest_present() {
    let h = AdapterTestHarness::new().with_file("<manifest>", "…").build();
    let a = Adapter;
    assert!(a.detect(h.path()).await.unwrap().is_yes());
}
```

### Status / plan / apply

Fixtures under `tests/fixtures/<scenario>/` with a manifest + expected output. Assert via snapshots (`insta`).

### Round-trip

Every adapter gets the shared "plan serializes and applies" round-trip test. Wire it up:

```rust
versionx_adapter_trait::testkit::roundtrip!(Adapter);
```

## Shell-out patterns

- Use `duct` or `tokio::process::Command` via `versionx-core`'s `proc` helper — it scrubs env properly.
- Never set `NODE_OPTIONS`, `PYTHONPATH`, `RUSTC_*` leakage from the host process. The helper does this.
- Always pass `--frozen-lockfile` (or equivalent) in `apply` so the native tool doesn't re-resolve unexpectedly.

## Documentation

- Doc comments on every `pub` item. `clippy::pedantic` enforces.
- Add a "tier" attribute in `Cargo.toml` metadata so the CI matrix knows how to test:

  ```toml
  [package.metadata.versionx]
  tier = 1
  ecosystem = "<name>"
  ```

## Conventional Commit

Your commit message:

```text
feat(adapter-<name>): add <name> adapter (Tier <N>)
```

## See also

- [Workspace tour](./workspace-tour)
- [Writing tests](./writing-tests)
- [`docs/spec/03-ecosystem-adapters.md`](https://github.com/KodyDennon/versionx/blob/main/docs/spec/03-ecosystem-adapters.md) — adapter design spec.
