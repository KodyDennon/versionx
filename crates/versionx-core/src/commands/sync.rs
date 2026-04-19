//! `versionx sync` — install everything declared in `versionx.toml`.

use camino::Utf8PathBuf;
use serde::Serialize;
use versionx_lockfile::{LockedRuntime, Lockfile, blake3_hex};
use versionx_runtime_trait::{InstallOutcome as InstallerOutcome, VersionSpec};

use super::{CoreContext, shim_install};
use crate::error::{CoreError, CoreResult};

/// Options controlling `sync`.
#[derive(Clone, Debug)]
pub struct SyncOptions {
    pub root: Utf8PathBuf,
    /// Compute the plan and write the lockfile but skip actual install work.
    pub dry_run: bool,
}

/// Structured outcome returned to the CLI.
#[derive(Clone, Debug, Serialize)]
pub struct SyncOutcome {
    pub config_hash: String,
    pub lockfile_path: Utf8PathBuf,
    pub installed: Vec<InstalledRuntime>,
    pub shims: Vec<String>,
    pub skipped: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct InstalledRuntime {
    pub tool: String,
    pub version: String,
    pub source: String,
    pub already_installed: bool,
}

/// Run a sync from `opts.root` using the ctx's registry + state.
///
/// # Errors
/// - [`CoreError::NoConfig`] if no `versionx.toml` at the root.
/// - [`CoreError::UnknownRuntime`] if `[runtimes]` names a tool we can't install.
/// - [`CoreError::Installer`] on any installer-layer failure.
pub async fn sync(ctx: &CoreContext, opts: &SyncOptions) -> CoreResult<SyncOutcome> {
    let config_path = opts.root.join("versionx.toml");
    if !config_path.exists() {
        return Err(CoreError::NoConfig { path: config_path.to_string() });
    }
    let effective = versionx_config::load(&config_path)?;
    // Hash file *content* so sync detects config drift regardless of where
    // the workspace lives on disk (and so CI caching can key on it).
    let config_bytes = std::fs::read(&config_path)
        .map_err(|source| CoreError::Io { path: config_path.to_string(), source })?;
    let config_hash = blake3_hex(&config_bytes);

    let installer_ctx = ctx.installer_ctx();
    let mut installed = Vec::new();
    let mut skipped = Vec::new();
    let mut lock = Lockfile::new(config_hash.clone());

    for (tool, spec) in &effective.config.runtimes.tools {
        let Some(installer) = ctx.registry.get(tool) else {
            skipped.push(format!("{tool} (no installer)"));
            continue;
        };

        let resolved =
            installer.resolve_version(&VersionSpec::new(spec.version()), &installer_ctx).await?;

        let (already, install_info) = if opts.dry_run {
            // Dry-run never touches disk; we still record the resolved
            // version in the lockfile so users can diff what WOULD change.
            let path = installer.install_path(&resolved, &installer_ctx);
            (
                installer.is_installed(&resolved, &installer_ctx).await,
                (resolved.clone(), path, None),
            )
        } else {
            let outcome = installer.install(&resolved, &installer_ctx).await?;
            let install = outcome.installation().clone();
            let already = matches!(outcome, InstallerOutcome::AlreadyInstalled(_));
            (
                already,
                (install.version.clone(), install.install_path.clone(), install.observed_sha256),
            )
        };

        let (version, install_path, sha) = install_info;

        // State + shims (only when we actually touched disk).
        if !opts.dry_run {
            let state = versionx_state::open(ctx.home.state_db_path())?;
            state.record_runtime(
                tool,
                &version.version,
                &version.source,
                &install_path,
                sha.as_deref(),
            )?;
        }

        lock.runtimes.insert(
            tool.clone(),
            LockedRuntime {
                version: version.version.clone(),
                source: version.source.clone(),
                sha256: sha.clone(),
                install_path: Some(install_path.clone()),
            },
        );

        installed.push(InstalledRuntime {
            tool: tool.clone(),
            version: version.version.clone(),
            source: version.source.clone(),
            already_installed: already,
        });
    }

    // Shims: one pass at the end, covering everything we just installed.
    let mut all_shims = Vec::new();
    if !opts.dry_run {
        let shim_binary = shim_install::shim_binary_path();
        for (tool, spec) in &effective.config.runtimes.tools {
            let Some(installer) = ctx.registry.get(tool) else {
                continue;
            };
            let resolved = installer
                .resolve_version(&VersionSpec::new(spec.version()), &installer_ctx)
                .await?;
            if !installer.is_installed(&resolved, &installer_ctx).await {
                continue;
            }
            let path = installer.install_path(&resolved, &installer_ctx);
            let install = versionx_runtime_trait::Installation {
                version: resolved,
                install_path: path,
                installed_at: chrono::Utc::now(),
                observed_sha256: None,
            };
            let entries = installer.shim_entries(&install);
            let created = shim_install::install_shims(
                &ctx.home.shims_dir(),
                &entries,
                shim_binary.as_deref(),
            )?;
            all_shims.extend(created);
        }
    }

    // Write the lockfile even on dry-run so users can diff.
    let lockfile_path = opts.root.join("versionx.lock");
    lock.save(&lockfile_path)?;

    // Record the sync in the state DB.
    if !opts.dry_run {
        let state = versionx_state::open(ctx.home.state_db_path())?;
        let repo = state.upsert_repo(&opts.root, None)?;
        state.mark_repo_synced(repo.id, &config_hash)?;
    }

    Ok(SyncOutcome { config_hash, lockfile_path, installed, shims: all_shims, skipped })
}
