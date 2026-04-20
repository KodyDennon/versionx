---
title: Adding a runtime installer
description: Port a language toolchain to Versionx. The RuntimeInstaller trait, verification, shim wiring, platform matrix.
sidebar_position: 5
---

# Adding a runtime installer

You'll learn:

- The `RuntimeInstaller` trait.
- Sources: official archives, community builds, or wrap an existing installer.
- How shims are wired for the new runtime.

## The trait

Lives in `versionx-runtime-trait`:

```rust
#[async_trait::async_trait]
pub trait RuntimeInstaller: Send + Sync {
    fn id(&self) -> &'static str;            // "node", "python", "rust", ...

    async fn available_versions(&self) -> Result<Vec<Version>>;
    async fn resolve(&self, spec: &VersionSpec) -> Result<Version>;

    async fn install(&self, version: &Version, target: &Utf8Path) -> Result<InstallReport>;
    async fn verify(&self, target: &Utf8Path) -> Result<()>;
    async fn uninstall(&self, target: &Utf8Path) -> Result<()>;

    fn binaries(&self, target: &Utf8Path) -> Vec<BinaryEntry>;
}
```

## Source strategy

| Strategy | When |
|---|---|
| **Official archives** | If the project publishes platform-matrix tarballs/zips. Versionx downloads, verifies SHA256, extracts. (Node, Python via python-build-standalone.) |
| **Community builds** | If official builds don't cover every platform. Use a well-maintained mirror (e.g., python-build-standalone). |
| **Wrap an existing installer** | If the upstream installer is robust and you don't want to own build pipelines. (Rust: wrap rustup.) |

Tier-1 targets all platforms Versionx supports. Tier-2 may skip some (e.g., JVM on musl).

## Project layout

```
crates/versionx-runtime-<name>/
├── Cargo.toml
├── src/
│   ├── lib.rs
│   ├── installer.rs      # impl RuntimeInstaller
│   ├── sources.rs        # where to get the archive
│   ├── verify.rs         # SHA256 / signature checks
│   └── binaries.rs       # which exes live in the archive
└── tests/
    ├── resolve.rs
    └── install_mock.rs   # uses a fake archive server
```

## Verify

- **SHA256 mandatory.** Hash the archive before extracting. If the vendor publishes sigs, verify those too.
- **`verify()` post-install.** Run the installed binary once with `--version` and compare against the expected version string. Catches corrupted extracts.

```rust
async fn verify(&self, target: &Utf8Path) -> Result<()> {
    let out = Command::new(target.join("bin/node")).arg("--version").output().await?;
    let ver = String::from_utf8_lossy(&out.stdout).trim().to_string();
    // compare against expected
    Ok(())
}
```

## Shim wiring

The shim ecosystem picks up installed binaries automatically via `RuntimeInstaller::binaries()`:

```rust
fn binaries(&self, target: &Utf8Path) -> Vec<BinaryEntry> {
    vec![
        BinaryEntry::exe(target.join("bin/node")),
        BinaryEntry::exe(target.join("bin/npm")),
        BinaryEntry::exe(target.join("bin/npx")),
    ]
}
```

Versionx writes shims for each listed binary into `$XDG_DATA_HOME/versionx/shims/`. The shim is the tiny `versionx-shim` trampoline; it dispatches to the right version based on the current directory's `versionx.toml`.

## Platform matrix

In `Cargo.toml`:

```toml
[package.metadata.versionx]
runtime = "<name>"
tier = 1
platforms = ["x86_64-linux-gnu", "x86_64-linux-musl", "aarch64-linux-gnu",
             "x86_64-darwin", "aarch64-darwin",
             "x86_64-windows-msvc", "aarch64-windows-msvc"]
```

CI runs install tests on every platform listed. If your runtime can't support a platform yet, omit it — the CI matrix will skip.

## Tests

- **Resolve.** Given `VersionSpec::Channel("stable")`, does it pick a real version? Use a recorded fixture of the version list.
- **Install (mocked).** Run against a local fake archive server (`tests/install_mock.rs` includes a helper).
- **Verify.** Sanity-check `verify()` against a real install in a scheduled CI job (not on every PR — too slow).

## Registering

Same as adapters — add to `versionx-runtime-*` meta re-export and the core registry.

## Documentation

- A page under `/docs/contributing/` is optional unless the runtime has platform quirks worth calling out.
- Doc comments on every `pub` item.
- Add to [Managing toolchains](/guides/managing-toolchains) (the user-facing page) once the runtime is Stable.

## See also

- [Adding a package-manager adapter](./adding-a-package-manager-adapter)
- [`docs/spec/04-runtime-toolchain-mgmt.md`](https://github.com/KodyDennon/versionx/blob/main/docs/spec/04-runtime-toolchain-mgmt.md) — runtime design spec.
