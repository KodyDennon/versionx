//! `versionx init` — synthesize a starter `versionx.toml`.
//!
//! Delegates ecosystem detection to `versionx-config::detect` and writes
//! the result through `toml_edit` so the emitted TOML has stable ordering
//! and room to add comments later.

use std::fs;

use camino::{Utf8Path, Utf8PathBuf};
use serde::Serialize;
use versionx_config::detect::{DetectionReport, SignalKind, detect};
use versionx_config::schema::{RuntimeSpec, SUPPORTED_SCHEMA_VERSION, VersionxConfig};

use crate::error::{CoreError, CoreResult};
use crate::{EventSender, Level};

/// Options controlling `init`.
#[derive(Clone, Debug)]
pub struct InitOptions {
    /// Directory to operate in. Typically the user's cwd.
    pub root: Utf8PathBuf,
    /// Allow overwriting an existing `versionx.toml`.
    pub force: bool,
    /// When true, synthesize and return the config without touching disk.
    /// Used by `--output json --plan-only`-style flows.
    pub dry_run: bool,
}

/// Structured outcome, returned to the CLI for rendering + testing.
#[derive(Clone, Debug, Serialize)]
pub struct InitOutcome {
    /// Path the config was (or would be) written to.
    pub path: Utf8PathBuf,
    /// Was a new file created?
    pub created: bool,
    /// Was an existing file overwritten?
    pub overwrote: bool,
    /// What ecosystems are represented in the emitted config.
    pub ecosystems: Vec<String>,
    /// Tool -> version pairs that were pinned.
    pub runtimes: Vec<(String, String)>,
    /// Short human-readable signal descriptions for CLI output.
    pub signals: Vec<String>,
}

/// Run the `init` command.
///
/// # Errors
/// - [`CoreError::ConfigAlreadyExists`] if a config exists and `force` is false.
/// - [`CoreError::NoEcosystemsDetected`] if nothing interesting was found.
/// - [`CoreError::Io`] on filesystem failures.
pub fn init(opts: &InitOptions, events: &EventSender) -> CoreResult<InitOutcome> {
    let config_path = opts.root.join("versionx.toml");

    let exists = config_path.exists();
    if exists && !opts.force && !opts.dry_run {
        return Err(CoreError::ConfigAlreadyExists { path: config_path.to_string() });
    }

    events.info("config.detect.start", format!("scanning {} for ecosystem signals", opts.root));

    let report = detect(&opts.root);

    if report.config.ecosystems.is_empty() && report.config.runtimes.tools.is_empty() {
        return Err(CoreError::NoEcosystemsDetected { path: opts.root.to_string() });
    }

    let rendered = render_versionx_toml(&report.config, &report);

    if !opts.dry_run {
        write_file(&config_path, &rendered)?;
    }

    let outcome = InitOutcome {
        path: config_path,
        created: !exists && !opts.dry_run,
        overwrote: exists && !opts.dry_run,
        ecosystems: report.config.ecosystems.keys().cloned().collect(),
        runtimes: report
            .config
            .runtimes
            .tools
            .iter()
            .map(|(k, v)| (k.clone(), v.version().to_string()))
            .collect(),
        signals: report
            .signals
            .iter()
            .map(|s| format!("{}: {}", s.path, describe_signal(&s.implies)))
            .collect(),
    };

    events.emit(
        crate::Event::new(
            "config.init.complete",
            Level::Info,
            format!(
                "wrote {} ({} ecosystems, {} runtimes)",
                outcome.path,
                outcome.ecosystems.len(),
                outcome.runtimes.len()
            ),
        )
        .with_data(&outcome),
    );

    Ok(outcome)
}

fn write_file(path: &Utf8Path, contents: &str) -> CoreResult<()> {
    if let Some(parent) = path.parent()
        && !parent.as_str().is_empty()
    {
        fs::create_dir_all(parent)
            .map_err(|source| CoreError::Io { path: parent.to_string(), source })?;
    }
    fs::write(path, contents).map_err(|source| CoreError::Io { path: path.to_string(), source })
}

fn describe_signal(kind: &SignalKind) -> String {
    match kind {
        SignalKind::NodeEcosystem { package_manager, runtime, pm_version } => {
            let pm = package_manager.as_deref().unwrap_or("unknown");
            let rt = runtime.as_deref().unwrap_or("any");
            let pmv = pm_version.as_deref().unwrap_or("unpinned");
            format!("node/{pm}@{pmv} with node {rt}")
        }
        SignalKind::PythonEcosystem { package_manager, .. } => {
            format!("python/{package_manager}")
        }
        SignalKind::RustEcosystem { workspace } => {
            if *workspace {
                "rust workspace (cargo)".into()
            } else {
                "rust crate (cargo)".into()
            }
        }
        SignalKind::ToolVersionsFile { pins } => {
            format!(
                "tool pins: {}",
                pins.iter().map(|(t, v)| format!("{t} {v}")).collect::<Vec<_>>().join(", ")
            )
        }
        SignalKind::Informational { note } => note.clone(),
    }
}

/// Render a synthesized config to a nicely-formatted TOML string.
///
/// Doesn't yet use `toml_edit` for round-trip preservation — since this is
/// a new file, we emit plain `toml` with stable key ordering plus a header
/// comment with pointers to the docs.
fn render_versionx_toml(cfg: &VersionxConfig, report: &DetectionReport) -> String {
    use std::fmt::Write as _;

    let mut out = String::new();

    out.push_str("# Versionx project configuration.\n");
    out.push_str("# Generated by `versionx init`. See docs/spec/02-config-and-state-model.md\n");
    out.push_str("# for the full schema. Edit freely; `versionx sync` respects your changes.\n\n");

    // [versionx]
    out.push_str("[versionx]\n");
    let _ = writeln!(out, "schema_version = \"{SUPPORTED_SCHEMA_VERSION}\"");
    if let Some(name) = cfg.versionx.name.as_deref() {
        let _ = writeln!(out, "name = \"{name}\"");
    }
    if cfg.versionx.workspace {
        out.push_str("workspace = true\n");
    }
    out.push('\n');

    // [runtimes]
    if !cfg.runtimes.tools.is_empty() {
        out.push_str("[runtimes]\n");
        for (tool, spec) in &cfg.runtimes.tools {
            match spec {
                RuntimeSpec::Version(v) => {
                    let _ = writeln!(out, "{tool} = \"{v}\"");
                }
                RuntimeSpec::Detailed { version, distribution, channel } => {
                    let mut parts = vec![format!("version = \"{version}\"")];
                    if let Some(d) = distribution {
                        parts.push(format!("distribution = \"{d}\""));
                    }
                    if let Some(c) = channel {
                        parts.push(format!("channel = \"{c}\""));
                    }
                    let _ = writeln!(out, "{tool} = {{ {} }}", parts.join(", "));
                }
            }
        }
        out.push('\n');
    }

    // [ecosystems.*]
    for (name, eco) in &cfg.ecosystems {
        let _ = writeln!(out, "[ecosystems.{name}]");
        if let Some(pm) = &eco.package_manager {
            let _ = writeln!(out, "package_manager = \"{pm}\"");
        }
        if let Some(root) = &eco.root {
            let _ = writeln!(out, "root = \"{root}\"");
        }
        if !eco.workspaces.is_empty() {
            let ws: Vec<String> = eco.workspaces.iter().map(|s| format!("\"{s}\"")).collect();
            let _ = writeln!(out, "workspaces = [{}]", ws.join(", "));
        }
        if let Some(venv) = &eco.venv_manager {
            let _ = writeln!(out, "venv_manager = \"{venv}\"");
        }
        out.push('\n');
    }

    // Trailing comment with informational signals, for user awareness.
    let info_signals: Vec<_> = report
        .signals
        .iter()
        .filter(|s| matches!(s.implies, SignalKind::Informational { .. }))
        .collect();
    if !info_signals.is_empty() {
        out.push_str("# Detected but not yet supported in this Versionx release:\n");
        for s in info_signals {
            if let SignalKind::Informational { note } = &s.implies {
                let _ = writeln!(out, "#   - {}: {note}", s.path);
            }
        }
        out.push('\n');
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use versionx_events::EventBus;

    #[test]
    fn dry_run_does_not_write() {
        let dir = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        fs::write(root.join("package.json"), r#"{"name":"t"}"#).unwrap();

        let bus = EventBus::new();
        let outcome =
            init(&InitOptions { root: root.clone(), force: false, dry_run: true }, &bus.sender())
                .unwrap();

        assert!(!root.join("versionx.toml").exists());
        assert!(!outcome.created);
        assert!(outcome.ecosystems.contains(&"node".to_string()));
    }

    #[test]
    fn writes_config_when_ecosystems_present() {
        let dir = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        fs::write(root.join("package.json"), r#"{"name":"t","packageManager":"pnpm@8.15.0"}"#)
            .unwrap();

        let bus = EventBus::new();
        let outcome =
            init(&InitOptions { root: root.clone(), force: false, dry_run: false }, &bus.sender())
                .unwrap();

        assert!(outcome.created);
        let written = fs::read_to_string(root.join("versionx.toml")).unwrap();
        assert!(written.contains("schema_version = \"1\""));
        assert!(written.contains("package_manager = \"pnpm\""));
        assert!(written.contains("pnpm = \"8.15.0\""));
    }

    #[test]
    fn refuses_to_overwrite_without_force() {
        let dir = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        fs::write(root.join("versionx.toml"), "# existing\n").unwrap();
        fs::write(root.join("package.json"), r#"{"name":"t"}"#).unwrap();

        let bus = EventBus::new();
        let err =
            init(&InitOptions { root, force: false, dry_run: false }, &bus.sender()).unwrap_err();
        assert!(matches!(err, CoreError::ConfigAlreadyExists { .. }));
    }

    #[test]
    fn overwrites_with_force() {
        let dir = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        fs::write(root.join("versionx.toml"), "# old\n").unwrap();
        fs::write(root.join("package.json"), r#"{"name":"t","packageManager":"pnpm@8.15.0"}"#)
            .unwrap();

        let bus = EventBus::new();
        let outcome =
            init(&InitOptions { root: root.clone(), force: true, dry_run: false }, &bus.sender())
                .unwrap();
        assert!(outcome.overwrote);
        let written = fs::read_to_string(root.join("versionx.toml")).unwrap();
        assert!(!written.contains("# old"));
    }

    #[test]
    fn no_ecosystems_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        let bus = EventBus::new();
        let err =
            init(&InitOptions { root, force: false, dry_run: false }, &bus.sender()).unwrap_err();
        assert!(matches!(err, CoreError::NoEcosystemsDetected { .. }));
    }

    #[test]
    fn rust_workspace_is_detected() {
        let dir = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        fs::write(root.join("Cargo.toml"), "[workspace]\nmembers = [\"crates/*\"]\n").unwrap();
        let bus = EventBus::new();
        let outcome =
            init(&InitOptions { root, force: false, dry_run: false }, &bus.sender()).unwrap();
        assert!(outcome.ecosystems.contains(&"rust".to_string()));
    }
}
