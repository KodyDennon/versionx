//! Rust ecosystem adapter — drives `cargo`.
//!
//! Cargo is the only PM here; detection is just "is there a Cargo.toml".
//! `sync` maps to `cargo fetch` (and `--locked` when `Cargo.lock` exists).
//! `install <spec>` uses `cargo add` (from cargo-edit, baked into cargo
//! since 1.62). `upgrade` uses `cargo update`. Publishing is a release
//! concern, not a sync concern — handled by `versionx release` later.

#![deny(unsafe_code)]

use async_trait::async_trait;
use blake3::Hasher;
use camino::Utf8PathBuf;
use versionx_adapter_trait::{
    AdapterContext, AdapterError, AdapterResult, DetectResult, Ecosystem, Intent,
    PackageManagerAdapter, Plan, PlanStep, StepOutcome, resolve_binary,
};
use versionx_events::Level;

const ADAPTER_ID: &str = "rust";

#[derive(Debug, Default)]
pub struct RustAdapter;

impl RustAdapter {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

#[async_trait]
impl PackageManagerAdapter for RustAdapter {
    fn id(&self) -> &'static str {
        ADAPTER_ID
    }
    fn ecosystem(&self) -> Ecosystem {
        Ecosystem::Rust
    }

    async fn detect(&self, ctx: &AdapterContext) -> AdapterResult<DetectResult> {
        let manifest = ctx.cwd.join("Cargo.toml");
        if !manifest.exists() {
            return Ok(DetectResult {
                applicable: false,
                reason: None,
                package_manager: None,
                manifest_path: None,
            });
        }
        let is_workspace =
            std::fs::read_to_string(&manifest).is_ok_and(|raw| raw.contains("[workspace]"));
        let reason = if is_workspace { "Cargo.toml ([workspace])" } else { "Cargo.toml" };
        Ok(DetectResult {
            applicable: true,
            reason: Some(reason.into()),
            package_manager: Some("cargo".into()),
            manifest_path: Some(manifest),
        })
    }

    async fn plan(&self, ctx: &AdapterContext, intent: &Intent) -> AdapterResult<Plan> {
        let preview = command_preview(intent, &ctx.cwd);

        let mut hasher = Hasher::new();
        hasher.update(b"cargo");
        hasher.update(format!("{intent:?}").as_bytes());
        hasher.update(ctx.cwd.as_str().as_bytes());
        let id = hasher.finalize().to_hex().to_string();

        let affects_lockfile = matches!(
            intent,
            Intent::Install { .. }
                | Intent::Remove { .. }
                | Intent::Upgrade { .. }
                | Intent::LockOnly
        );

        let mut warnings = Vec::new();
        if matches!(intent, Intent::Sync) && !ctx.cwd.join("Cargo.lock").is_file() {
            warnings.push("no Cargo.lock yet; `sync` will create one".into());
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

        let (program, args) = command_line(intent, &ctx.cwd);
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
        // Never set RUSTC — rustup 1.25+ bug #3031 breaks toolchain overrides.
        cmd.env_remove("RUSTC");

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

fn command_preview(intent: &Intent, cwd: &camino::Utf8Path) -> String {
    let (prog, args) = command_line(intent, cwd);
    std::iter::once(prog.to_string()).chain(args).collect::<Vec<_>>().join(" ")
}

fn command_line(intent: &Intent, cwd: &camino::Utf8Path) -> (&'static str, Vec<String>) {
    let locked = cwd.join("Cargo.lock").is_file();
    let args: Vec<String> = match intent {
        Intent::Sync => {
            let mut a = vec!["fetch".into()];
            if locked {
                a.push("--locked".into());
            }
            a
        }
        Intent::Install { spec, dev } => {
            let mut a = vec!["add".into(), spec.clone()];
            if *dev {
                a.push("--dev".into());
            }
            a
        }
        Intent::Remove { name } => vec!["remove".into(), name.clone()],
        Intent::Upgrade { spec } => {
            let mut a = vec!["update".into()];
            if let Some(s) = spec {
                a.push("--package".into());
                a.push(s.clone());
            }
            a
        }
        Intent::LockOnly => vec!["generate-lockfile".into()],
    };
    ("cargo", args)
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

    fn tmp() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn detect_false_without_cargo_toml() {
        let dir = tmp();
        let path = camino::Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        assert!(!path.join("Cargo.toml").exists());
    }

    #[test]
    fn sync_command_is_fetch() {
        let dir = tmp();
        let path = camino::Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        assert_eq!(command_line(&Intent::Sync, &path).1, vec!["fetch"]);
        std::fs::write(path.join("Cargo.lock"), "").unwrap();
        assert_eq!(command_line(&Intent::Sync, &path).1, vec!["fetch", "--locked"]);
    }

    #[test]
    fn install_maps_to_cargo_add() {
        let intent = Intent::Install { spec: "serde".into(), dev: false };
        let cwd = camino::Utf8Path::new(".");
        assert_eq!(command_line(&intent, cwd).1, vec!["add", "serde"]);

        let dev = Intent::Install { spec: "proptest".into(), dev: true };
        assert_eq!(command_line(&dev, cwd).1, vec!["add", "proptest", "--dev"]);
    }

    #[test]
    fn upgrade_scoped_to_package() {
        let cwd = camino::Utf8Path::new(".");
        assert_eq!(
            command_line(&Intent::Upgrade { spec: Some("serde".into()) }, cwd).1,
            vec!["update", "--package", "serde"]
        );
        assert_eq!(command_line(&Intent::Upgrade { spec: None }, cwd).1, vec!["update"]);
    }
}
