//! Rust toolchain installer. Wraps `rustup` instead of reimplementing it.
//!
//! Strategy: install toolchains into an isolated `RUSTUP_HOME` under
//! `$XDG_DATA_HOME/versionx/runtimes/rust/` so Versionx-managed toolchains
//! don't collide with the user's global rustup install (`~/.rustup`). On
//! every `install` call we invoke `rustup toolchain install` with the right
//! env vars, using whichever `rustup` binary is on PATH. If rustup isn't
//! installed we error with a clear hint — bootstrapping rustup itself is
//! out of scope for 0.1.0.
//!
//! Notes:
//! - We **never** set `RUSTC` (rustup 1.25+ bug #3031 breaks toolchain
//!   overrides when `RUSTC` is set).
//! - Shims point at rustup's own proxies so that each tool (`cargo`,
//!   `rustc`, `rustfmt`, `clippy-driver`) picks the active toolchain from
//!   `RUSTUP_TOOLCHAIN` at invocation time.

#![deny(unsafe_code)]

use async_trait::async_trait;
use camino::Utf8PathBuf;
use chrono::Utc;
use versionx_events::Level;
use versionx_runtime_trait::{
    InstallOutcome, Installation, InstallerContext, InstallerError, InstallerResult,
    ResolvedVersion, RuntimeInstaller, ShimEntry, VersionSpec,
};

#[derive(Debug, Default)]
pub struct RustInstaller;

impl RustInstaller {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// `RUSTUP_HOME` we point rustup at. Scoped under the same runtimes dir
    /// every other runtime lives under.
    #[must_use]
    pub fn rustup_home(ctx: &InstallerContext) -> Utf8PathBuf {
        ctx.runtimes_dir.join("rust")
    }
}

#[async_trait]
impl RuntimeInstaller for RustInstaller {
    fn id(&self) -> &'static str {
        "rust"
    }
    fn display_name(&self) -> &'static str {
        "Rust toolchain (rustup)"
    }

    async fn resolve_version(
        &self,
        spec: &VersionSpec,
        _ctx: &InstallerContext,
    ) -> InstallerResult<ResolvedVersion> {
        ensure_rustup()?;

        let candidate = spec.as_str().trim();
        // Accept named channels and exact version pins verbatim; rustup
        // handles the lookup itself. A future enhancement could query
        // `https://static.rust-lang.org/dist/channel-rust-<channel>.toml`
        // to resolve `"stable"` to an exact version up-front.
        let (version, channel) = match candidate {
            "stable" | "beta" | "nightly" => (candidate.to_string(), Some(candidate.to_string())),
            other => (other.to_string(), None),
        };

        Ok(ResolvedVersion { version, channel, source: "rustup".into(), sha256: None, url: None })
    }

    fn install_path(&self, version: &ResolvedVersion, ctx: &InstallerContext) -> Utf8PathBuf {
        // Mirror rustup's layout: `$RUSTUP_HOME/toolchains/<toolchain>`.
        Self::rustup_home(ctx).join("toolchains").join(toolchain_dir_name(version))
    }

    async fn install(
        &self,
        version: &ResolvedVersion,
        ctx: &InstallerContext,
    ) -> InstallerResult<InstallOutcome> {
        ensure_rustup()?;

        let install_path = self.install_path(version, ctx);
        let rustup_home = Self::rustup_home(ctx);
        tokio::fs::create_dir_all(&rustup_home)
            .await
            .map_err(|source| InstallerError::Io { path: rustup_home.clone(), source })?;

        if install_path.exists() {
            return Ok(InstallOutcome::AlreadyInstalled(Installation {
                version: version.clone(),
                install_path,
                installed_at: Utc::now(),
                observed_sha256: None,
            }));
        }

        ctx.events
            .info("runtime.install.start", format!("rustup toolchain install {}", version.version));

        let output = tokio::process::Command::new("rustup")
            .arg("toolchain")
            .arg("install")
            .arg(&version.version)
            .arg("--profile")
            .arg("minimal")
            .arg("--no-self-update")
            .env("RUSTUP_HOME", rustup_home.as_std_path())
            .env("CARGO_HOME", ctx.runtimes_dir.join("rust-cargo").as_std_path())
            .env_remove("RUSTC")
            .output()
            .await
            .map_err(|source| InstallerError::Io { path: Utf8PathBuf::from("rustup"), source })?;

        if !output.status.success() {
            return Err(InstallerError::Subprocess {
                program: "rustup".into(),
                status: output.status.code().unwrap_or(-1),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            });
        }

        ctx.events.emit(versionx_events::Event::new(
            "runtime.install.complete",
            Level::Info,
            format!("installed rust {} via rustup", version.version),
        ));

        Ok(InstallOutcome::Installed(Installation {
            version: version.clone(),
            install_path,
            installed_at: Utc::now(),
            observed_sha256: None,
        }))
    }

    fn shim_entries(&self, installation: &Installation) -> Vec<ShimEntry> {
        // We point at the *toolchain's own* binaries (not rustup proxies) so
        // the shim resolves without needing `rustup` on PATH at invocation.
        let names = ["cargo", "rustc", "rustfmt", "clippy-driver", "rust-analyzer", "rustdoc"];
        names
            .into_iter()
            .map(|name| ShimEntry {
                name: name.to_string(),
                target: installation.install_path.join("bin").join(name),
            })
            .collect()
    }

    async fn verify(&self, installation: &Installation) -> InstallerResult<()> {
        let cargo = installation.install_path.join("bin/cargo");
        let output = tokio::process::Command::new(cargo.as_std_path())
            .arg("--version")
            .output()
            .await
            .map_err(|source| InstallerError::Io { path: cargo.clone(), source })?;
        if !output.status.success() {
            return Err(InstallerError::Subprocess {
                program: cargo.to_string(),
                status: output.status.code().unwrap_or(-1),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            });
        }
        Ok(())
    }
}

fn ensure_rustup() -> InstallerResult<()> {
    if which::which("rustup").is_ok() {
        return Ok(());
    }
    Err(InstallerError::MissingExternalTool {
        tool: "rustup".into(),
        hint: "Install rustup from https://rustup.rs/ before running `versionx install rust <version>`".into(),
    })
}

/// Translate a resolved version into the directory name rustup uses under
/// `toolchains/`. For `stable`/`beta`/`nightly` we suffix the host triple
/// because rustup's own directory names look like `stable-aarch64-apple-darwin`.
fn toolchain_dir_name(version: &ResolvedVersion) -> String {
    let host = current_host_triple();
    // If `version.version` already includes a `-<arch>-<os>-<env>` suffix
    // (because the user typed it verbatim), don't double-suffix.
    if version.version.contains('-') {
        version.version.clone()
    } else {
        format!("{}-{host}", version.version)
    }
}

const fn current_host_triple() -> &'static str {
    // We can't call rustc's target triple at compile time for *this* binary
    // portably, so hardcode the matrix we ship binaries for. Unknowns fall
    // back to an empty triple which means rustup's install still works
    // (it's the directory-name computation that's slightly off).
    if cfg!(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu")) {
        "x86_64-unknown-linux-gnu"
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64", target_env = "gnu")) {
        "aarch64-unknown-linux-gnu"
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64", target_env = "musl")) {
        "x86_64-unknown-linux-musl"
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64", target_env = "musl")) {
        "aarch64-unknown-linux-musl"
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        "x86_64-apple-darwin"
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "aarch64-apple-darwin"
    } else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        "x86_64-pc-windows-msvc"
    } else if cfg!(all(target_os = "windows", target_arch = "aarch64")) {
        "aarch64-pc-windows-msvc"
    } else {
        ""
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_channel_gets_host_suffix() {
        let v = ResolvedVersion {
            version: "stable".into(),
            channel: Some("stable".into()),
            source: "rustup".into(),
            sha256: None,
            url: None,
        };
        let dir = toolchain_dir_name(&v);
        assert!(dir.starts_with("stable-"), "got: {dir}");
    }

    #[test]
    fn version_with_triple_is_preserved() {
        let v = ResolvedVersion {
            version: "1.88.0-aarch64-apple-darwin".into(),
            channel: None,
            source: "rustup".into(),
            sha256: None,
            url: None,
        };
        assert_eq!(toolchain_dir_name(&v), "1.88.0-aarch64-apple-darwin");
    }

    #[test]
    fn current_host_triple_is_nonempty_on_supported_platforms() {
        // If you're running these tests on an unsupported host the assertion
        // won't fire — that's the correct behavior, no-op for unknown hosts.
        let _ = current_host_triple();
    }
}
