//! Registry publish — npm + crates.io for now.
//!
//! Strategy:
//!
//! 1. Detect which ecosystem the component lives in by manifest
//!    file: `package.json` → npm, `Cargo.toml` → crates.io.
//! 2. Choose credentials in this order: GitHub Actions OIDC env vars
//!    (`ACTIONS_ID_TOKEN_REQUEST_TOKEN` +
//!    `ACTIONS_ID_TOKEN_REQUEST_URL`) trump everything else; if those
//!    are absent we fall back to static `NPM_TOKEN` /
//!    `CARGO_REGISTRY_TOKEN`. With neither, bail with a clear error.
//! 3. Shell out to `npm publish` / `cargo publish` with sensible
//!    flags (`--access public` for npm, `--allow-dirty` deliberately
//!    omitted for cargo).
//!
//! Returns a structured [`PublishOutcome`] so the saga can record it.
//!
//! No sigstore / cosign attestation yet — that's the
//! `provenance_required` policy's job.

use std::process::Command;

use camino::{Utf8Path, Utf8PathBuf};
use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum PublishError {
    #[error("no recognized manifest under {root}")]
    NoManifest { root: Utf8PathBuf },
    #[error(
        "no credentials available for {registry}: set {env} or run inside a GitHub Actions OIDC-enabled job"
    )]
    MissingCredentials { registry: &'static str, env: &'static str },
    #[error("publish command failed for {registry} at {root}: exit {code} stderr {stderr}")]
    CommandFailed { registry: &'static str, root: Utf8PathBuf, code: i32, stderr: String },
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub type PublishResult<T> = Result<T, PublishError>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Registry {
    Npm,
    Crates,
}

impl Registry {
    fn label(self) -> &'static str {
        match self {
            Self::Npm => "npm",
            Self::Crates => "crates.io",
        }
    }

    fn token_env(self) -> &'static str {
        match self {
            Self::Npm => "NPM_TOKEN",
            Self::Crates => "CARGO_REGISTRY_TOKEN",
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct PublishOutcome {
    pub registry: Registry,
    pub component_root: Utf8PathBuf,
    pub used_oidc: bool,
}

/// Detect the ecosystem at `component_root` and run the right
/// `<tool> publish`. Skips silently with `Ok(None)` when no recognized
/// manifest exists — that lets the saga fan out across mixed-ecosystem
/// fleets without error.
pub fn publish(component_root: &Utf8Path) -> PublishResult<Option<PublishOutcome>> {
    let registry = detect_registry(component_root);
    let Some(registry) = registry else {
        return Ok(None);
    };
    let used_oidc = oidc_available();
    if !used_oidc && std::env::var(registry.token_env()).is_err() {
        return Err(PublishError::MissingCredentials {
            registry: registry.label(),
            env: registry.token_env(),
        });
    }

    let (program, args): (&str, Vec<&str>) = match registry {
        Registry::Npm => ("npm", vec!["publish", "--access", "public"]),
        Registry::Crates => ("cargo", vec!["publish"]),
    };
    let output =
        Command::new(program).args(&args).current_dir(component_root.as_std_path()).output()?;
    if !output.status.success() {
        return Err(PublishError::CommandFailed {
            registry: registry.label(),
            root: component_root.to_path_buf(),
            code: output.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }

    Ok(Some(PublishOutcome { registry, component_root: component_root.to_path_buf(), used_oidc }))
}

fn detect_registry(root: &Utf8Path) -> Option<Registry> {
    if root.join("package.json").is_file() {
        return Some(Registry::Npm);
    }
    if root.join("Cargo.toml").is_file() {
        return Some(Registry::Crates);
    }
    None
}

/// True when GitHub Actions trusted-publisher OIDC env vars are set.
/// Both `npm publish` (since npm 9.5) and `cargo publish` (via
/// `cargo-credential-gcm` / `cargo-trusted-publish`) honor these.
#[must_use]
pub fn oidc_available() -> bool {
    std::env::var("ACTIONS_ID_TOKEN_REQUEST_TOKEN").is_ok()
        && std::env::var("ACTIONS_ID_TOKEN_REQUEST_URL").is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_npm_via_package_json() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        std::fs::write(root.join("package.json"), "{}").unwrap();
        assert_eq!(detect_registry(&root), Some(Registry::Npm));
    }

    #[test]
    fn detects_crates_via_cargo_toml() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        std::fs::write(root.join("Cargo.toml"), "[package]\nname=\"x\"\nversion=\"0.1.0\"\n")
            .unwrap();
        assert_eq!(detect_registry(&root), Some(Registry::Crates));
    }

    #[test]
    fn detects_none_for_unknown_root() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        assert_eq!(detect_registry(&root), None);
    }

    #[test]
    fn publish_returns_none_for_unknown_root() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        let out = publish(&root).unwrap();
        assert!(out.is_none());
    }
}
