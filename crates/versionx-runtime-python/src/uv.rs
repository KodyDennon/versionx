//! `uv` runtime installer — astral-sh/uv.
//!
//! Source: GitHub releases at `github.com/astral-sh/uv`. Each release
//! ships per-target archives:
//!   - `uv-<triple>.tar.gz` on Unix
//!   - `uv-<triple>.zip` on Windows
//!
//! The archive contains the `uv` and `uvx` binaries at the root. We
//! resolve the version (exact, prefix, or `"latest"`), download the
//! matching archive, and extract into the install dir. No checksum
//! verification yet — astral's release stream doesn't ship sidecar
//! `.sha256` files at fixed URLs (they post a single
//! `dist-manifest.json`); fetching + parsing that is a clean follow-up.

use async_trait::async_trait;
use chrono::Utc;
use serde::Deserialize;
use versionx_events::Level;
use versionx_runtime_trait::{
    Arch, InstallOutcome, Installation, InstallerContext, InstallerError, InstallerResult, Libc,
    Os, Platform, ResolvedVersion, RuntimeInstaller, ShimEntry, VersionSpec,
    download::{download_to_file, download_to_memory, extract_tar, extract_zip},
};

#[derive(Debug, Default)]
pub struct UvInstaller;

impl UvInstaller {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

#[async_trait]
impl RuntimeInstaller for UvInstaller {
    fn id(&self) -> &'static str {
        "uv"
    }
    fn display_name(&self) -> &'static str {
        "uv (astral-sh/uv)"
    }

    async fn resolve_version(
        &self,
        spec: &VersionSpec,
        ctx: &InstallerContext,
    ) -> InstallerResult<ResolvedVersion> {
        let triple = uv_triple(ctx.platform).ok_or_else(|| {
            InstallerError::UnsupportedPlatform { tool: "uv", platform: ctx.platform.to_string() }
        })?;
        let version = resolve_uv_version(&ctx.http, spec.as_str()).await?;
        let ext = if matches!(ctx.platform.os, Os::Windows) { "zip" } else { "tar.gz" };
        let url = format!(
            "https://github.com/astral-sh/uv/releases/download/{version}/uv-{triple}.{ext}"
        );
        Ok(ResolvedVersion {
            version,
            channel: None,
            source: "github:astral-sh/uv".into(),
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
            return Err(InstallerError::Other("uv url missing — resolve_version first".into()));
        };
        let artifact_name = url.rsplit('/').next().unwrap_or("uv").to_string();
        let archive_path = ctx.cache_dir.join("uv").join(&artifact_name);
        let download_sha = download_to_file(&ctx.http, &url, &archive_path, &ctx.events).await?;

        if install_path.exists() {
            let _ = std::fs::remove_dir_all(&install_path);
        }
        // The astral-sh/uv archives put `uv` and `uvx` at the
        // top level — strip 0 components.
        if artifact_name.ends_with(".zip") {
            extract_zip(&archive_path, &install_path, 0)?;
        } else {
            extract_tar(&archive_path, &install_path, 0)?;
        }

        ctx.events.emit(versionx_events::Event::new(
            "runtime.install.complete",
            Level::Info,
            format!("installed uv {} at {}", version.version, install_path),
        ));

        Ok(InstallOutcome::Installed(Installation {
            version: ResolvedVersion { sha256: Some(download_sha.clone()), ..version.clone() },
            install_path,
            installed_at: Utc::now(),
            observed_sha256: Some(download_sha),
        }))
    }

    fn shim_entries(&self, installation: &Installation) -> Vec<ShimEntry> {
        let bin = if cfg!(target_os = "windows") { "uv.exe" } else { "uv" };
        let bin_x = if cfg!(target_os = "windows") { "uvx.exe" } else { "uvx" };
        vec![
            ShimEntry { name: "uv".into(), target: installation.install_path.join(bin) },
            ShimEntry { name: "uvx".into(), target: installation.install_path.join(bin_x) },
        ]
    }

    async fn verify(&self, installation: &Installation) -> InstallerResult<()> {
        let bin_name = if cfg!(target_os = "windows") { "uv.exe" } else { "uv" };
        let bin = installation.install_path.join(bin_name);
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

/// Map a [`Platform`] to the triple uv uses in asset names.
#[must_use]
pub const fn uv_triple(platform: Platform) -> Option<&'static str> {
    Some(match (platform.os, platform.arch, platform.libc) {
        (Os::Linux, Arch::X86_64, Libc::Glibc) => "x86_64-unknown-linux-gnu",
        (Os::Linux, Arch::Aarch64, Libc::Glibc) => "aarch64-unknown-linux-gnu",
        (Os::Linux, Arch::X86_64, Libc::Musl) => "x86_64-unknown-linux-musl",
        (Os::Linux, Arch::Aarch64, Libc::Musl) => "aarch64-unknown-linux-musl",
        (Os::MacOs, Arch::X86_64, _) => "x86_64-apple-darwin",
        (Os::MacOs, Arch::Aarch64, _) => "aarch64-apple-darwin",
        (Os::Windows, Arch::X86_64, _) => "x86_64-pc-windows-msvc",
        (Os::Windows, Arch::Aarch64, _) => "aarch64-pc-windows-msvc",
        _ => return None,
    })
}

#[derive(Deserialize)]
struct UvLatest {
    tag_name: String,
}

#[derive(Deserialize)]
struct UvRelease {
    tag_name: String,
    #[serde(default)]
    prerelease: bool,
}

/// Resolve a uv version spec to an exact version string.
///
/// Accepts:
///   - exact: `0.4.30`
///   - tag form: `v0.4.30` (we strip the v)
///   - prefix: `0.4`
///   - alias: `latest` / `stable`
async fn resolve_uv_version(http: &reqwest::Client, spec: &str) -> InstallerResult<String> {
    let bare = spec.trim().trim_start_matches('v');
    if looks_exact(bare) {
        return Ok(bare.to_string());
    }
    if bare == "latest" || bare == "stable" || bare == "current" {
        let url = "https://api.github.com/repos/astral-sh/uv/releases/latest";
        let (bytes, _) = download_to_memory(http, url).await?;
        let rel: UvLatest = serde_json::from_slice(&bytes)
            .map_err(|e| InstallerError::Other(format!("parsing uv latest release: {e}")))?;
        return Ok(rel.tag_name);
    }
    // Prefix match — scan recent releases.
    let url = "https://api.github.com/repos/astral-sh/uv/releases?per_page=50";
    let (bytes, _) = download_to_memory(http, url).await?;
    let releases: Vec<UvRelease> = serde_json::from_slice(&bytes)
        .map_err(|e| InstallerError::Other(format!("parsing uv releases: {e}")))?;
    let mut best: Option<(semver::Version, String)> = None;
    for r in releases.iter().filter(|r| !r.prerelease) {
        let ver_str = r.tag_name.trim_start_matches('v').to_string();
        let Ok(parsed) = semver::Version::parse(&ver_str) else { continue };
        if ver_str.starts_with(bare) {
            match &best {
                Some((cur, _)) if cur >= &parsed => {}
                _ => best = Some((parsed, r.tag_name.clone())),
            }
        }
    }
    best.map(|(_, v)| v)
        .ok_or_else(|| InstallerError::UnresolvableVersion { tool: "uv", spec: spec.to_string() })
}

fn looks_exact(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    parts.len() == 3 && parts.iter().all(|p| p.parse::<u32>().is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triple_lookup() {
        let mac = Platform::new(Os::MacOs, Arch::Aarch64, Libc::Apple);
        assert_eq!(uv_triple(mac), Some("aarch64-apple-darwin"));
        let win = Platform::new(Os::Windows, Arch::X86_64, Libc::Msvc);
        assert_eq!(uv_triple(win), Some("x86_64-pc-windows-msvc"));
    }

    #[test]
    fn looks_exact_is_strict() {
        assert!(looks_exact("0.4.30"));
        assert!(!looks_exact("0.4"));
        assert!(!looks_exact("latest"));
    }
}
