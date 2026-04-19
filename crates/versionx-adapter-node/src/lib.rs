//! Node.js ecosystem adapter.
//!
//! Drives `pnpm`, `npm`, and `yarn` as subprocesses. Adapter picks which PM
//! from (in priority order):
//! 1. Explicit `package_manager` value in the ecosystem config.
//! 2. `packageManager` field in `package.json` (`"pnpm@8.15.0"`).
//! 3. Presence of a lockfile (`pnpm-lock.yaml` / `yarn.lock` / `package-lock.json`).
//! 4. Default to `npm` if nothing else is detectable.

#![deny(unsafe_code)]

use async_trait::async_trait;
use blake3::Hasher;
use camino::Utf8PathBuf;
use serde::Deserialize;
use versionx_adapter_trait::{
    AdapterContext, AdapterError, AdapterResult, DetectResult, Ecosystem, Intent,
    PackageManagerAdapter, Plan, PlanStep, StepOutcome, resolve_binary,
};
use versionx_events::Level;

/// `versionx-adapter-node` is the canonical adapter id.
const ADAPTER_ID: &str = "node";

#[derive(Debug, Default)]
pub struct NodeAdapter;

impl NodeAdapter {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

/// The three package managers covered by this adapter.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Pm {
    Pnpm,
    Npm,
    Yarn,
}

impl Pm {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pnpm => "pnpm",
            Self::Npm => "npm",
            Self::Yarn => "yarn",
        }
    }
}

#[async_trait]
impl PackageManagerAdapter for NodeAdapter {
    fn id(&self) -> &'static str {
        ADAPTER_ID
    }
    fn ecosystem(&self) -> Ecosystem {
        Ecosystem::Node
    }

    async fn detect(&self, ctx: &AdapterContext) -> AdapterResult<DetectResult> {
        let manifest = ctx.cwd.join("package.json");
        if !manifest.exists() {
            return Ok(DetectResult {
                applicable: false,
                reason: None,
                package_manager: None,
                manifest_path: None,
            });
        }

        let (pm, reason) = detect_pm(&ctx.cwd)?;
        Ok(DetectResult {
            applicable: true,
            reason: Some(reason),
            package_manager: Some(pm.as_str().to_string()),
            manifest_path: Some(manifest),
        })
    }

    async fn plan(&self, ctx: &AdapterContext, intent: &Intent) -> AdapterResult<Plan> {
        let (pm, _) = detect_pm(&ctx.cwd)?;
        let preview = command_preview(pm, intent);

        let mut hasher = Hasher::new();
        hasher.update(pm.as_str().as_bytes());
        hasher.update(format!("{intent:?}").as_bytes());
        hasher.update(ctx.cwd.as_str().as_bytes());
        let id = hasher.finalize().to_hex().to_string();

        let affects_lockfile = match intent {
            Intent::Sync => false,
            Intent::Install { .. }
            | Intent::Remove { .. }
            | Intent::Upgrade { .. }
            | Intent::LockOnly => true,
        };

        let step = PlanStep {
            id: id[..16].to_string(),
            action: action_name(intent).into(),
            command_preview: preview.clone(),
            affects_lockfile,
        };

        let mut warnings = Vec::new();
        if matches!(intent, Intent::Sync) && !has_lockfile(&ctx.cwd, pm) {
            warnings.push(format!(
                "no {pm_lock} found; `sync` will fall back to a non-frozen install",
                pm_lock = pm_lockfile_name(pm)
            ));
        }

        Ok(Plan { steps: vec![step], summary: preview, warnings })
    }

    async fn execute(
        &self,
        ctx: &AdapterContext,
        step: &PlanStep,
        intent: &Intent,
    ) -> AdapterResult<StepOutcome> {
        if ctx.dry_run {
            return Ok(StepOutcome {
                step_id: step.id.clone(),
                exit_code: Some(0),
                duration_ms: 0,
                stdout_tail: "(dry-run)".into(),
                stderr_tail: String::new(),
            });
        }

        let (pm, _) = detect_pm(&ctx.cwd)?;
        let (program, args) = command_line(pm, intent, &ctx.cwd);
        let bin = resolve_binary(ctx.runtime_bin_dir.as_deref(), program);

        ctx.events.emit(versionx_events::Event::new(
            "adapter.exec.start",
            Level::Info,
            format!("{program} {}", args.join(" ")),
        ));

        let start = std::time::Instant::now();
        let mut cmd = tokio::process::Command::new(&bin);
        cmd.args(&args).current_dir(ctx.cwd.as_std_path());
        for (k, v) in &ctx.env {
            cmd.env(k, v);
        }
        // Scrub env vars we never want to inherit.
        for scrub in ["NODE_OPTIONS", "NPM_CONFIG_AUDIT"] {
            cmd.env_remove(scrub);
        }

        let output = cmd
            .output()
            .await
            .map_err(|source| AdapterError::Io { path: Utf8PathBuf::from(program), source })?;

        let duration_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
        let stdout_tail = tail_string(&output.stdout, 4096);
        let stderr_tail = tail_string(&output.stderr, 4096);
        let status = output.status.code();

        if !output.status.success() {
            return Err(AdapterError::Subprocess {
                program: program.into(),
                status: status.unwrap_or(-1),
                stderr: stderr_tail,
            });
        }

        ctx.events.emit(versionx_events::Event::new(
            "adapter.exec.complete",
            Level::Info,
            format!("{program} exited {}", status.unwrap_or(-1)),
        ));

        Ok(StepOutcome {
            step_id: step.id.clone(),
            exit_code: status,
            duration_ms,
            stdout_tail,
            stderr_tail,
        })
    }
}

const fn action_name(intent: &Intent) -> &'static str {
    match intent {
        Intent::Sync => "sync",
        Intent::Install { .. } => "install",
        Intent::Remove { .. } => "remove",
        Intent::Upgrade { .. } => "upgrade",
        Intent::LockOnly => "lock-only",
    }
}

fn command_preview(pm: Pm, intent: &Intent) -> String {
    let (prog, args) = command_line(pm, intent, camino::Utf8Path::new("."));
    std::iter::once(prog.to_string()).chain(args).collect::<Vec<_>>().join(" ")
}

fn command_line(pm: Pm, intent: &Intent, cwd: &camino::Utf8Path) -> (&'static str, Vec<String>) {
    let prog = pm.as_str();
    let frozen = has_lockfile(cwd, pm);
    let args = match (pm, intent) {
        (Pm::Pnpm, Intent::Sync) => {
            if frozen {
                vec!["install".into(), "--frozen-lockfile".into()]
            } else {
                vec!["install".into()]
            }
        }
        (Pm::Npm, Intent::Sync) => {
            if frozen {
                vec!["ci".into()]
            } else {
                vec!["install".into()]
            }
        }
        (Pm::Yarn, Intent::Sync) => {
            if frozen {
                vec!["install".into(), "--immutable".into()]
            } else {
                vec!["install".into()]
            }
        }
        (Pm::Pnpm, Intent::Install { spec, dev }) => {
            let mut a = vec!["add".into(), spec.clone()];
            if *dev {
                a.push("--save-dev".into());
            }
            a
        }
        (Pm::Npm, Intent::Install { spec, dev }) => {
            let mut a = vec!["install".into(), spec.clone()];
            if *dev {
                a.push("--save-dev".into());
            }
            a
        }
        (Pm::Yarn, Intent::Install { spec, dev }) => {
            let mut a = vec!["add".into(), spec.clone()];
            if *dev {
                a.push("--dev".into());
            }
            a
        }
        (Pm::Pnpm | Pm::Yarn, Intent::Remove { name }) => vec!["remove".into(), name.clone()],
        (Pm::Npm, Intent::Remove { name }) => vec!["uninstall".into(), name.clone()],
        (Pm::Pnpm | Pm::Npm, Intent::Upgrade { spec }) => {
            let mut a = vec!["update".into()];
            if let Some(s) = spec {
                a.push(s.clone());
            }
            a
        }
        (Pm::Yarn, Intent::Upgrade { spec }) => {
            let mut a = vec!["up".into()];
            if let Some(s) = spec {
                a.push(s.clone());
            }
            a
        }
        (Pm::Pnpm | Pm::Yarn, Intent::LockOnly) => {
            vec!["install".into(), "--lockfile-only".into()]
        }
        (Pm::Npm, Intent::LockOnly) => vec!["install".into(), "--package-lock-only".into()],
    };
    (prog, args)
}

fn has_lockfile(cwd: &camino::Utf8Path, pm: Pm) -> bool {
    cwd.join(pm_lockfile_name(pm)).is_file()
}

const fn pm_lockfile_name(pm: Pm) -> &'static str {
    match pm {
        Pm::Pnpm => "pnpm-lock.yaml",
        Pm::Npm => "package-lock.json",
        Pm::Yarn => "yarn.lock",
    }
}

/// Detect the PM from signals in `cwd`. Precedence:
///   1. `packageManager` field in `package.json`.
///   2. Lockfile presence.
///   3. Default to npm.
fn detect_pm(cwd: &camino::Utf8Path) -> AdapterResult<(Pm, String)> {
    let pkg = cwd.join("package.json");
    if pkg.exists() {
        let raw = std::fs::read_to_string(&pkg)
            .map_err(|source| AdapterError::Io { path: pkg.clone(), source })?;
        #[derive(Deserialize)]
        struct Parsed {
            #[serde(default, rename = "packageManager")]
            package_manager: Option<String>,
        }
        let parsed: Parsed = serde_json::from_str(&raw).unwrap_or(Parsed { package_manager: None });
        if let Some(pm_field) = parsed.package_manager.as_deref() {
            let name = pm_field.split('@').next().unwrap_or("");
            if let Some(pm) = pm_by_name(name) {
                return Ok((pm, format!("packageManager:{pm_field}")));
            }
        }
    }

    let mut found = Vec::new();
    for (pm, reason) in
        [(Pm::Pnpm, "pnpm-lock.yaml"), (Pm::Yarn, "yarn.lock"), (Pm::Npm, "package-lock.json")]
    {
        if cwd.join(reason).is_file() {
            found.push((pm, reason));
        }
    }
    match found.as_slice() {
        [] => Ok((Pm::Npm, "default:npm".into())),
        [(pm, reason)] => Ok((*pm, (*reason).to_string())),
        many => Err(AdapterError::AmbiguousPackageManager {
            found: many.iter().map(|(_, r)| (*r).to_string()).collect(),
        }),
    }
}

const fn pm_by_name(name: &str) -> Option<Pm> {
    match name.as_bytes() {
        b"pnpm" => Some(Pm::Pnpm),
        b"npm" => Some(Pm::Npm),
        b"yarn" => Some(Pm::Yarn),
        _ => None,
    }
}

fn tail_string(bytes: &[u8], max: usize) -> String {
    let s = String::from_utf8_lossy(bytes);
    if s.len() <= max {
        s.into_owned()
    } else {
        let start = s.len() - max;
        // Clamp to a char boundary to avoid panicking.
        let mut idx = start;
        while !s.is_char_boundary(idx) && idx < s.len() {
            idx += 1;
        }
        s[idx..].to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_cwd() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn detects_pnpm_via_packagemanager_field() {
        let dir = fake_cwd();
        let path = camino::Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        std::fs::write(path.join("package.json"), r#"{"name":"t","packageManager":"pnpm@8.15.0"}"#)
            .unwrap();
        let (pm, reason) = detect_pm(&path).unwrap();
        assert_eq!(pm, Pm::Pnpm);
        assert!(reason.contains("pnpm@8.15.0"));
    }

    #[test]
    fn detects_yarn_from_lockfile() {
        let dir = fake_cwd();
        let path = camino::Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        std::fs::write(path.join("package.json"), r#"{"name":"t"}"#).unwrap();
        std::fs::write(path.join("yarn.lock"), "# yarn lockfile v1\n").unwrap();
        let (pm, _) = detect_pm(&path).unwrap();
        assert_eq!(pm, Pm::Yarn);
    }

    #[test]
    fn defaults_to_npm_when_no_signals() {
        let dir = fake_cwd();
        let path = camino::Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        std::fs::write(path.join("package.json"), r#"{"name":"t"}"#).unwrap();
        let (pm, _) = detect_pm(&path).unwrap();
        assert_eq!(pm, Pm::Npm);
    }

    #[test]
    fn ambiguous_lockfiles_error() {
        let dir = fake_cwd();
        let path = camino::Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        std::fs::write(path.join("package.json"), r#"{"name":"t"}"#).unwrap();
        std::fs::write(path.join("pnpm-lock.yaml"), "").unwrap();
        std::fs::write(path.join("yarn.lock"), "").unwrap();
        assert!(matches!(detect_pm(&path), Err(AdapterError::AmbiguousPackageManager { .. })));
    }

    #[test]
    fn install_with_dev_flag_per_pm() {
        let intent = Intent::Install { spec: "lodash".into(), dev: true };
        let cwd = camino::Utf8Path::new(".");
        assert_eq!(command_line(Pm::Pnpm, &intent, cwd).1, vec!["add", "lodash", "--save-dev"]);
        assert_eq!(command_line(Pm::Npm, &intent, cwd).1, vec!["install", "lodash", "--save-dev"]);
        assert_eq!(command_line(Pm::Yarn, &intent, cwd).1, vec!["add", "lodash", "--dev"]);
    }
}
