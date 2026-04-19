//! The `RuntimeInstaller` trait + shared types every installer uses.
//!
//! Installer crates (`versionx-runtime-node`, `-python`, `-rust`, …)
//! implement this trait. `versionx-core` holds a registry and dispatches
//! by tool id.
//!
//! See `docs/spec/04-runtime-toolchain-mgmt.md §2`.

#![deny(unsafe_code)]

pub mod download;
pub mod error;
pub mod platform;

use async_trait::async_trait;
use camino::{Utf8Path, Utf8PathBuf};
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};

pub use error::{InstallerError, InstallerResult};
pub use platform::{Arch, Libc, Os, Platform};
use versionx_events::EventSender;

/// Context handed to every installer method. Cheap to clone.
#[derive(Clone)]
pub struct InstallerContext {
    /// Root directory under which installers create `<tool>/<version>/`.
    pub runtimes_dir: Utf8PathBuf,
    /// Shared cache dir (tarball downloads, metadata).
    pub cache_dir: Utf8PathBuf,
    /// Shared reqwest client — reuses connections across installers.
    pub http: Client,
    /// Event bus for progress + download + verify events.
    pub events: EventSender,
    /// Detected host platform.
    pub platform: Platform,
}

impl std::fmt::Debug for InstallerContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InstallerContext")
            .field("runtimes_dir", &self.runtimes_dir)
            .field("cache_dir", &self.cache_dir)
            .field("platform", &self.platform)
            .finish_non_exhaustive()
    }
}

/// Ask for a specific version. Installers may accept free-form channel labels
/// (`"lts"`, `"stable"`, `"nightly-2024-04-18"`) and resolve to an exact version.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionSpec(pub String);

impl VersionSpec {
    #[must_use]
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A concrete version the installer can act on.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedVersion {
    /// Fully-qualified version (`"22.12.0"`, `"3.12.2"`).
    pub version: String,
    /// Optional channel label (`"lts"`, `"stable"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
    /// Source / distribution label (`"nodejs.org"`, `"python-build-standalone"`, `"rustup"`).
    pub source: String,
    /// SHA-256 of the primary installer archive, where known up front.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    /// URL of the archive (informational; some installers don't have one).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// A completed installation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Installation {
    pub version: ResolvedVersion,
    /// Absolute install directory.
    pub install_path: Utf8PathBuf,
    pub installed_at: DateTime<Utc>,
    /// SHA-256 observed on the downloaded artifact, if verified.
    pub observed_sha256: Option<String>,
}

/// Shims produced by an installer — the shell-facing binaries we want on PATH.
#[derive(Clone, Debug)]
pub struct ShimEntry {
    /// `"node"`, `"npm"`, `"pip"`, ...
    pub name: String,
    /// Path to the real binary inside [`Installation::install_path`].
    pub target: Utf8PathBuf,
}

/// Outcome of [`RuntimeInstaller::install`].
#[derive(Clone, Debug)]
pub enum InstallOutcome {
    /// The runtime was downloaded + extracted this call.
    Installed(Installation),
    /// The runtime was already present — no work done.
    AlreadyInstalled(Installation),
}

impl InstallOutcome {
    #[must_use]
    pub const fn installation(&self) -> &Installation {
        match self {
            Self::Installed(i) | Self::AlreadyInstalled(i) => i,
        }
    }
}

/// The trait every runtime installer implements. Async because installs hit
/// the network. Object-safe via `async_trait`.
#[async_trait]
pub trait RuntimeInstaller: Send + Sync {
    /// Stable id (`"node"`, `"python"`, `"rust"`, `"pnpm"`, ...).
    fn id(&self) -> &'static str;

    /// Human-friendly name for UI (`"Node.js"`, `"CPython"`).
    fn display_name(&self) -> &'static str;

    /// Resolve a user-supplied spec to an exact version. Implementations may
    /// hit the network to look up channel labels (`"lts"`) but should cache
    /// aggressively.
    async fn resolve_version(
        &self,
        spec: &VersionSpec,
        ctx: &InstallerContext,
    ) -> InstallerResult<ResolvedVersion>;

    /// Directory where this version would be installed. Pure — no I/O.
    fn install_path(&self, version: &ResolvedVersion, ctx: &InstallerContext) -> Utf8PathBuf {
        ctx.runtimes_dir.join(self.id()).join(&version.version)
    }

    /// Whether the install exists + looks functional on disk.
    async fn is_installed(&self, version: &ResolvedVersion, ctx: &InstallerContext) -> bool {
        self.install_path(version, ctx).exists()
    }

    /// Install. Idempotent: if already installed, returns
    /// [`InstallOutcome::AlreadyInstalled`].
    async fn install(
        &self,
        version: &ResolvedVersion,
        ctx: &InstallerContext,
    ) -> InstallerResult<InstallOutcome>;

    /// Remove an installation. Never affects unrelated versions.
    async fn uninstall(
        &self,
        version: &ResolvedVersion,
        ctx: &InstallerContext,
    ) -> InstallerResult<()> {
        let path = self.install_path(version, ctx);
        if path.exists() {
            tokio::fs::remove_dir_all(&path)
                .await
                .map_err(|source| InstallerError::Io { path: path.clone(), source })?;
        }
        Ok(())
    }

    /// Names + in-install paths of the binaries we want to shim.
    fn shim_entries(&self, installation: &Installation) -> Vec<ShimEntry>;

    /// Optional sanity-check after install. Default: no-op.
    async fn verify(&self, _installation: &Installation) -> InstallerResult<()> {
        Ok(())
    }
}

/// Resolve the path to a binary inside an [`Installation`], following OS
/// conventions: `bin/<name>` on Unix, `<name>.exe` at the install root on Windows
/// for tools like Node that bundle everything at the top level.
#[must_use]
pub fn bin_path(install: &Utf8Path, name: &str, platform: Platform) -> Utf8PathBuf {
    if matches!(platform.os, Os::Windows) {
        install.join(format!("{name}.exe"))
    } else {
        install.join("bin").join(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolved_version_round_trips_through_serde() {
        let v = ResolvedVersion {
            version: "22.12.0".into(),
            channel: Some("lts".into()),
            source: "nodejs.org".into(),
            sha256: Some("deadbeef".into()),
            url: None,
        };
        let j = serde_json::to_string(&v).unwrap();
        let back: ResolvedVersion = serde_json::from_str(&j).unwrap();
        assert_eq!(back, v);
    }

    #[test]
    fn bin_path_platform_aware() {
        let linux = Platform::new(Os::Linux, Arch::X86_64, Libc::Glibc);
        let win = Platform::new(Os::Windows, Arch::X86_64, Libc::Msvc);
        assert_eq!(
            bin_path(Utf8Path::new("/rt/node/20"), "node", linux).as_str(),
            "/rt/node/20/bin/node"
        );
        assert_eq!(
            bin_path(Utf8Path::new("C:/rt/node/20"), "node", win).as_str(),
            "C:/rt/node/20/node.exe"
        );
    }
}
