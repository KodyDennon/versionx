//! Node.js runtime installer.
//!
//! Source: official releases from `https://nodejs.org/dist/`.
//!
//! Resolution strategy:
//! 1. If the spec is an exact version (`"22.12.0"`), fetch
//!    `/dist/index.json` once and confirm it exists.
//! 2. If it's a major (`"20"`) or caret (`"^20.11"`), pick the highest
//!    matching version in the index.
//! 3. If it's `"lts"`, pick the latest LTS across all majors.
//! 4. If it's `"lts/<codename>"`, pick the latest LTS with that codename.
//!
//! Archive selection uses [`Platform`] to pick `linux-x64`, `darwin-arm64`,
//! `win-x64`, etc.

#![deny(unsafe_code)]

pub mod pm;

use async_trait::async_trait;
use camino::Utf8PathBuf;
use chrono::Utc;
use semver::Version;
use serde::Deserialize;
use versionx_events::{EventSender, Level};
use versionx_runtime_trait::{
    Arch, InstallOutcome, Installation, InstallerContext, InstallerError, InstallerResult, Libc,
    Os, Platform, ResolvedVersion, RuntimeInstaller, ShimEntry, VersionSpec,
    download::{download_to_file, download_to_memory, extract_tar, extract_zip, verify_sha256},
};

pub use pm::{PnpmInstaller, YarnInstaller};

/// Default index URL. Overridable via the `NODEJS_MIRROR` env var.
pub const DEFAULT_INDEX_URL: &str = "https://nodejs.org/dist/index.json";

#[derive(Debug, Default)]
pub struct NodeInstaller;

impl NodeInstaller {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

#[derive(Clone, Debug, Deserialize)]
struct NodeIndexEntry {
    /// Leading `"v"` is included in the API (`"v22.12.0"`).
    version: String,
    /// Enum of file suffixes available for this release.
    #[allow(dead_code)] // Kept for future platform-availability filtering.
    files: Vec<String>,
    /// LTS codename (`"Iron"`) or `false` when not an LTS release.
    #[serde(default)]
    lts: Lts,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
enum Lts {
    Flag(bool),
    Codename(String),
}

impl Default for Lts {
    fn default() -> Self {
        Self::Flag(false)
    }
}

impl Lts {
    const fn is_lts(&self) -> bool {
        !matches!(self, Self::Flag(false))
    }
    fn codename(&self) -> Option<&str> {
        match self {
            Self::Codename(s) => Some(s),
            Self::Flag(_) => None,
        }
    }
}

#[async_trait]
impl RuntimeInstaller for NodeInstaller {
    fn id(&self) -> &'static str {
        "node"
    }
    fn display_name(&self) -> &'static str {
        "Node.js"
    }

    async fn resolve_version(
        &self,
        spec: &VersionSpec,
        ctx: &InstallerContext,
    ) -> InstallerResult<ResolvedVersion> {
        let raw_index = fetch_index_cached(ctx).await?;

        let spec_str = spec.as_str().trim();
        let pick = resolve_pick(&raw_index, spec_str).ok_or_else(|| {
            InstallerError::UnresolvableVersion { tool: "node", spec: spec.0.clone() }
        })?;

        let version = pick.version.trim_start_matches('v').to_string();
        let artifact = Self::artifact_name(&version, ctx.platform).ok_or_else(|| {
            InstallerError::UnsupportedPlatform { tool: "node", platform: ctx.platform.to_string() }
        })?;

        let base =
            std::env::var("NODEJS_MIRROR").ok().unwrap_or_else(|| "https://nodejs.org/dist".into());
        let url = format!("{base}/v{version}/{artifact}");

        Ok(ResolvedVersion {
            version,
            channel: pick
                .lts
                .codename()
                .map(ToString::to_string)
                .or_else(|| pick.lts.is_lts().then_some("lts".to_string())),
            source: "nodejs.org".into(),
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
            return Err(InstallerError::Other(
                "Node ResolvedVersion missing url — call resolve_version first".into(),
            ));
        };
        let artifact_name = url
            .rsplit('/')
            .next()
            .ok_or_else(|| InstallerError::Other("malformed Node url".into()))?
            .to_string();

        let archive_path = ctx.cache_dir.join("node").join(&artifact_name);
        let download_sha = if archive_path.exists() {
            // Re-use the cached download — re-verify.
            verify_sha256(&archive_path, "0000").unwrap_or_else(|e| match e {
                InstallerError::ChecksumMismatch { actual, .. } => actual,
                _ => String::new(),
            })
        } else {
            download_to_file(&ctx.http, &url, &archive_path, &ctx.events).await?
        };

        // Fetch + verify against the official SHASUMS256.txt for this release.
        if let Some(expected) =
            fetch_expected_sha256(&ctx.http, &url, &artifact_name, &ctx.events).await?
        {
            if !download_sha.eq_ignore_ascii_case(&expected) {
                // Remove the bad download so a rerun refetches.
                let _ = std::fs::remove_file(&archive_path);
                return Err(InstallerError::ChecksumMismatch {
                    url,
                    expected,
                    actual: download_sha,
                });
            }
            ctx.events.info(
                "runtime.verify.complete",
                format!("sha256 matches SHASUMS256.txt for {artifact_name}"),
            );
        } else {
            ctx.events.warn(
                "runtime.verify.skipped",
                format!(
                    "no SHASUMS256.txt entry for {artifact_name} — proceeding with observed sha256"
                ),
            );
        }

        // Install into `<runtimes_dir>/node/<version>/`. Extraction strips the
        // `node-v<ver>-<platform>/` root.
        if install_path.exists() {
            let _ = std::fs::remove_dir_all(&install_path);
        }
        if artifact_name.ends_with(".zip") {
            extract_zip(&archive_path, &install_path, 1)?;
        } else {
            extract_tar(&archive_path, &install_path, 1)?;
        }

        ctx.events.emit(versionx_events::Event::new(
            "runtime.install.complete",
            Level::Info,
            format!("installed node {} at {}", version.version, install_path),
        ));

        Ok(InstallOutcome::Installed(Installation {
            version: ResolvedVersion { sha256: Some(download_sha.clone()), ..version.clone() },
            install_path,
            installed_at: Utc::now(),
            observed_sha256: Some(download_sha),
        }))
    }

    fn shim_entries(&self, installation: &Installation) -> Vec<ShimEntry> {
        let binaries = ["node", "npm", "npx"];
        let mut out = Vec::with_capacity(binaries.len());
        for name in binaries {
            let target = Self::binary_path(&installation.install_path, name, ctx_platform_hint());
            out.push(ShimEntry { name: name.to_string(), target });
        }
        out
    }

    async fn verify(&self, installation: &Installation) -> InstallerResult<()> {
        // Smoke-test: invoke `node --version` and confirm it matches.
        let platform = ctx_platform_hint();
        let bin = Self::binary_path(&installation.install_path, "node", platform);
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
        let reported = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let expected = format!("v{}", installation.version.version);
        if reported != expected {
            return Err(InstallerError::Other(format!(
                "node --version reported `{reported}`, expected `{expected}`"
            )));
        }
        Ok(())
    }
}

impl NodeInstaller {
    /// Build the archive filename for a given version + platform, or `None`
    /// if the combo isn't supported.
    #[must_use]
    pub fn artifact_name(version: &str, platform: Platform) -> Option<String> {
        let (os_part, arch_part, ext) = match (platform.os, platform.arch, platform.libc) {
            (Os::Linux, Arch::X86_64, Libc::Glibc) => ("linux", "x64", "tar.xz"),
            (Os::Linux, Arch::Aarch64, Libc::Glibc) => ("linux", "arm64", "tar.xz"),
            (Os::Linux, Arch::X86_64, Libc::Musl) => ("linux", "x64-musl", "tar.xz"),
            (Os::Linux, Arch::Aarch64, Libc::Musl) => ("linux", "arm64-musl", "tar.xz"),
            (Os::MacOs, Arch::X86_64, _) => ("darwin", "x64", "tar.xz"),
            (Os::MacOs, Arch::Aarch64, _) => ("darwin", "arm64", "tar.xz"),
            (Os::Windows, Arch::X86_64, _) => ("win", "x64", "zip"),
            (Os::Windows, Arch::Aarch64, _) => ("win", "arm64", "zip"),
            _ => return None,
        };
        Some(format!("node-v{version}-{os_part}-{arch_part}.{ext}"))
    }

    /// Path to a binary in a Node install, accounting for Windows layout
    /// (binaries at the root, not under `bin/`).
    #[must_use]
    pub fn binary_path(install: &camino::Utf8Path, name: &str, platform: Platform) -> Utf8PathBuf {
        match platform.os {
            Os::Windows => {
                let exe = if matches!(name, "node") {
                    "node.exe"
                } else {
                    // npm/npx ship as `.cmd` on Windows plus a `node_modules/` dir.
                    return install.join(format!("{name}.cmd"));
                };
                install.join(exe)
            }
            _ => install.join("bin").join(name),
        }
    }
}

// --- private helpers -------------------------------------------------------

fn ctx_platform_hint() -> Platform {
    Platform::detect()
}

/// Simple on-disk cache for the index. Stored at
/// `$CACHE/node/index.json` with a `mtime < 1 hour` freshness check.
async fn fetch_index_cached(ctx: &InstallerContext) -> InstallerResult<Vec<NodeIndexEntry>> {
    let cache_path = ctx.cache_dir.join("node").join("index.json");
    let fresh = cache_path
        .metadata()
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|mtime| mtime.elapsed().ok())
        .is_some_and(|age| age.as_secs() < 3600);

    if fresh
        && let Ok(raw) = std::fs::read(&cache_path)
        && let Ok(parsed) = serde_json::from_slice::<Vec<NodeIndexEntry>>(&raw)
    {
        return Ok(parsed);
    }

    let url = std::env::var("NODEJS_INDEX_URL").ok().unwrap_or_else(|| DEFAULT_INDEX_URL.into());
    ctx.events.info("runtime.resolve.fetch", format!("fetching {url}"));
    let (bytes, _) = download_to_memory(&ctx.http, &url).await?;

    if let Some(parent) = cache_path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    let _ = tokio::fs::write(&cache_path, &bytes).await;

    serde_json::from_slice::<Vec<NodeIndexEntry>>(&bytes)
        .map_err(|e| InstallerError::Other(format!("parsing Node index.json: {e}")))
}

/// Dispatch entry: pick by `lts`, `lts/<codename>`, `stable`, or version.
#[allow(clippy::option_if_let_else)] // The match-arm shape reads more clearly than nested map_or.
fn resolve_pick<'a>(index: &'a [NodeIndexEntry], spec_str: &str) -> Option<&'a NodeIndexEntry> {
    if let Some(codename) = spec_str.strip_prefix("lts/") {
        pick_lts(index, Some(codename))
    } else if spec_str == "lts" {
        pick_lts(index, None)
    } else if spec_str == "stable" || spec_str == "current" {
        pick_stable(index)
    } else {
        pick_by_version(index, spec_str)
    }
}

/// Pick the entry matching an exact or semver-prefix spec.
fn pick_by_version<'a>(index: &'a [NodeIndexEntry], spec: &str) -> Option<&'a NodeIndexEntry> {
    let target = spec.trim_start_matches('v');

    // Exact match first (matches full `22.12.0` — Node index is ordered newest-first).
    if let Some(hit) = index.iter().find(|e| e.version.trim_start_matches('v') == target) {
        return Some(hit);
    }

    // Prefix match: `"20"` → highest 20.x.y; `"20.11"` → highest 20.11.y.
    let prefix_bits: Vec<&str> = target.split('.').collect();
    let mut candidates: Vec<(Version, &NodeIndexEntry)> = index
        .iter()
        .filter_map(|e| {
            let v = e.version.trim_start_matches('v');
            Version::parse(v).ok().map(|ver| (ver, e))
        })
        .filter(|(v, _)| version_matches_prefix(v, &prefix_bits))
        .collect();
    candidates.sort_by(|a, b| b.0.cmp(&a.0));
    candidates.first().map(|(_, e)| *e)
}

fn version_matches_prefix(v: &Version, bits: &[&str]) -> bool {
    match bits {
        [maj] => maj.parse::<u64>().is_ok_and(|m| v.major == m),
        [maj, min] => {
            maj.parse::<u64>().is_ok_and(|m| v.major == m)
                && min.parse::<u64>().is_ok_and(|mn| v.minor == mn)
        }
        _ => false,
    }
}

fn pick_lts<'a>(index: &'a [NodeIndexEntry], codename: Option<&str>) -> Option<&'a NodeIndexEntry> {
    index.iter().find(|e| match (&e.lts, codename) {
        (Lts::Codename(c), Some(want)) => c.eq_ignore_ascii_case(want),
        (Lts::Codename(_), None) => true,
        _ => false,
    })
}

const fn pick_stable(index: &[NodeIndexEntry]) -> Option<&NodeIndexEntry> {
    index.first()
}

/// Fetch the SHASUMS256.txt for this version and find the entry for `artifact_name`.
async fn fetch_expected_sha256(
    http: &reqwest::Client,
    archive_url: &str,
    artifact_name: &str,
    events: &EventSender,
) -> InstallerResult<Option<String>> {
    let shasums_url = archive_url
        .rsplit_once('/')
        .map_or_else(String::new, |(base, _)| format!("{base}/SHASUMS256.txt"));
    if shasums_url.is_empty() {
        return Ok(None);
    }
    events.info("runtime.verify.fetch", format!("fetching {shasums_url}"));
    let (bytes, _) = download_to_memory(http, &shasums_url).await?;
    let text = std::str::from_utf8(&bytes).map_err(|e| InstallerError::Other(e.to_string()))?;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let Some(hash) = parts.next() else { continue };
        let Some(file) = parts.next() else { continue };
        if file == artifact_name {
            return Ok(Some(hash.to_string()));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(v: &str, lts: Lts) -> NodeIndexEntry {
        NodeIndexEntry { version: v.into(), files: vec!["linux-x64".into()], lts }
    }

    #[test]
    fn artifact_name_covers_common_platforms() {
        let linux = Platform::new(Os::Linux, Arch::X86_64, Libc::Glibc);
        assert_eq!(
            NodeInstaller::artifact_name("22.12.0", linux).as_deref(),
            Some("node-v22.12.0-linux-x64.tar.xz")
        );
        let mac_arm = Platform::new(Os::MacOs, Arch::Aarch64, Libc::Apple);
        assert_eq!(
            NodeInstaller::artifact_name("22.12.0", mac_arm).as_deref(),
            Some("node-v22.12.0-darwin-arm64.tar.xz")
        );
        let win = Platform::new(Os::Windows, Arch::X86_64, Libc::Msvc);
        assert_eq!(
            NodeInstaller::artifact_name("22.12.0", win).as_deref(),
            Some("node-v22.12.0-win-x64.zip")
        );
    }

    #[test]
    fn pick_by_exact_version() {
        let idx = vec![
            entry("v22.1.0", Lts::Flag(false)),
            entry("v22.12.0", Lts::Codename("Iron".into())),
            entry("v20.11.0", Lts::Codename("Iron".into())),
            entry("v18.19.0", Lts::Codename("Hydrogen".into())),
        ];
        let hit = pick_by_version(&idx, "22.12.0").unwrap();
        assert_eq!(hit.version, "v22.12.0");
    }

    #[test]
    fn pick_by_major_selects_highest_in_line() {
        let idx = vec![
            entry("v22.1.0", Lts::Flag(false)),
            entry("v22.12.0", Lts::Codename("Iron".into())),
            entry("v20.11.0", Lts::Codename("Iron".into())),
        ];
        let hit = pick_by_version(&idx, "22").unwrap();
        assert_eq!(hit.version, "v22.12.0");
    }

    #[test]
    fn lts_picks_newest_lts() {
        let idx = vec![
            entry("v22.1.0", Lts::Flag(false)),
            entry("v22.12.0", Lts::Codename("Iron".into())),
            entry("v18.19.0", Lts::Codename("Hydrogen".into())),
        ];
        let hit = pick_lts(&idx, None).unwrap();
        assert_eq!(hit.version, "v22.12.0");
    }

    #[test]
    fn lts_codename_picks_matching_codename() {
        let idx = vec![
            entry("v22.1.0", Lts::Flag(false)),
            entry("v22.12.0", Lts::Codename("Iron".into())),
            entry("v18.19.0", Lts::Codename("Hydrogen".into())),
        ];
        let hit = pick_lts(&idx, Some("hydrogen")).unwrap();
        assert_eq!(hit.version, "v18.19.0");
    }

    #[test]
    fn stable_picks_head() {
        let idx = vec![
            entry("v22.1.0", Lts::Flag(false)),
            entry("v22.12.0", Lts::Codename("Iron".into())),
        ];
        let hit = pick_stable(&idx).unwrap();
        assert_eq!(hit.version, "v22.1.0");
    }
}
