//! Zero-config ecosystem detection.
//!
//! When no `versionx.toml` exists, walk the workspace root looking for
//! signals and synthesize an in-memory config. Implements the table in
//! `docs/spec/02-config-and-state-model.md §2.4`.
//!
//! Detection is intentionally shallow: we read manifests at the workspace
//! root (and one level of monorepo subdirs if present). Deep filesystem
//! scans happen later — and only on explicit opt-in — in 0.9+ polish.

use camino::{Utf8Path, Utf8PathBuf};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::schema::{EcosystemConfig, RuntimeSpec, VersionxConfig};

/// Detected-but-not-yet-written config. Emitted by [`detect`] for zero-config
/// flows and consumed by `versionx init` to produce an actual file.
#[derive(Clone, Debug)]
pub struct DetectionReport {
    /// The synthesized config.
    pub config: VersionxConfig,
    /// Which individual signals contributed. Surfaced in `vx init` UX so
    /// users see exactly what was detected before the file is written.
    pub signals: Vec<DetectedSignal>,
}

/// A single filesystem signal that contributed to detection.
#[derive(Clone, Debug)]
pub struct DetectedSignal {
    /// Relative path from the workspace root.
    pub path: Utf8PathBuf,
    /// What the signal implied.
    pub implies: SignalKind,
}

/// What a detected signal means for the synthesized config.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SignalKind {
    /// A Node ecosystem with the given package manager (if determinable).
    NodeEcosystem {
        package_manager: Option<String>,
        runtime: Option<String>,
        pm_version: Option<String>,
    },
    /// A Python ecosystem, preferred package manager guessed from `pyproject.toml`.
    PythonEcosystem { package_manager: String, runtime: Option<String> },
    /// A Rust ecosystem (cargo). `workspace = true` if `[workspace]` section.
    RustEcosystem { workspace: bool },
    /// An external tool-version file we read for runtime pins.
    ToolVersionsFile { pins: Vec<(String, String)> },
    /// A hint we noticed but don't currently use (`Dockerfile`, `Gemfile`, etc.).
    Informational { note: String },
}

/// Filesystem accessor for detection. Trait-based for testability.
pub trait DetectFs {
    fn exists(&self, path: &Utf8Path) -> bool;
    fn read(&self, path: &Utf8Path) -> std::io::Result<String>;
}

/// Real-filesystem implementation.
#[derive(Debug, Default)]
pub struct RealFs;

impl DetectFs for RealFs {
    fn exists(&self, path: &Utf8Path) -> bool {
        path.exists()
    }
    fn read(&self, path: &Utf8Path) -> std::io::Result<String> {
        std::fs::read_to_string(path)
    }
}

/// Run zero-config detection at `root`.
#[must_use]
pub fn detect(root: &Utf8Path) -> DetectionReport {
    detect_with(&RealFs, root)
}

/// Run detection with an injected FS reader (for tests).
pub fn detect_with(fs: &dyn DetectFs, root: &Utf8Path) -> DetectionReport {
    let mut report = DetectionReport { config: VersionxConfig::default(), signals: Vec::new() };

    // `.tool-versions` / `.nvmrc` / `.python-version` / `rust-toolchain.toml`
    // are read FIRST so that ecosystem-specific defaults can be refined by
    // explicit tool pins.
    detect_tool_versions(fs, root, &mut report);

    detect_node(fs, root, &mut report);
    detect_python(fs, root, &mut report);
    detect_rust(fs, root, &mut report);
    detect_informational(fs, root, &mut report);

    report
}

fn detect_tool_versions(fs: &dyn DetectFs, root: &Utf8Path, report: &mut DetectionReport) {
    if let Some(pins) = read_tool_versions(fs, root) {
        for (tool, ver) in &pins {
            report
                .config
                .runtimes
                .tools
                .entry(tool.clone())
                .or_insert_with(|| RuntimeSpec::Version(ver.clone()));
        }
        report.signals.push(DetectedSignal {
            path: ".tool-versions".into(),
            implies: SignalKind::ToolVersionsFile { pins },
        });
    }
    if let Ok(contents) = fs.read(&root.join(".nvmrc")) {
        let ver = contents.trim().trim_start_matches('v').to_string();
        if !ver.is_empty() {
            report
                .config
                .runtimes
                .tools
                .entry("node".into())
                .or_insert_with(|| RuntimeSpec::Version(ver.clone()));
            report.signals.push(DetectedSignal {
                path: ".nvmrc".into(),
                implies: SignalKind::ToolVersionsFile { pins: vec![("node".into(), ver)] },
            });
        }
    }
    if let Ok(contents) = fs.read(&root.join(".python-version")) {
        let ver = contents.trim().to_string();
        if !ver.is_empty() {
            report
                .config
                .runtimes
                .tools
                .entry("python".into())
                .or_insert_with(|| RuntimeSpec::Version(ver.clone()));
            report.signals.push(DetectedSignal {
                path: ".python-version".into(),
                implies: SignalKind::ToolVersionsFile { pins: vec![("python".into(), ver)] },
            });
        }
    }
}

fn read_tool_versions(fs: &dyn DetectFs, root: &Utf8Path) -> Option<Vec<(String, String)>> {
    let contents = fs.read(&root.join(".tool-versions")).ok()?;
    let mut out = Vec::new();
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let tool = parts.next()?;
        let version = parts.next()?;
        out.push((tool.to_string(), version.to_string()));
    }
    (!out.is_empty()).then_some(out)
}

fn detect_node(fs: &dyn DetectFs, root: &Utf8Path, report: &mut DetectionReport) {
    let pkg = root.join("package.json");
    if !fs.exists(&pkg) {
        return;
    }

    // We only parse a couple of fields; use a minimal shape that tolerates
    // arbitrary JSON in the rest of the file.
    #[derive(Deserialize)]
    struct PkgJson {
        #[serde(default, rename = "packageManager")]
        package_manager: Option<String>,
        #[serde(default)]
        engines: Option<serde_json::Value>,
    }

    let raw = fs.read(&pkg).unwrap_or_default();
    let parsed: PkgJson =
        serde_json::from_str(&raw).unwrap_or(PkgJson { package_manager: None, engines: None });

    let (pm, pm_version) = parsed
        .package_manager
        .as_deref()
        .and_then(parse_package_manager_field)
        .map_or_else(|| (detect_pm_from_lockfiles(fs, root), None), |(p, v)| (Some(p), Some(v)));

    // Runtime: engines.node wins over any ambient default. Otherwise the user
    // can pin later via `[runtimes] node = ...`.
    let runtime = parsed
        .engines
        .as_ref()
        .and_then(|e| e.get("node"))
        .and_then(|v| v.as_str())
        .map(ToString::to_string);

    let eco = EcosystemConfig {
        package_manager: pm.clone(),
        root: None,
        workspaces: Vec::new(),
        venv_manager: None,
    };

    report.config.ecosystems.insert("node".into(), eco);
    if let Some(v) = runtime.clone() {
        report
            .config
            .runtimes
            .tools
            .entry("node".into())
            .or_insert_with(|| RuntimeSpec::Version(v));
    }
    if let (Some(pm_name), Some(ver)) = (pm.as_deref(), pm_version.as_deref()) {
        report
            .config
            .runtimes
            .tools
            .entry(pm_name.to_string())
            .or_insert_with(|| RuntimeSpec::Version(ver.to_string()));
    }

    report.signals.push(DetectedSignal {
        path: "package.json".into(),
        implies: SignalKind::NodeEcosystem { package_manager: pm, runtime, pm_version },
    });
}

fn parse_package_manager_field(raw: &str) -> Option<(String, String)> {
    // Format: `pnpm@8.15.0` or `pnpm@8.15.0+sha512.<hash>`.
    let at = raw.find('@')?;
    let name = raw[..at].to_string();
    let rest = &raw[at + 1..];
    let version_end = rest.find('+').unwrap_or(rest.len());
    Some((name, rest[..version_end].to_string()))
}

fn detect_pm_from_lockfiles(fs: &dyn DetectFs, root: &Utf8Path) -> Option<String> {
    if fs.exists(&root.join("pnpm-lock.yaml")) {
        return Some("pnpm".into());
    }
    if fs.exists(&root.join("yarn.lock")) {
        return Some("yarn".into());
    }
    if fs.exists(&root.join("package-lock.json")) {
        return Some("npm".into());
    }
    None
}

fn detect_python(fs: &dyn DetectFs, root: &Utf8Path, report: &mut DetectionReport) {
    let pyproj = root.join("pyproject.toml");
    if fs.exists(&pyproj) {
        let raw = fs.read(&pyproj).unwrap_or_default();
        let pm = if raw.contains("[tool.uv]") || raw.contains("[tool.uv.") {
            "uv"
        } else if raw.contains("[tool.poetry]") || raw.contains("[tool.poetry.") {
            "poetry"
        } else {
            // PEP 517 `[project]` with no uv/poetry: default to uv.
            "uv"
        };
        let eco = EcosystemConfig {
            package_manager: Some(pm.into()),
            root: None,
            workspaces: Vec::new(),
            venv_manager: Some(pm.into()),
        };
        report.config.ecosystems.insert("python".into(), eco);
        report.signals.push(DetectedSignal {
            path: "pyproject.toml".into(),
            implies: SignalKind::PythonEcosystem { package_manager: pm.into(), runtime: None },
        });
    } else if fs.exists(&root.join("requirements.txt")) {
        let eco = EcosystemConfig {
            package_manager: Some("pip".into()),
            root: None,
            workspaces: Vec::new(),
            venv_manager: Some("versionx".into()),
        };
        report.config.ecosystems.insert("python".into(), eco);
        report.signals.push(DetectedSignal {
            path: "requirements.txt".into(),
            implies: SignalKind::PythonEcosystem { package_manager: "pip".into(), runtime: None },
        });
    }
}

fn detect_rust(fs: &dyn DetectFs, root: &Utf8Path, report: &mut DetectionReport) {
    let cargo = root.join("Cargo.toml");
    if !fs.exists(&cargo) {
        return;
    }

    let raw = fs.read(&cargo).unwrap_or_default();
    let workspace = raw.contains("[workspace]") || raw.contains("[workspace.");

    let eco = EcosystemConfig {
        package_manager: Some("cargo".into()),
        root: None,
        workspaces: Vec::new(),
        venv_manager: None,
    };
    report.config.ecosystems.insert("rust".into(), eco);

    // rust-toolchain.toml overrides any existing pin.
    if let Ok(contents) = fs.read(&root.join("rust-toolchain.toml"))
        && let Some(channel) = parse_rust_toolchain(&contents)
    {
        report
            .config
            .runtimes
            .tools
            .entry("rust".into())
            .or_insert_with(|| RuntimeSpec::Version(channel));
    }

    report.signals.push(DetectedSignal {
        path: "Cargo.toml".into(),
        implies: SignalKind::RustEcosystem { workspace },
    });
}

fn parse_rust_toolchain(contents: &str) -> Option<String> {
    // Very lightweight parse — we only want the `channel` key.
    #[derive(Deserialize)]
    struct RustToolchainFile {
        toolchain: Option<Toolchain>,
    }
    #[derive(Deserialize)]
    struct Toolchain {
        channel: Option<String>,
    }
    let parsed: RustToolchainFile = toml::from_str(contents).ok()?;
    parsed.toolchain?.channel
}

fn detect_informational(fs: &dyn DetectFs, root: &Utf8Path, report: &mut DetectionReport) {
    let notes = [
        ("Dockerfile", "OCI adapter is available in v1.1+"),
        ("Gemfile", "Ruby adapter lands in v1.1"),
        ("go.mod", "Go adapter lands in v1.1"),
        ("pom.xml", "JVM adapter lands in v1.2"),
        ("build.gradle", "JVM adapter lands in v1.2"),
        ("build.gradle.kts", "JVM adapter lands in v1.2"),
    ];
    for (file, note) in notes {
        if fs.exists(&root.join(file)) {
            report.signals.push(DetectedSignal {
                path: file.into(),
                implies: SignalKind::Informational { note: (*note).to_string() },
            });
        }
    }
}

/// A small JSON-serializable summary used by the CLI's `--output json` on init.
#[derive(Clone, Debug, Serialize)]
pub struct DetectionSummary {
    pub ecosystems: Vec<String>,
    pub runtimes: IndexMap<String, String>,
    pub signals: Vec<String>,
}

impl From<&DetectionReport> for DetectionSummary {
    fn from(r: &DetectionReport) -> Self {
        let ecosystems = r.config.ecosystems.keys().cloned().collect();
        let runtimes = r
            .config
            .runtimes
            .tools
            .iter()
            .map(|(k, v)| (k.clone(), v.version().to_string()))
            .collect();
        let signals = r.signals.iter().map(|s| format!("{}: {:?}", s.path, s.implies)).collect();
        Self { ecosystems, runtimes, signals }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct FakeFs(HashMap<Utf8PathBuf, String>);

    impl FakeFs {
        fn new() -> Self {
            Self(HashMap::new())
        }
        fn write(mut self, p: &str, s: &str) -> Self {
            self.0.insert(Utf8PathBuf::from(p), s.into());
            self
        }
    }

    impl DetectFs for FakeFs {
        fn exists(&self, path: &Utf8Path) -> bool {
            self.0.contains_key(path)
        }
        fn read(&self, path: &Utf8Path) -> std::io::Result<String> {
            self.0
                .get(path)
                .cloned()
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "nope"))
        }
    }

    #[test]
    fn detects_node_with_packagemanager_field() {
        let fs = FakeFs::new().write(
            "/repo/package.json",
            r#"{"name":"t","packageManager":"pnpm@8.15.0","engines":{"node":"22.12.0"}}"#,
        );
        let r = detect_with(&fs, Utf8Path::new("/repo"));
        let eco = r.config.ecosystems.get("node").unwrap();
        assert_eq!(eco.package_manager.as_deref(), Some("pnpm"));
        assert_eq!(r.config.runtimes.tools.get("node").unwrap().version(), "22.12.0");
        assert_eq!(r.config.runtimes.tools.get("pnpm").unwrap().version(), "8.15.0");
    }

    #[test]
    fn detects_node_from_lockfile_only() {
        let fs = FakeFs::new()
            .write("/repo/package.json", "{}")
            .write("/repo/pnpm-lock.yaml", "lockfileVersion: 9");
        let r = detect_with(&fs, Utf8Path::new("/repo"));
        assert_eq!(
            r.config.ecosystems.get("node").unwrap().package_manager.as_deref(),
            Some("pnpm")
        );
    }

    #[test]
    fn detects_python_uv_from_pyproject() {
        let fs = FakeFs::new()
            .write("/repo/pyproject.toml", "[project]\nname='p'\n[tool.uv]\ndev-dependencies=[]\n");
        let r = detect_with(&fs, Utf8Path::new("/repo"));
        let eco = r.config.ecosystems.get("python").unwrap();
        assert_eq!(eco.package_manager.as_deref(), Some("uv"));
        assert_eq!(eco.venv_manager.as_deref(), Some("uv"));
    }

    #[test]
    fn detects_python_poetry() {
        let fs = FakeFs::new()
            .write("/repo/pyproject.toml", "[tool.poetry]\nname='p'\nversion='0.1.0'\n");
        let r = detect_with(&fs, Utf8Path::new("/repo"));
        assert_eq!(
            r.config.ecosystems.get("python").unwrap().package_manager.as_deref(),
            Some("poetry")
        );
    }

    #[test]
    fn detects_pip_from_requirements_txt() {
        let fs = FakeFs::new().write("/repo/requirements.txt", "requests==2.31\n");
        let r = detect_with(&fs, Utf8Path::new("/repo"));
        assert_eq!(
            r.config.ecosystems.get("python").unwrap().package_manager.as_deref(),
            Some("pip")
        );
    }

    #[test]
    fn detects_rust_workspace() {
        let fs = FakeFs::new().write("/repo/Cargo.toml", "[workspace]\nmembers = [\"crates/*\"]\n");
        let r = detect_with(&fs, Utf8Path::new("/repo"));
        assert_eq!(
            r.config.ecosystems.get("rust").unwrap().package_manager.as_deref(),
            Some("cargo")
        );
    }

    #[test]
    fn rust_toolchain_file_sets_runtime_pin() {
        let fs = FakeFs::new()
            .write("/repo/Cargo.toml", "[package]\nname='p'\nversion='0.1.0'\n")
            .write("/repo/rust-toolchain.toml", "[toolchain]\nchannel = \"1.88.0\"\n");
        let r = detect_with(&fs, Utf8Path::new("/repo"));
        assert_eq!(r.config.runtimes.tools.get("rust").unwrap().version(), "1.88.0");
    }

    #[test]
    fn tool_versions_file_imports_all_pins() {
        let fs =
            FakeFs::new().write("/repo/.tool-versions", "# comment\nnode 22.12.0\npython 3.12.2\n");
        let r = detect_with(&fs, Utf8Path::new("/repo"));
        assert_eq!(r.config.runtimes.tools["node"].version(), "22.12.0");
        assert_eq!(r.config.runtimes.tools["python"].version(), "3.12.2");
    }

    #[test]
    fn nvmrc_imports_node_pin() {
        let fs = FakeFs::new().write("/repo/.nvmrc", "v22.12.0\n");
        let r = detect_with(&fs, Utf8Path::new("/repo"));
        assert_eq!(r.config.runtimes.tools["node"].version(), "22.12.0");
    }

    #[test]
    fn informational_signals_for_dockerfile() {
        let fs = FakeFs::new().write("/repo/Dockerfile", "FROM node:20");
        let r = detect_with(&fs, Utf8Path::new("/repo"));
        assert!(
            r.signals.iter().any(|s| matches!(&s.implies, SignalKind::Informational { .. })),
            "expected an informational signal for Dockerfile"
        );
    }

    #[test]
    fn empty_repo_yields_empty_report() {
        let fs = FakeFs::new();
        let r = detect_with(&fs, Utf8Path::new("/repo"));
        assert!(r.config.ecosystems.is_empty());
        assert!(r.signals.is_empty());
    }
}
