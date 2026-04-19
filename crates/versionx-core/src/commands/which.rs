//! `versionx which <tool>` — show how a shim would resolve.

use camino::Utf8PathBuf;
use serde::Serialize;

use super::CoreContext;
use crate::error::{CoreError, CoreResult};

/// Options controlling `which`.
#[derive(Clone, Debug)]
pub struct WhichOptions {
    pub tool: String,
    pub cwd: Utf8PathBuf,
}

#[derive(Clone, Debug, Serialize)]
pub struct WhichOutcome {
    pub tool: String,
    pub resolved_version: Option<String>,
    pub resolved_source: Option<String>,
    pub install_path: Option<Utf8PathBuf>,
    pub binary: Option<Utf8PathBuf>,
    pub reason: String,
}

/// Resolve where a shim for `tool` would dispatch.
///
/// # Errors
/// - [`CoreError::UnknownRuntime`] if no installer covers the tool.
pub async fn which(ctx: &CoreContext, opts: &WhichOptions) -> CoreResult<WhichOutcome> {
    let installer =
        ctx.registry.get(&opts.tool).ok_or_else(|| CoreError::UnknownRuntime(opts.tool.clone()))?;
    let installer_ctx = ctx.installer_ctx();

    // Priority order matches the shim's own resolution order:
    //   1. per-invocation env var (VERSIONX_<TOOL>_VERSION).
    //   2. versionx.toml walking up from `cwd`.
    //   3. user global config.
    let env_key = format!("VERSIONX_{}_VERSION", opts.tool.to_uppercase());
    let (version, reason) = if let Ok(v) = std::env::var(&env_key) {
        (Some(v), format!("env {env_key}"))
    } else if let Some(v) = resolve_from_config(&opts.cwd, &opts.tool) {
        v
    } else if let Some(v) = resolve_from_global(ctx, &opts.tool) {
        v
    } else {
        return Ok(WhichOutcome {
            tool: opts.tool.clone(),
            resolved_version: None,
            resolved_source: None,
            install_path: None,
            binary: None,
            reason: format!("no version pinned for `{}` in config, env, or global", opts.tool),
        });
    };

    let Some(version) = version else {
        return Ok(WhichOutcome {
            tool: opts.tool.clone(),
            resolved_version: None,
            resolved_source: None,
            install_path: None,
            binary: None,
            reason,
        });
    };

    // Resolve version -> installer metadata. No network work on `which`:
    // we use the registered install_path predictor without hitting the index.
    let resolved = versionx_runtime_trait::ResolvedVersion {
        version: version.clone(),
        channel: None,
        source: "<pending>".into(),
        sha256: None,
        url: None,
    };
    let install_path = installer.install_path(&resolved, &installer_ctx);
    let exists = install_path.exists();

    let binary = if exists { pick_binary(&install_path, &opts.tool) } else { None };

    Ok(WhichOutcome {
        tool: opts.tool.clone(),
        resolved_version: Some(version),
        resolved_source: Some(installer.display_name().to_string()),
        install_path: exists.then_some(install_path),
        binary,
        reason: if exists {
            reason
        } else {
            format!("{reason} (not installed — run `versionx sync`)")
        },
    })
}

fn resolve_from_config(cwd: &camino::Utf8Path, tool: &str) -> Option<(Option<String>, String)> {
    let mut cursor: Option<&camino::Utf8Path> = Some(cwd);
    while let Some(dir) = cursor {
        let candidate = dir.join("versionx.toml");
        if candidate.is_file()
            && let Ok(loaded) = versionx_config::load(&candidate)
            && let Some(spec) = loaded.config.runtimes.tools.get(tool)
        {
            return Some((
                Some(spec.version().to_string()),
                format!("versionx.toml at {candidate}"),
            ));
        }
        cursor = dir.parent();
    }
    None
}

fn resolve_from_global(ctx: &CoreContext, tool: &str) -> Option<(Option<String>, String)> {
    let path = ctx.home.global_config();
    if !path.is_file() {
        return None;
    }
    let loaded = versionx_config::load(&path).ok()?;
    let spec = loaded.config.runtimes.tools.get(tool)?;
    Some((Some(spec.version().to_string()), format!("global config at {path}")))
}

fn pick_binary(install_path: &camino::Utf8Path, tool: &str) -> Option<Utf8PathBuf> {
    let candidates = match tool {
        "node" => vec!["bin/node", "node.exe"],
        "python" => vec!["bin/python3", "python.exe"],
        "rust" => vec!["bin/cargo", "bin/cargo.exe"],
        _ => vec!["bin/"],
    };
    for rel in candidates {
        let p = install_path.join(rel);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}
