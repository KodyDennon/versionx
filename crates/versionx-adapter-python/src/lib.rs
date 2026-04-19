//! Python ecosystem adapter.
//!
//! Drives `uv` / `poetry` / `pip` as subprocesses. PM detection, in priority
//! order:
//! 1. `[tool.uv]` / `[tool.poetry]` block in `pyproject.toml`.
//! 2. Presence of `uv.lock` / `poetry.lock`.
//! 3. Presence of `requirements.txt` → pip.
//! 4. Default to uv (Astral's direction; fastest install path).
//!
//! Venv creation is delegated to uv / poetry — they already manage `.venv`
//! better than we could.

#![deny(unsafe_code)]

use async_trait::async_trait;
use blake3::Hasher;
use camino::Utf8PathBuf;
use versionx_adapter_trait::{
    AdapterContext, AdapterError, AdapterResult, DetectResult, Ecosystem, Intent,
    PackageManagerAdapter, Plan, PlanStep, StepOutcome, resolve_binary,
};
use versionx_events::Level;

const ADAPTER_ID: &str = "python";

#[derive(Debug, Default)]
pub struct PythonAdapter;

impl PythonAdapter {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Pm {
    Uv,
    Poetry,
    Pip,
}

impl Pm {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Uv => "uv",
            Self::Poetry => "poetry",
            Self::Pip => "pip",
        }
    }
}

#[async_trait]
impl PackageManagerAdapter for PythonAdapter {
    fn id(&self) -> &'static str {
        ADAPTER_ID
    }
    fn ecosystem(&self) -> Ecosystem {
        Ecosystem::Python
    }

    async fn detect(&self, ctx: &AdapterContext) -> AdapterResult<DetectResult> {
        let pyproj = ctx.cwd.join("pyproject.toml");
        let reqs = ctx.cwd.join("requirements.txt");
        if !pyproj.exists() && !reqs.exists() {
            return Ok(DetectResult {
                applicable: false,
                reason: None,
                package_manager: None,
                manifest_path: None,
            });
        }

        let (pm, reason) = detect_pm(&ctx.cwd)?;
        let manifest = if pyproj.exists() { Some(pyproj) } else { Some(reqs) };
        Ok(DetectResult {
            applicable: true,
            reason: Some(reason),
            package_manager: Some(pm.as_str().to_string()),
            manifest_path: manifest,
        })
    }

    async fn plan(&self, ctx: &AdapterContext, intent: &Intent) -> AdapterResult<Plan> {
        let (pm, _) = detect_pm(&ctx.cwd)?;
        let preview = command_preview(pm, intent, &ctx.cwd);

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

        let mut warnings = Vec::new();
        if matches!(intent, Intent::Sync) && !has_lockfile(&ctx.cwd, pm) {
            warnings.push(format!(
                "no {lock} found; `sync` will create one",
                lock = pm_lockfile_name(pm).unwrap_or("(none)")
            ));
        }

        Ok(Plan {
            steps: vec![PlanStep {
                id: id[..16].to_string(),
                action: action_name(intent).into(),
                command_preview: preview.clone(),
                affects_lockfile,
            }],
            summary: preview,
            warnings,
        })
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
        // Scrub env that poisons Python subprocesses.
        for scrub in ["PYTHONHOME", "PYTHONPATH", "PIP_TARGET", "PIP_USER"] {
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

fn command_preview(pm: Pm, intent: &Intent, cwd: &camino::Utf8Path) -> String {
    let (prog, args) = command_line(pm, intent, cwd);
    std::iter::once(prog.to_string()).chain(args).collect::<Vec<_>>().join(" ")
}

fn command_line(pm: Pm, intent: &Intent, cwd: &camino::Utf8Path) -> (&'static str, Vec<String>) {
    let prog = pm.as_str();
    let frozen = has_lockfile(cwd, pm);
    let args: Vec<String> = match (pm, intent) {
        (Pm::Uv, Intent::Sync) => {
            if frozen {
                vec!["sync".into(), "--frozen".into()]
            } else {
                vec!["sync".into()]
            }
        }
        (Pm::Poetry, Intent::Sync) => {
            if frozen {
                vec!["install".into(), "--sync".into(), "--no-interaction".into()]
            } else {
                vec!["install".into(), "--no-interaction".into()]
            }
        }
        (Pm::Pip, Intent::Sync) => {
            if cwd.join("requirements.lock").exists() {
                vec!["install".into(), "-r".into(), "requirements.lock".into()]
            } else if cwd.join("requirements.txt").exists() {
                vec!["install".into(), "-r".into(), "requirements.txt".into()]
            } else {
                vec!["install".into()]
            }
        }
        (Pm::Uv, Intent::Install { spec, dev }) => {
            let mut a = vec!["add".into(), spec.clone()];
            if *dev {
                a.push("--dev".into());
            }
            a
        }
        (Pm::Poetry, Intent::Install { spec, dev }) => {
            let mut a = vec!["add".into(), "--no-interaction".into(), spec.clone()];
            if *dev {
                a.push("--group".into());
                a.push("dev".into());
            }
            a
        }
        (Pm::Pip, Intent::Install { spec, dev: _ }) => {
            vec!["install".into(), spec.clone()]
        }
        (Pm::Uv, Intent::Remove { name }) => vec!["remove".into(), name.clone()],
        (Pm::Poetry, Intent::Remove { name }) => {
            vec!["remove".into(), "--no-interaction".into(), name.clone()]
        }
        (Pm::Pip, Intent::Remove { name }) => vec!["uninstall".into(), "-y".into(), name.clone()],
        (Pm::Uv, Intent::Upgrade { spec }) => {
            let mut a = vec!["lock".into(), "--upgrade".into()];
            if let Some(s) = spec {
                a.push("--upgrade-package".into());
                a.push(s.clone());
            }
            a
        }
        (Pm::Poetry, Intent::Upgrade { spec }) => {
            let mut a = vec!["update".into(), "--no-interaction".into()];
            if let Some(s) = spec {
                a.push(s.clone());
            }
            a
        }
        (Pm::Pip, Intent::Upgrade { spec }) => {
            let mut a = vec!["install".into(), "--upgrade".into()];
            if let Some(s) = spec {
                a.push(s.clone());
            } else {
                a.push("-r".into());
                a.push("requirements.txt".into());
            }
            a
        }
        (Pm::Uv, Intent::LockOnly) => vec!["lock".into()],
        (Pm::Poetry, Intent::LockOnly) => {
            vec!["lock".into(), "--no-interaction".into()]
        }
        (Pm::Pip, Intent::LockOnly) => {
            vec!["help".into()]
        }
    };
    (prog, args)
}

fn has_lockfile(cwd: &camino::Utf8Path, pm: Pm) -> bool {
    pm_lockfile_name(pm).is_some_and(|n| cwd.join(n).is_file())
}

const fn pm_lockfile_name(pm: Pm) -> Option<&'static str> {
    match pm {
        Pm::Uv => Some("uv.lock"),
        Pm::Poetry => Some("poetry.lock"),
        Pm::Pip => None,
    }
}

/// Detect the PM by reading pyproject.toml tool tables, then lockfile
/// presence, then `requirements.txt`, then uv default.
fn detect_pm(cwd: &camino::Utf8Path) -> AdapterResult<(Pm, String)> {
    let pyproj = cwd.join("pyproject.toml");
    if pyproj.exists() {
        let raw = std::fs::read_to_string(&pyproj)
            .map_err(|source| AdapterError::Io { path: pyproj.clone(), source })?;
        if raw.contains("[tool.uv]") || raw.contains("[tool.uv.") {
            return Ok((Pm::Uv, "pyproject:[tool.uv]".into()));
        }
        if raw.contains("[tool.poetry]") || raw.contains("[tool.poetry.") {
            return Ok((Pm::Poetry, "pyproject:[tool.poetry]".into()));
        }
    }

    if cwd.join("uv.lock").is_file() {
        return Ok((Pm::Uv, "uv.lock".into()));
    }
    if cwd.join("poetry.lock").is_file() {
        return Ok((Pm::Poetry, "poetry.lock".into()));
    }

    if cwd.join("requirements.txt").is_file() {
        return Ok((Pm::Pip, "requirements.txt".into()));
    }

    Ok((Pm::Uv, "default:uv".into()))
}

fn tail_string(bytes: &[u8], max: usize) -> String {
    let s = String::from_utf8_lossy(bytes);
    if s.len() <= max {
        s.into_owned()
    } else {
        let start = s.len() - max;
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

    fn tmp_cwd() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn detect_uv_from_pyproject() {
        let dir = tmp_cwd();
        let path = camino::Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        std::fs::write(
            path.join("pyproject.toml"),
            "[project]\nname='p'\n[tool.uv]\ndev-dependencies=[]",
        )
        .unwrap();
        let (pm, reason) = detect_pm(&path).unwrap();
        assert_eq!(pm, Pm::Uv);
        assert!(reason.contains("uv"));
    }

    #[test]
    fn detect_poetry_from_pyproject() {
        let dir = tmp_cwd();
        let path = camino::Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        std::fs::write(path.join("pyproject.toml"), "[tool.poetry]\nname='p'\nversion='0.0.1'")
            .unwrap();
        let (pm, _) = detect_pm(&path).unwrap();
        assert_eq!(pm, Pm::Poetry);
    }

    #[test]
    fn detect_pip_from_requirements() {
        let dir = tmp_cwd();
        let path = camino::Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        std::fs::write(path.join("requirements.txt"), "requests==2.31\n").unwrap();
        let (pm, _) = detect_pm(&path).unwrap();
        assert_eq!(pm, Pm::Pip);
    }

    #[test]
    fn default_uv_when_nothing_matches() {
        let dir = tmp_cwd();
        let path = camino::Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        let (pm, reason) = detect_pm(&path).unwrap();
        assert_eq!(pm, Pm::Uv);
        assert!(reason.starts_with("default"));
    }

    #[test]
    fn sync_command_lines() {
        let dir = tmp_cwd();
        let path = camino::Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        assert_eq!(command_line(Pm::Uv, &Intent::Sync, &path).1, vec!["sync"]);
        std::fs::write(path.join("uv.lock"), "").unwrap();
        assert_eq!(command_line(Pm::Uv, &Intent::Sync, &path).1, vec!["sync", "--frozen"]);
    }

    #[test]
    fn install_command_respects_dev_flag() {
        let cwd = camino::Utf8Path::new(".");
        let intent = Intent::Install { spec: "requests".into(), dev: true };
        assert_eq!(command_line(Pm::Uv, &intent, cwd).1, vec!["add", "requests", "--dev"]);
        assert_eq!(
            command_line(Pm::Poetry, &intent, cwd).1,
            vec!["add", "--no-interaction", "requests", "--group", "dev"]
        );
        assert_eq!(command_line(Pm::Pip, &intent, cwd).1, vec!["install", "requests"]);
    }
}
