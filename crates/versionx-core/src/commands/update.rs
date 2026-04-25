//! `versionx update` — update ecosystem dependencies and refresh `versionx.lock`.

use camino::Utf8PathBuf;
use serde::Serialize;
use versionx_adapter_trait::{AdapterContext, Intent};
use versionx_lockfile::{Lockfile, LockedEcosystem, LockedRuntime, blake3_hex};
use versionx_runtime_trait::VersionSpec;

use super::CoreContext;
use crate::error::{CoreError, CoreResult};

/// Options controlling `update`.
#[derive(Clone, Debug)]
pub struct UpdateOptions {
    pub root: Utf8PathBuf,
    /// Compute and print the plan but do not execute or write `versionx.lock`.
    pub dry_run: bool,
    /// Optional dependency/package selector forwarded to each adapter.
    pub spec: Option<String>,
    /// Restrict to a single ecosystem id (`node`, `python`, `rust`).
    pub ecosystem: Option<String>,
}

/// Structured outcome returned to the CLI.
#[derive(Clone, Debug, Serialize)]
pub struct UpdateOutcome {
    pub config_hash: String,
    pub lockfile_path: Utf8PathBuf,
    pub dry_run: bool,
    pub targeted_spec: Option<String>,
    pub ecosystems: Vec<EcosystemUpdate>,
}

#[derive(Clone, Debug, Serialize)]
pub struct EcosystemUpdate {
    pub ecosystem: String,
    pub root: Utf8PathBuf,
    pub package_manager: Option<String>,
    pub skipped_reason: Option<String>,
    pub warnings: Vec<String>,
    pub step_preview: Option<String>,
    pub duration_ms: u64,
    pub exit_code: Option<i32>,
}

/// Run an update from `opts.root` using the ctx's adapter + runtime registries.
#[allow(clippy::too_many_lines)]
pub async fn update(ctx: &CoreContext, opts: &UpdateOptions) -> CoreResult<UpdateOutcome> {
    let config_path = opts.root.join("versionx.toml");
    if !config_path.exists() {
        return Err(CoreError::NoConfig { path: config_path.to_string() });
    }
    let effective = versionx_config::load(&config_path)?;

    let config_bytes = std::fs::read(&config_path)
        .map_err(|source| CoreError::Io { path: config_path.to_string(), source })?;
    let config_hash = blake3_hex(&config_bytes);
    let lockfile_path = opts.root.join("versionx.lock");
    let mut lock = if lockfile_path.exists() {
        Lockfile::load(&lockfile_path)?
    } else {
        Lockfile::new(config_hash.clone())
    };
    lock.generated_at = chrono::Utc::now();
    lock.versionx_version = env!("CARGO_PKG_VERSION").into();
    lock.config_hash = config_hash.clone();

    backfill_runtime_lock_entries(ctx, &effective.config.runtimes.tools, &mut lock).await?;

    let mut ecosystems = Vec::new();
    for (eco_id, eco_cfg) in &effective.config.ecosystems {
        if opts.ecosystem.as_deref().is_some_and(|target| target != eco_id) {
            continue;
        }

        let Some(adapter) = ctx.adapter_registry.get(eco_id) else {
            ecosystems.push(EcosystemUpdate {
                ecosystem: eco_id.clone(),
                root: eco_root(&opts.root, eco_cfg.root.as_ref()),
                package_manager: None,
                skipped_reason: Some("no adapter for this ecosystem in this build".into()),
                warnings: Vec::new(),
                step_preview: None,
                duration_ms: 0,
                exit_code: None,
            });
            continue;
        };

        let eco_root = eco_root(&opts.root, eco_cfg.root.as_ref());
        let adapter_ctx = AdapterContext {
            cwd: eco_root.clone(),
            runtime_bin_dir: runtime_bin_dir_for(ctx, eco_id),
            events: ctx.events.clone(),
            env: Vec::new(),
            dry_run: opts.dry_run,
        };

        let detect = adapter.detect(&adapter_ctx).await?;
        if !detect.applicable {
            ecosystems.push(EcosystemUpdate {
                ecosystem: eco_id.clone(),
                root: eco_root,
                package_manager: None,
                skipped_reason: Some("adapter reported not-applicable at this cwd".into()),
                warnings: Vec::new(),
                step_preview: None,
                duration_ms: 0,
                exit_code: None,
            });
            continue;
        }

        let intent = Intent::Upgrade { spec: opts.spec.clone() };
        let plan = adapter.plan(&adapter_ctx, &intent).await?;
        let Some(first_step) = plan.steps.first().cloned() else {
            ecosystems.push(EcosystemUpdate {
                ecosystem: eco_id.clone(),
                root: eco_root,
                package_manager: detect.package_manager.clone(),
                skipped_reason: Some("empty plan".into()),
                warnings: plan.warnings,
                step_preview: None,
                duration_ms: 0,
                exit_code: None,
            });
            continue;
        };

        let outcome = adapter.execute(&adapter_ctx, &first_step, &intent).await?;
        ecosystems.push(EcosystemUpdate {
            ecosystem: eco_id.clone(),
            root: eco_root.clone(),
            package_manager: detect.package_manager.clone(),
            skipped_reason: None,
            warnings: plan.warnings,
            step_preview: Some(first_step.command_preview.clone()),
            duration_ms: outcome.duration_ms,
            exit_code: outcome.exit_code,
        });

        lock.ecosystems.insert(
            eco_id.clone(),
            LockedEcosystem {
                package_manager: detect.package_manager.unwrap_or_default(),
                native_lockfile: detect
                    .manifest_path
                    .as_ref()
                    .and_then(|_| detect_native_lockfile_name(&eco_root, eco_id)),
                native_lockfile_hash: detect_native_lockfile_hash(&eco_root, eco_id)?,
                resolved_at: Some(chrono::Utc::now()),
            },
        );
    }

    if !opts.dry_run {
        lock.save(&lockfile_path)?;
    }

    Ok(UpdateOutcome {
        config_hash,
        lockfile_path,
        dry_run: opts.dry_run,
        targeted_spec: opts.spec.clone(),
        ecosystems,
    })
}

async fn backfill_runtime_lock_entries(
    ctx: &CoreContext,
    tools: &indexmap::IndexMap<String, versionx_config::schema::RuntimeSpec>,
    lock: &mut Lockfile,
) -> CoreResult<()> {
    let installer_ctx = ctx.installer_ctx();
    for (tool, spec) in tools {
        if lock.runtimes.contains_key(tool) {
            continue;
        }
        let Some(installer) = ctx.registry.get(tool) else {
            continue;
        };
        let resolved =
            installer.resolve_version(&VersionSpec::new(spec.version()), &installer_ctx).await?;
        let install_path = installer.install_path(&resolved, &installer_ctx);
        lock.runtimes.insert(
            tool.to_string(),
            LockedRuntime {
                version: resolved.version.clone(),
                source: resolved.source.clone(),
                sha256: None,
                install_path: Some(install_path),
            },
        );
    }
    Ok(())
}

fn eco_root(root: &camino::Utf8Path, configured: Option<&Utf8PathBuf>) -> Utf8PathBuf {
    configured.map_or_else(|| root.to_path_buf(), |path| root.join(path))
}

fn runtime_bin_dir_for(ctx: &CoreContext, ecosystem: &str) -> Option<Utf8PathBuf> {
    match ecosystem {
        "node" | "python" | "rust" => Some(ctx.home.shims_dir()),
        _ => None,
    }
}

fn detect_native_lockfile_name(root: &camino::Utf8Path, ecosystem: &str) -> Option<String> {
    native_lockfile_path(root, ecosystem)
        .and_then(|path| path.file_name().map(ToString::to_string))
}

fn detect_native_lockfile_hash(
    root: &camino::Utf8Path,
    ecosystem: &str,
) -> CoreResult<Option<String>> {
    let Some(path) = native_lockfile_path(root, ecosystem) else {
        return Ok(None);
    };
    let bytes = std::fs::read(&path)
        .map_err(|source| CoreError::Io { path: path.to_string(), source })?;
    Ok(Some(blake3_hex(&bytes)))
}

fn native_lockfile_path(root: &camino::Utf8Path, ecosystem: &str) -> Option<Utf8PathBuf> {
    let candidates: &[&str] = match ecosystem {
        "node" => &["pnpm-lock.yaml", "package-lock.json", "yarn.lock"],
        "python" => &["uv.lock", "poetry.lock", "requirements.lock", "requirements.txt"],
        "rust" => &["Cargo.lock"],
        _ => &[],
    };
    candidates.iter().map(|name| root.join(name)).find(|path| path.is_file())
}
