//! `versionx sync` — install everything declared in `versionx.toml`.

use camino::Utf8PathBuf;
use serde::Serialize;
use versionx_adapter_trait::{AdapterContext, Intent};
use versionx_lockfile::{LockedEcosystem, LockedRuntime, Lockfile, blake3_hex};
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
    pub ecosystems: Vec<EcosystemSync>,
}

#[derive(Clone, Debug, Serialize)]
pub struct EcosystemSync {
    pub ecosystem: String,
    pub package_manager: Option<String>,
    pub skipped_reason: Option<String>,
    pub step_preview: Option<String>,
    pub duration_ms: u64,
    pub exit_code: Option<i32>,
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
#[allow(clippy::too_many_lines)] // Orchestrator — breaking it up obscures the linear flow.
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

    // Run each ecosystem adapter after runtimes are in place. Skips adapters
    // that say `applicable = false` (e.g. no package.json in the repo).
    let mut ecosystems = Vec::new();
    for (eco_id, _eco_cfg) in &effective.config.ecosystems {
        let Some(adapter) = ctx.adapter_registry.get(eco_id) else {
            ecosystems.push(EcosystemSync {
                ecosystem: eco_id.clone(),
                package_manager: None,
                skipped_reason: Some("no adapter for this ecosystem in 0.1.0".into()),
                step_preview: None,
                duration_ms: 0,
                exit_code: None,
            });
            continue;
        };

        let runtime_bin_dir = runtime_bin_dir_for(ctx, eco_id);
        let adapter_ctx = AdapterContext {
            cwd: opts.root.clone(),
            runtime_bin_dir,
            events: ctx.events.clone(),
            env: Vec::new(),
            dry_run: opts.dry_run,
        };

        let detect = adapter.detect(&adapter_ctx).await?;
        if !detect.applicable {
            ecosystems.push(EcosystemSync {
                ecosystem: eco_id.clone(),
                package_manager: None,
                skipped_reason: Some("adapter reported not-applicable at this cwd".into()),
                step_preview: None,
                duration_ms: 0,
                exit_code: None,
            });
            continue;
        }

        let intent = Intent::Sync;
        let plan = adapter.plan(&adapter_ctx, &intent).await?;
        let Some(first_step) = plan.steps.first().cloned() else {
            ecosystems.push(EcosystemSync {
                ecosystem: eco_id.clone(),
                package_manager: detect.package_manager.clone(),
                skipped_reason: Some("empty plan".into()),
                step_preview: None,
                duration_ms: 0,
                exit_code: None,
            });
            continue;
        };

        let outcome = adapter.execute(&adapter_ctx, &first_step, &intent).await?;
        ecosystems.push(EcosystemSync {
            ecosystem: eco_id.clone(),
            package_manager: detect.package_manager.clone(),
            skipped_reason: None,
            step_preview: Some(first_step.command_preview.clone()),
            duration_ms: outcome.duration_ms,
            exit_code: outcome.exit_code,
        });

        // Record the ecosystem in the lockfile.
        lock.ecosystems.insert(
            eco_id.clone(),
            LockedEcosystem {
                package_manager: detect.package_manager.unwrap_or_default(),
                native_lockfile: detect
                    .manifest_path
                    .as_ref()
                    .and_then(|p| p.file_name())
                    .map(ToString::to_string),
                native_lockfile_hash: None,
                resolved_at: Some(chrono::Utc::now()),
            },
        );
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

    Ok(SyncOutcome { config_hash, lockfile_path, installed, shims: all_shims, skipped, ecosystems })
}

/// Locate the bin dir of the pinned package manager for a given ecosystem.
/// Returns `None` if the PM isn't a Versionx-managed runtime (the adapter
/// will fall back to PATH in that case).
fn runtime_bin_dir_for(ctx: &CoreContext, ecosystem: &str) -> Option<Utf8PathBuf> {
    // For Node we want the PM's bin dir on PATH when we spawn it — but
    // also Node's bin/ (pnpm on certain systems shells out to node).
    // First approximation: prepend the shims dir, since that's where all
    // of our installed PMs + their underlying runtimes resolve.
    match ecosystem {
        "node" | "python" | "rust" => Some(ctx.home.shims_dir()),
        _ => None,
    }
}
