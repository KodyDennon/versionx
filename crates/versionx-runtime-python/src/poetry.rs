//! Poetry runtime installer.
//!
//! Approach: install Poetry into an isolated virtualenv on top of an
//! already-resolved Python interpreter. This mirrors what `pipx`
//! does and is the official Poetry-recommended install method when
//! you want a specific version pinned.
//!
//! The Python used for the venv is the host's `python3` (looked up
//! via `which`). For Versionx-managed Python, callers should sync
//! Python first and ensure its shim is on PATH.
//!
//! The install path layout is:
//!   `<install_path>/venv/bin/poetry`        (Unix)
//!   `<install_path>\venv\Scripts\poetry.exe` (Windows)
//!
//! Verification runs `poetry --version`.

use async_trait::async_trait;
use chrono::Utc;
use serde::Deserialize;
use versionx_events::Level;
use versionx_runtime_trait::{
    InstallOutcome, Installation, InstallerContext, InstallerError, InstallerResult,
    ResolvedVersion, RuntimeInstaller, ShimEntry, VersionSpec, download::download_to_memory,
};

#[derive(Debug, Default)]
pub struct PoetryInstaller;

impl PoetryInstaller {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

#[async_trait]
impl RuntimeInstaller for PoetryInstaller {
    fn id(&self) -> &'static str {
        "poetry"
    }
    fn display_name(&self) -> &'static str {
        "Poetry (python-poetry/poetry)"
    }

    async fn resolve_version(
        &self,
        spec: &VersionSpec,
        ctx: &InstallerContext,
    ) -> InstallerResult<ResolvedVersion> {
        let version = resolve_poetry_version(&ctx.http, spec.as_str()).await?;
        Ok(ResolvedVersion {
            version,
            channel: None,
            source: "pip:poetry".into(),
            sha256: None,
            url: None, // pip handles fetching from PyPI
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

        // Locate a host python — prefer python3, fall back to python.
        let py = which_python().ok_or_else(|| {
            InstallerError::Other(
                "Poetry install requires `python3` on PATH (sync Python first).".into(),
            )
        })?;

        if install_path.exists() {
            let _ = std::fs::remove_dir_all(&install_path);
        }
        std::fs::create_dir_all(install_path.as_std_path())
            .map_err(|source| InstallerError::Io { path: install_path.clone(), source })?;

        let venv_path = install_path.join("venv");
        // 1. Create the venv.
        let mk_venv = tokio::process::Command::new(&py)
            .args(["-m", "venv", venv_path.as_str()])
            .output()
            .await
            .map_err(|source| InstallerError::Io {
                path: camino::Utf8PathBuf::from(py.to_string_lossy().to_string()),
                source,
            })?;
        if !mk_venv.status.success() {
            return Err(InstallerError::Subprocess {
                program: py.to_string_lossy().into_owned(),
                status: mk_venv.status.code().unwrap_or(-1),
                stderr: String::from_utf8_lossy(&mk_venv.stderr).into_owned(),
            });
        }

        // 2. pip install poetry==<version> into the venv.
        let pip = if cfg!(target_os = "windows") {
            venv_path.join("Scripts/pip.exe")
        } else {
            venv_path.join("bin/pip")
        };
        let installed = tokio::process::Command::new(pip.as_std_path())
            .args(["install", "--no-input", &format!("poetry=={}", version.version)])
            .output()
            .await
            .map_err(|source| InstallerError::Io { path: pip.clone(), source })?;
        if !installed.status.success() {
            return Err(InstallerError::Subprocess {
                program: pip.to_string(),
                status: installed.status.code().unwrap_or(-1),
                stderr: String::from_utf8_lossy(&installed.stderr).into_owned(),
            });
        }

        ctx.events.emit(versionx_events::Event::new(
            "runtime.install.complete",
            Level::Info,
            format!("installed poetry {} at {}", version.version, install_path),
        ));

        Ok(InstallOutcome::Installed(Installation {
            version: version.clone(),
            install_path,
            installed_at: Utc::now(),
            observed_sha256: None,
        }))
    }

    fn shim_entries(&self, installation: &Installation) -> Vec<ShimEntry> {
        let target = if cfg!(target_os = "windows") {
            installation.install_path.join("venv/Scripts/poetry.exe")
        } else {
            installation.install_path.join("venv/bin/poetry")
        };
        vec![ShimEntry { name: "poetry".into(), target }]
    }

    async fn verify(&self, installation: &Installation) -> InstallerResult<()> {
        let bin = if cfg!(target_os = "windows") {
            installation.install_path.join("venv/Scripts/poetry.exe")
        } else {
            installation.install_path.join("venv/bin/poetry")
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

fn which_python() -> Option<std::path::PathBuf> {
    which::which("python3").or_else(|_| which::which("python")).ok()
}

#[derive(Deserialize)]
struct PypiPackage {
    info: PypiInfo,
    releases: std::collections::BTreeMap<String, serde_json::Value>,
}

#[derive(Deserialize)]
struct PypiInfo {
    version: String,
}

/// Resolve a Poetry version spec against `PyPI`. Accepts:
///   - exact: `1.8.3`
///   - prefix: `1.8`
///   - alias: `latest` / `stable`
async fn resolve_poetry_version(http: &reqwest::Client, spec: &str) -> InstallerResult<String> {
    let bare = spec.trim().trim_start_matches('v');
    if looks_exact(bare) {
        return Ok(bare.to_string());
    }
    let url = "https://pypi.org/pypi/poetry/json";
    let (bytes, _) = download_to_memory(http, url).await?;
    let pkg: PypiPackage = serde_json::from_slice(&bytes)
        .map_err(|e| InstallerError::Other(format!("parsing poetry PyPI metadata: {e}")))?;
    if bare == "latest" || bare == "stable" || bare == "current" {
        return Ok(pkg.info.version);
    }
    // Prefix match — pick the highest release whose version starts
    // with `bare.`.
    let mut best: Option<semver::Version> = None;
    for v in pkg.releases.keys() {
        if !v.starts_with(&format!("{bare}.")) && v != bare {
            continue;
        }
        if let Ok(parsed) = semver::Version::parse(v) {
            match &best {
                Some(cur) if cur >= &parsed => {}
                _ => best = Some(parsed),
            }
        }
    }
    best.map(|v| v.to_string()).ok_or_else(|| InstallerError::UnresolvableVersion {
        tool: "poetry",
        spec: spec.to_string(),
    })
}

fn looks_exact(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    parts.len() == 3 && parts.iter().all(|p| p.parse::<u32>().is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn looks_exact_strict() {
        assert!(looks_exact("1.8.3"));
        assert!(!looks_exact("1.8"));
        assert!(!looks_exact("latest"));
    }
}
