//! Node package managers as managed runtimes.
//!
//! We ship pnpm + yarn (classic + berry) as independent runtimes rather than
//! relying on corepack — Node 25+ removed corepack from the default
//! distribution, and even on Node <25 corepack had multiple signature-
//! verification breakages in early 2025. Owning the install here makes
//! Versionx's behavior predictable across Node versions.

use async_trait::async_trait;
use chrono::Utc;
use serde::Deserialize;
use versionx_events::Level;
use versionx_runtime_trait::{
    Arch, InstallOutcome, Installation, InstallerContext, InstallerError, InstallerResult, Libc,
    Os, Platform, ResolvedVersion, RuntimeInstaller, ShimEntry, VersionSpec,
    download::{download_to_file, download_to_memory, extract_tar, extract_zip},
};

/// pnpm installer. Uses the official standalone binaries from
/// `github.com/pnpm/pnpm/releases`.
#[derive(Debug, Default)]
pub struct PnpmInstaller;

impl PnpmInstaller {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Asset filename for a given pnpm version + platform.
    /// pnpm names them `pnpm-<os>-<arch>[-libc]` (no extension on Unix, `.exe`
    /// on Windows) — a single static binary per release.
    #[must_use]
    pub const fn artifact_name(platform: Platform) -> Option<&'static str> {
        Some(match (platform.os, platform.arch, platform.libc) {
            (Os::Linux, Arch::X86_64, Libc::Glibc) => "pnpm-linux-x64",
            (Os::Linux, Arch::Aarch64, Libc::Glibc) => "pnpm-linux-arm64",
            (Os::Linux, Arch::X86_64, Libc::Musl) => "pnpm-linuxstatic-x64",
            (Os::Linux, Arch::Aarch64, Libc::Musl) => "pnpm-linuxstatic-arm64",
            (Os::MacOs, Arch::X86_64, _) => "pnpm-macos-x64",
            (Os::MacOs, Arch::Aarch64, _) => "pnpm-macos-arm64",
            (Os::Windows, Arch::X86_64, _) => "pnpm-win-x64.exe",
            (Os::Windows, Arch::Aarch64, _) => "pnpm-win-arm64.exe",
            _ => return None,
        })
    }
}

#[async_trait]
impl RuntimeInstaller for PnpmInstaller {
    fn id(&self) -> &'static str {
        "pnpm"
    }
    fn display_name(&self) -> &'static str {
        "pnpm"
    }

    async fn resolve_version(
        &self,
        spec: &VersionSpec,
        ctx: &InstallerContext,
    ) -> InstallerResult<ResolvedVersion> {
        let version =
            resolve_github_release_version(&ctx.http, "pnpm", "pnpm", spec.as_str()).await?;
        let artifact = Self::artifact_name(ctx.platform).ok_or_else(|| {
            InstallerError::UnsupportedPlatform { tool: "pnpm", platform: ctx.platform.to_string() }
        })?;
        let url = format!("https://github.com/pnpm/pnpm/releases/download/v{version}/{artifact}");
        Ok(ResolvedVersion {
            version,
            channel: None,
            source: "github:pnpm/pnpm".into(),
            sha256: None,
            url: Some(url),
        })
    }

    async fn install(
        &self,
        version: &ResolvedVersion,
        ctx: &InstallerContext,
    ) -> InstallerResult<InstallOutcome> {
        let install_path = self.install_path(version, ctx);
        if self.is_installed(version, ctx).await {
            return Ok(InstallOutcome::AlreadyInstalled(Installation {
                version: version.clone(),
                install_path,
                installed_at: Utc::now(),
                observed_sha256: None,
            }));
        }
        let Some(url) = version.url.clone() else {
            return Err(InstallerError::Other("pnpm url missing — resolve_version first".into()));
        };
        let artifact_name = url.rsplit('/').next().unwrap_or("pnpm-binary").to_string();
        let archive_path = ctx.cache_dir.join("pnpm").join(&artifact_name);
        let download_sha = download_to_file(&ctx.http, &url, &archive_path, &ctx.events).await?;

        // pnpm ships as a single binary — install is just "copy + chmod +x".
        tokio::fs::create_dir_all(install_path.join("bin"))
            .await
            .map_err(|source| InstallerError::Io { path: install_path.clone(), source })?;
        let target = install_path.join("bin/pnpm");
        tokio::fs::copy(&archive_path, &target)
            .await
            .map_err(|source| InstallerError::Io { path: target.clone(), source })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(target.as_std_path())
                .map_err(|source| InstallerError::Io { path: target.clone(), source })?
                .permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(target.as_std_path(), perms)
                .map_err(|source| InstallerError::Io { path: target.clone(), source })?;
        }

        ctx.events.emit(versionx_events::Event::new(
            "runtime.install.complete",
            Level::Info,
            format!("installed pnpm {} at {}", version.version, install_path),
        ));

        Ok(InstallOutcome::Installed(Installation {
            version: ResolvedVersion { sha256: Some(download_sha.clone()), ..version.clone() },
            install_path,
            installed_at: Utc::now(),
            observed_sha256: Some(download_sha),
        }))
    }

    fn shim_entries(&self, installation: &Installation) -> Vec<ShimEntry> {
        vec![ShimEntry { name: "pnpm".into(), target: installation.install_path.join("bin/pnpm") }]
    }

    async fn verify(&self, installation: &Installation) -> InstallerResult<()> {
        let bin = installation.install_path.join("bin/pnpm");
        let output = tokio::process::Command::new(bin.as_std_path())
            .arg("--version")
            .output()
            .await
            .map_err(|source| InstallerError::Io { path: bin.clone(), source })?;
        if !output.status.success() {
            return Err(InstallerError::Subprocess {
                program: bin.to_string(),
                status: output.status.code().unwrap_or(-1),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            });
        }
        Ok(())
    }
}

/// yarn installer. Covers both yarn classic (v1) and yarn berry (v2+).
///
/// yarn classic is distributed as a tarball on GitHub Releases. Berry ships
/// a single `yarn-<version>.js` released under the `yarnpkg/berry` repo
/// which users typically install via `corepack`; we install the tarball
/// of the CLI from GH Releases and shim to it.
#[derive(Debug, Default)]
pub struct YarnInstaller;

impl YarnInstaller {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Treat the major-version digit as the selector between classic + berry.
    fn release_flavor(version: &str) -> YarnFlavor {
        version.split('.').next().and_then(|s| s.parse::<u32>().ok()).map_or(
            YarnFlavor::Classic,
            |major| {
                if major >= 2 { YarnFlavor::Berry } else { YarnFlavor::Classic }
            },
        )
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum YarnFlavor {
    Classic,
    Berry,
}

#[async_trait]
impl RuntimeInstaller for YarnInstaller {
    fn id(&self) -> &'static str {
        "yarn"
    }
    fn display_name(&self) -> &'static str {
        "Yarn"
    }

    async fn resolve_version(
        &self,
        spec: &VersionSpec,
        ctx: &InstallerContext,
    ) -> InstallerResult<ResolvedVersion> {
        let version =
            resolve_github_release_version(&ctx.http, "yarnpkg", "yarn", spec.as_str()).await?;
        let url = format!(
            "https://github.com/yarnpkg/yarn/releases/download/v{version}/yarn-v{version}.tar.gz"
        );
        Ok(ResolvedVersion {
            version,
            channel: None,
            source: "github:yarnpkg/yarn".into(),
            sha256: None,
            url: Some(url),
        })
    }

    async fn install(
        &self,
        version: &ResolvedVersion,
        ctx: &InstallerContext,
    ) -> InstallerResult<InstallOutcome> {
        let install_path = self.install_path(version, ctx);
        if self.is_installed(version, ctx).await {
            return Ok(InstallOutcome::AlreadyInstalled(Installation {
                version: version.clone(),
                install_path,
                installed_at: Utc::now(),
                observed_sha256: None,
            }));
        }
        let Some(url) = version.url.clone() else {
            return Err(InstallerError::Other("yarn url missing — resolve_version first".into()));
        };
        let artifact_name = url.rsplit('/').next().unwrap_or("yarn.tar.gz").to_string();
        let archive_path = ctx.cache_dir.join("yarn").join(&artifact_name);
        let download_sha = download_to_file(&ctx.http, &url, &archive_path, &ctx.events).await?;

        if install_path.exists() {
            let _ = std::fs::remove_dir_all(&install_path);
        }
        match Self::release_flavor(&version.version) {
            YarnFlavor::Classic | YarnFlavor::Berry => {
                // Both flavours ship as tar.gz with a single top-level dir.
                if artifact_name.ends_with(".zip") {
                    extract_zip(&archive_path, &install_path, 1)?;
                } else {
                    extract_tar(&archive_path, &install_path, 1)?;
                }
            }
        }

        ctx.events.emit(versionx_events::Event::new(
            "runtime.install.complete",
            Level::Info,
            format!("installed yarn {} at {}", version.version, install_path),
        ));

        Ok(InstallOutcome::Installed(Installation {
            version: ResolvedVersion { sha256: Some(download_sha.clone()), ..version.clone() },
            install_path,
            installed_at: Utc::now(),
            observed_sha256: Some(download_sha),
        }))
    }

    fn shim_entries(&self, installation: &Installation) -> Vec<ShimEntry> {
        vec![
            ShimEntry { name: "yarn".into(), target: installation.install_path.join("bin/yarn") },
            ShimEntry {
                name: "yarnpkg".into(),
                target: installation.install_path.join("bin/yarnpkg"),
            },
        ]
    }

    async fn verify(&self, installation: &Installation) -> InstallerResult<()> {
        let bin = installation.install_path.join("bin/yarn");
        let output = tokio::process::Command::new(bin.as_std_path())
            .arg("--version")
            .output()
            .await
            .map_err(|source| InstallerError::Io { path: bin.clone(), source })?;
        if !output.status.success() {
            return Err(InstallerError::Subprocess {
                program: bin.to_string(),
                status: output.status.code().unwrap_or(-1),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            });
        }
        Ok(())
    }
}

/// Resolve a GitHub-hosted tag to an exact version.
///
/// - Exact versions (`"8.15.0"`, `"v8.15.0"`) pass through.
/// - `"latest"` / `"stable"` fetch `/releases/latest`.
/// - Prefixes (`"8"`, `"8.15"`) scan `/releases?per_page=50` and pick the
///   highest matching tag.
async fn resolve_github_release_version(
    http: &reqwest::Client,
    owner: &str,
    repo: &str,
    spec: &str,
) -> InstallerResult<String> {
    let bare = spec.trim().trim_start_matches('v');

    // Exact version: `X.Y.Z`
    if looks_exact(bare) {
        return Ok(bare.to_string());
    }

    if bare == "latest" || bare == "stable" || bare == "current" {
        let url = format!("https://api.github.com/repos/{owner}/{repo}/releases/latest");
        let (bytes, _) = download_to_memory(http, &url).await?;
        #[derive(Deserialize)]
        struct LatestRelease {
            tag_name: String,
        }
        let rel: LatestRelease = serde_json::from_slice(&bytes).map_err(|e| {
            InstallerError::Other(format!("parsing {owner}/{repo} latest release: {e}"))
        })?;
        return Ok(rel.tag_name.trim_start_matches('v').to_string());
    }

    // Prefix match.
    let url = format!("https://api.github.com/repos/{owner}/{repo}/releases?per_page=50");
    let (bytes, _) = download_to_memory(http, &url).await?;
    #[derive(Deserialize)]
    struct Release {
        tag_name: String,
        #[serde(default)]
        prerelease: bool,
    }
    let mut releases: Vec<Release> = serde_json::from_slice(&bytes)
        .map_err(|e| InstallerError::Other(format!("parsing {owner}/{repo} releases: {e}")))?;
    releases.retain(|r| !r.prerelease);

    let parts: Vec<&str> = bare.split('.').collect();
    let mut best: Option<(semver::Version, String)> = None;
    for r in &releases {
        let ver_str = r.tag_name.trim_start_matches('v').to_string();
        let Ok(parsed) = semver::Version::parse(&ver_str) else {
            continue;
        };
        let matches_prefix = match parts.as_slice() {
            [maj] => maj.parse::<u64>().is_ok_and(|m| parsed.major == m),
            [maj, min] => {
                maj.parse::<u64>().is_ok_and(|m| parsed.major == m)
                    && min.parse::<u64>().is_ok_and(|n| parsed.minor == n)
            }
            _ => false,
        };
        if matches_prefix {
            match &best {
                Some((cur, _)) if cur >= &parsed => {}
                _ => best = Some((parsed, ver_str)),
            }
        }
    }
    best.map(|(_, v)| v).ok_or_else(|| InstallerError::UnresolvableVersion {
        tool: "github-release",
        spec: spec.to_string(),
    })
}

fn looks_exact(v: &str) -> bool {
    // Must match major.minor.patch with optional pre-release.
    semver::Version::parse(v).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pnpm_artifact_for_common_platforms() {
        let linux = Platform::new(Os::Linux, Arch::X86_64, Libc::Glibc);
        assert_eq!(PnpmInstaller::artifact_name(linux), Some("pnpm-linux-x64"));
        let mac = Platform::new(Os::MacOs, Arch::Aarch64, Libc::Apple);
        assert_eq!(PnpmInstaller::artifact_name(mac), Some("pnpm-macos-arm64"));
        let win = Platform::new(Os::Windows, Arch::X86_64, Libc::Msvc);
        assert_eq!(PnpmInstaller::artifact_name(win), Some("pnpm-win-x64.exe"));
    }

    #[test]
    fn yarn_flavor_detection() {
        assert_eq!(YarnInstaller::release_flavor("1.22.22"), YarnFlavor::Classic);
        assert_eq!(YarnInstaller::release_flavor("2.0.0-rc.36"), YarnFlavor::Berry);
        assert_eq!(YarnInstaller::release_flavor("4.5.0"), YarnFlavor::Berry);
    }

    #[test]
    fn exact_version_passes_through() {
        assert!(looks_exact("8.15.0"));
        assert!(looks_exact("1.22.22"));
        assert!(!looks_exact("8"));
        assert!(!looks_exact("8.15"));
        assert!(!looks_exact("latest"));
    }
}
