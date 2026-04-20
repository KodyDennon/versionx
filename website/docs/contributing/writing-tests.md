---
title: Writing tests
description: Unit, property, snapshot, and integration tests. When to use which, and the project conventions around each.
sidebar_position: 6
---

# Writing tests

Versionx has four tiers of test. Use the right one for the job; don't mix them.

## Tier 1 — Unit

Fast, isolated, per-function. Live alongside the source in `#[cfg(test)] mod tests`.

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_semver() {
        assert_eq!(Version::parse("1.2.3").unwrap().major, 1);
    }
}
```

Every function added to a `pub` surface needs at least one happy-path unit test.

## Tier 2 — Property (proptest)

For anything that has state transitions, roundtrips, or invariants. Lives in `tests/` subdirs.

```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn plan_json_roundtrip(plan in any::<Plan>()) {
        let json = serde_json::to_string(&plan).unwrap();
        let back: Plan = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(plan, back);
    }
}
```

Use property tests for:

- Serialization round-trips.
- Parser/lexer invariants.
- Lockfile hash stability.
- Adapter plan idempotence.

## Tier 3 — Snapshot (insta)

For anything with a stable human-readable output (CLI stdout, rendered markdown, generated docs). Lives in `tests/` with `__snapshots__/` companions.

```rust
use insta::assert_snapshot;

#[test]
fn status_output_renders_expected() {
    let report = example_status_report();
    assert_snapshot!(render_human(&report));
}
```

Review snapshot changes with:

```bash
cargo insta review
```

Commit the approved snapshots alongside your code change.

## Tier 4 — Integration

For end-to-end behavior that crosses multiple crates. Live in `tests/` at the binary-crate level. Use `assert_cmd`.

```rust
use assert_cmd::Command;

#[test]
fn versionx_status_exits_zero_in_empty_dir() {
    let tmp = tempfile::tempdir().unwrap();
    Command::cargo_bin("versionx")
        .unwrap()
        .arg("status")
        .current_dir(&tmp)
        .assert()
        .success();
}
```

Use integration tests for:

- CLI argument parsing end-to-end.
- MCP tool invocation shapes.
- Daemon lifecycle (start, RPC, shutdown).

## When to use which

| If you're testing… | Use |
|---|---|
| A pure function | Unit |
| A round-trip or invariant | Property |
| Human-readable output | Snapshot |
| A whole verb end-to-end | Integration |
| An adapter | `versionx-adapter-trait::testkit` helpers + whichever above fits |

## Running

```bash
cargo test --workspace                       # all tests
cargo nextest run --workspace                # faster, with shard awareness
cargo test --package versionx-core           # single crate
cargo test --package versionx-release plan   # one test
```

## CI gates

- All four tiers run on every PR.
- `insta` snapshots must be committed; CI fails on unchecked changes.
- Property tests use a fixed seed per job; failures are reproducible.

## Anti-patterns

- **Sleeping in tests.** If you're tempted to `sleep(100ms)` to wait for a state change, you need a synchronization primitive or a test-only hook. Flaky tests get reverted.
- **Mocking what you own.** Test the real thing when you can. Mock only external processes or network.
- **Snapshots of random data.** Seed RNGs, sort sets, normalize timestamps. Snapshots should be 100% deterministic.

## See also

- [Dev environment setup](./dev-environment)
- [Debugging & tracing](./debugging-and-tracing)
