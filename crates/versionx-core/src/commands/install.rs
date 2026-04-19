//! `versionx install <tool> <version>` — global toolchain install.

use camino::Utf8PathBuf;
use serde::Serialize;
use versionx_runtime_trait::{InstallOutcome as InstallerOutcome, VersionSpec};

use super::CoreContext;
use super::shim_install;
use crate::error::{CoreError, CoreResult};

/// Options controlling `install`.
#[derive(Clone, Debug)]
pub struct InstallOptions {
    pub tool: String,
    pub version: String,
    /// When true, skip shim generation (mostly for tests).
    pub skip_shims: bool,
}

/// Structured outcome returned to the CLI.
#[derive(Clone, Debug, Serialize)]
pub struct InstallOutcome {
    pub tool: String,
    pub resolved_version: String,
    pub source: String,
    pub install_path: Utf8PathBuf,
    pub already_installed: bool,
    pub sha256: Option<String>,
    /// Shims refreshed in this run.
    pub shims: Vec<String>,
}

/// Install `tool@version` globally and regenerate shims.
///
/// # Errors
/// - [`CoreError::UnknownRuntime`] if `tool` isn't in the registry.
/// - [`CoreError::Installer`] on any installer-layer failure.
pub async fn install(ctx: &CoreContext, opts: &InstallOptions) -> CoreResult<InstallOutcome> {
    let installer =
        ctx.registry.get(&opts.tool).ok_or_else(|| CoreError::UnknownRuntime(opts.tool.clone()))?;
    let installer_ctx = ctx.installer_ctx();

    ctx.events.info("runtime.resolve.start", format!("resolving {} = {}", opts.tool, opts.version));

    let resolved =
        installer.resolve_version(&VersionSpec::new(&opts.version), &installer_ctx).await?;

    let outcome = installer.install(&resolved, &installer_ctx).await?;
    let install_result = outcome.installation().clone();
    let already = matches!(outcome, InstallerOutcome::AlreadyInstalled(_));

    // Record in the state DB so `versionx runtime list` sees it + the shim
    // can double-check provenance later.
    let state = versionx_state::open(ctx.home.state_db_path())?;
    state.record_runtime(
        &opts.tool,
        &install_result.version.version,
        &install_result.version.source,
        &install_result.install_path,
        install_result.observed_sha256.as_deref(),
    )?;

    // Generate / refresh shims.
    let shim_binary = shim_install::shim_binary_path();
    let shims = if opts.skip_shims {
        Vec::new()
    } else {
        let entries = installer.shim_entries(&install_result);
        shim_install::install_shims(&ctx.home.shims_dir(), &entries, shim_binary.as_deref())?
    };

    Ok(InstallOutcome {
        tool: opts.tool.clone(),
        resolved_version: install_result.version.version.clone(),
        source: install_result.version.source.clone(),
        install_path: install_result.install_path.clone(),
        already_installed: already,
        sha256: install_result.observed_sha256,
        shims,
    })
}
