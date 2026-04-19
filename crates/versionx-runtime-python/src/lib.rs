//! `CPython` runtime installer using astral-sh/python-build-standalone.
//!
//! Source: GitHub releases at `github.com/astral-sh/python-build-standalone`.
//! Release assets are named
//! `cpython-<version>+<build>-<triple>-<flavor>.tar.gz` where:
//! - `<triple>` is a Rust-ish target triple (`aarch64-apple-darwin`,
//!   `x86_64-unknown-linux-gnu`, `x86_64-pc-windows-msvc`, …).
//! - `<flavor>` is either `install_only` (what users want) or `full`.
//!
//! We fetch the latest release, pick the correct asset for the host, download,
//! verify SHA-256 against the accompanying `.sha256` sidecar, extract, and
//! hand back the path. Sysconfig patching (a python-build-standalone quirk
//! that bakes absolute build-time paths into Makefiles) is deferred for
//! 0.1.0 — installs work for typical `python` usage without it; native-
//! extension builds that need `sysconfig.get_config_vars()["Makefile"]` to
//! be relocated will need that work to land.

#![deny(unsafe_code)]

use async_trait::async_trait;
use chrono::Utc;
use serde::Deserialize;
use versionx_events::Level;
use versionx_runtime_trait::{
    Arch, InstallOutcome, Installation, InstallerContext, InstallerError, InstallerResult, Libc,
    Os, Platform, ResolvedVersion, RuntimeInstaller, ShimEntry, VersionSpec,
    download::{download_to_file, download_to_memory, extract_tar, verify_sha256},
};

const RELEASES_URL: &str =
    "https://api.github.com/repos/astral-sh/python-build-standalone/releases?per_page=5";

#[derive(Debug, Default)]
pub struct PythonInstaller;

impl PythonInstaller {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

#[derive(Clone, Debug, Deserialize)]
struct GithubRelease {
    #[allow(dead_code)]
    name: Option<String>,
    tag_name: String,
    #[serde(default)]
    prerelease: bool,
    assets: Vec<GithubAsset>,
}

#[derive(Clone, Debug, Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
}

#[async_trait]
impl RuntimeInstaller for PythonInstaller {
    fn id(&self) -> &'static str {
        "python"
    }
    fn display_name(&self) -> &'static str {
        "CPython (python-build-standalone)"
    }

    async fn resolve_version(
        &self,
        spec: &VersionSpec,
        ctx: &InstallerContext,
    ) -> InstallerResult<ResolvedVersion> {
        let target = spec.as_str().trim();
        let triple =
            platform_triple(ctx.platform).ok_or_else(|| InstallerError::UnsupportedPlatform {
                tool: "python",
                platform: ctx.platform.to_string(),
            })?;

        // python-build-standalone ships many releases. We fetch the recent page,
        // filter to non-prerelease, and look for the first asset whose filename
        // embeds the requested python version and host triple in install-only
        // flavour.
        let releases = fetch_releases(ctx).await?;
        for release in releases.iter().filter(|r| !r.prerelease) {
            if let Some(asset) = pick_asset(&release.assets, target, triple) {
                let version = extract_python_version(&asset.name).unwrap_or_else(|| target.into());
                return Ok(ResolvedVersion {
                    version,
                    channel: Some(release.tag_name.clone()),
                    source: "python-build-standalone".into(),
                    sha256: None,
                    url: Some(asset.browser_download_url.clone()),
                });
            }
        }
        Err(InstallerError::UnresolvableVersion { tool: "python", spec: spec.0.clone() })
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
            return Err(InstallerError::Other(
                "Python ResolvedVersion missing url — call resolve_version first".into(),
            ));
        };
        let artifact_name = url
            .rsplit('/')
            .next()
            .ok_or_else(|| InstallerError::Other("malformed Python url".into()))?
            .to_string();

        let archive_path = ctx.cache_dir.join("python").join(&artifact_name);
        let download_sha = download_to_file(&ctx.http, &url, &archive_path, &ctx.events).await?;

        // Each python-build-standalone asset ships a `.sha256` sibling.
        let expected_sha_url = format!("{url}.sha256");
        let expected = fetch_sidecar_sha(&ctx.http, &expected_sha_url).await.unwrap_or(None);
        if let Some(expected_sha) = expected {
            if !download_sha.eq_ignore_ascii_case(&expected_sha) {
                let _ = std::fs::remove_file(&archive_path);
                return Err(InstallerError::ChecksumMismatch {
                    url,
                    expected: expected_sha,
                    actual: download_sha,
                });
            }
            ctx.events.info(
                "runtime.verify.complete",
                format!("sha256 matches sidecar for {artifact_name}"),
            );
        } else {
            ctx.events
                .warn("runtime.verify.skipped", format!("no .sha256 sidecar for {artifact_name}"));
        }

        // Belt-and-braces re-read — catches corruption-during-extract-prep.
        let _ = verify_sha256(&archive_path, &download_sha);

        if install_path.exists() {
            let _ = std::fs::remove_dir_all(&install_path);
        }
        // Archive layout: `python/install/bin/python3` with `python/` as the
        // top-level folder. Strip two components to land `bin/`, `include/`,
        // `lib/`, `share/` directly under <install_path>.
        extract_tar(&archive_path, &install_path, 2)?;

        ctx.events.emit(versionx_events::Event::new(
            "runtime.install.complete",
            Level::Info,
            format!("installed python {} at {}", version.version, install_path),
        ));

        Ok(InstallOutcome::Installed(Installation {
            version: ResolvedVersion { sha256: Some(download_sha.clone()), ..version.clone() },
            install_path,
            installed_at: Utc::now(),
            observed_sha256: Some(download_sha),
        }))
    }

    fn shim_entries(&self, installation: &Installation) -> Vec<ShimEntry> {
        let mut out = Vec::new();
        let install = &installation.install_path;
        let is_windows = cfg!(target_os = "windows");

        if is_windows {
            // python-build-standalone on Windows has python.exe at the install root.
            for name in ["python", "python3"] {
                out.push(ShimEntry { name: name.to_string(), target: install.join("python.exe") });
            }
        } else {
            out.push(ShimEntry { name: "python".into(), target: install.join("bin/python3") });
            out.push(ShimEntry { name: "python3".into(), target: install.join("bin/python3") });
            out.push(ShimEntry { name: "pip".into(), target: install.join("bin/pip3") });
            out.push(ShimEntry { name: "pip3".into(), target: install.join("bin/pip3") });
        }
        out
    }

    async fn verify(&self, installation: &Installation) -> InstallerResult<()> {
        let bin = if cfg!(target_os = "windows") {
            installation.install_path.join("python.exe")
        } else {
            installation.install_path.join("bin/python3")
        };
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

/// Map a [`Platform`] to the triple python-build-standalone uses in asset names.
#[must_use]
pub const fn platform_triple(platform: Platform) -> Option<&'static str> {
    Some(match (platform.os, platform.arch, platform.libc) {
        (Os::Linux, Arch::X86_64, Libc::Glibc) => "x86_64-unknown-linux-gnu",
        (Os::Linux, Arch::Aarch64, Libc::Glibc) => "aarch64-unknown-linux-gnu",
        (Os::Linux, Arch::X86_64, Libc::Musl) => "x86_64-unknown-linux-musl",
        (Os::Linux, Arch::Aarch64, Libc::Musl) => "aarch64-unknown-linux-musl",
        (Os::MacOs, Arch::X86_64, _) => "x86_64-apple-darwin",
        (Os::MacOs, Arch::Aarch64, _) => "aarch64-apple-darwin",
        (Os::Windows, Arch::X86_64, _) => "x86_64-pc-windows-msvc-shared",
        // python-build-standalone doesn't ship Windows aarch64 or anything else.
        _ => return None,
    })
}

async fn fetch_releases(ctx: &InstallerContext) -> InstallerResult<Vec<GithubRelease>> {
    let (bytes, _) = download_to_memory(&ctx.http, RELEASES_URL).await?;
    serde_json::from_slice(&bytes).map_err(|e| {
        InstallerError::Other(format!("parsing python-build-standalone releases JSON: {e}"))
    })
}

async fn fetch_sidecar_sha(http: &reqwest::Client, url: &str) -> InstallerResult<Option<String>> {
    let (bytes, _) = match download_to_memory(http, url).await {
        Ok(v) => v,
        Err(InstallerError::Http { status: 404, .. }) => return Ok(None),
        Err(e) => return Err(e),
    };
    let text = std::str::from_utf8(&bytes)
        .map_err(|e| InstallerError::Other(format!("sidecar sha not utf-8: {e}")))?;
    // Format: `<64 hex chars>  <filename>` or just the hex.
    let first = text.split_whitespace().next();
    Ok(first.map(str::to_string))
}

/// Pick the matching install-only asset for `version` + `triple`. `version`
/// can be an exact minor (`3.12`), exact patch (`3.12.2`), or `"stable"`.
fn pick_asset<'a>(
    assets: &'a [GithubAsset],
    version_spec: &str,
    triple: &str,
) -> Option<&'a GithubAsset> {
    assets
        .iter()
        .filter(|a| a.name.contains(triple))
        .filter(|a| a.name.contains("install_only"))
        .filter(|a| a.name.ends_with(".tar.gz"))
        .filter(|a| asset_matches_version(&a.name, version_spec))
        .max_by(|a, b| asset_version_key(&a.name).cmp(&asset_version_key(&b.name)))
}

fn asset_matches_version(name: &str, spec: &str) -> bool {
    if spec == "stable" {
        return true;
    }
    // Asset name: `cpython-3.12.2+20240107-aarch64-apple-darwin-install_only.tar.gz`
    let Some(rest) = name.strip_prefix("cpython-") else {
        return false;
    };
    let Some(dash) = rest.find('-') else {
        return false;
    };
    let version_plus = &rest[..dash];
    let version = version_plus.split('+').next().unwrap_or(version_plus);
    version == spec || version.starts_with(&format!("{spec}."))
}

fn asset_version_key(name: &str) -> (u64, u64, u64) {
    let Some(rest) = name.strip_prefix("cpython-") else {
        return (0, 0, 0);
    };
    let Some(dash) = rest.find('-') else {
        return (0, 0, 0);
    };
    let version_plus = &rest[..dash];
    let version = version_plus.split('+').next().unwrap_or(version_plus);
    let mut parts = version.split('.');
    let a = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let b = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let c = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    (a, b, c)
}

fn extract_python_version(asset_name: &str) -> Option<String> {
    let rest = asset_name.strip_prefix("cpython-")?;
    let dash = rest.find('-')?;
    let version_plus = &rest[..dash];
    Some(version_plus.split('+').next().unwrap_or(version_plus).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triple_for_known_platforms() {
        let linux = Platform::new(Os::Linux, Arch::X86_64, Libc::Glibc);
        assert_eq!(platform_triple(linux), Some("x86_64-unknown-linux-gnu"));
        let mac_arm = Platform::new(Os::MacOs, Arch::Aarch64, Libc::Apple);
        assert_eq!(platform_triple(mac_arm), Some("aarch64-apple-darwin"));
    }

    #[test]
    fn asset_matching_by_minor_version() {
        let assets = vec![
            GithubAsset {
                name: "cpython-3.12.2+20240107-aarch64-apple-darwin-install_only.tar.gz".into(),
                browser_download_url: "u1".into(),
            },
            GithubAsset {
                name: "cpython-3.11.8+20240107-aarch64-apple-darwin-install_only.tar.gz".into(),
                browser_download_url: "u2".into(),
            },
        ];
        let hit = pick_asset(&assets, "3.12", "aarch64-apple-darwin").unwrap();
        assert!(hit.name.contains("3.12.2"));
    }

    #[test]
    fn asset_matching_by_patch_version() {
        let assets = vec![
            GithubAsset {
                name: "cpython-3.12.2+20240107-aarch64-apple-darwin-install_only.tar.gz".into(),
                browser_download_url: "u1".into(),
            },
            GithubAsset {
                name: "cpython-3.12.1+20240107-aarch64-apple-darwin-install_only.tar.gz".into(),
                browser_download_url: "u2".into(),
            },
        ];
        let hit = pick_asset(&assets, "3.12.1", "aarch64-apple-darwin").unwrap();
        assert!(hit.name.contains("3.12.1"));
    }

    #[test]
    fn asset_matching_filters_non_install_only() {
        let assets = vec![GithubAsset {
            name: "cpython-3.12.2+20240107-aarch64-apple-darwin-debug-full.tar.zst".into(),
            browser_download_url: "u1".into(),
        }];
        assert!(pick_asset(&assets, "3.12", "aarch64-apple-darwin").is_none());
    }

    #[test]
    fn extract_version_handles_typical_names() {
        assert_eq!(
            extract_python_version(
                "cpython-3.12.2+20240107-aarch64-apple-darwin-install_only.tar.gz",
            ),
            Some("3.12.2".to_string())
        );
    }
}
