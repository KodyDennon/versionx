//! Walk a workspace root + read explicit `[[components]]` entries, then
//! stitch together a [`Workspace`] with every component + native-PM
//! dependency edges.

use std::collections::BTreeSet;

use camino::Utf8Path;
use indexmap::IndexMap;
use serde::Deserialize;

use crate::error::{WorkspaceError, WorkspaceResult};
use crate::model::{Component, ComponentId, ComponentKind, ComponentSource, Workspace};

/// Max directory depth to walk for auto-discovery. Keeps scans fast in
/// repos that have huge unrelated trees (artifacts, submodules).
const MAX_DEPTH: usize = 8;

/// Default input globs applied to every component.
const DEFAULT_INPUTS: &[&str] = &["**/*"];

/// Top-level entry point. Walks `root` for manifests, then reads
/// `[[components]]` entries from `versionx.toml` (if present) for extras.
///
/// # Errors
/// Any filesystem error during the scan propagates as [`WorkspaceError::Io`].
pub fn discover(root: &Utf8Path) -> WorkspaceResult<Workspace> {
    if !root.is_dir() {
        return Err(WorkspaceError::RootMissing { path: root.to_path_buf() });
    }

    let mut components: IndexMap<ComponentId, Component> = IndexMap::new();

    // 1. Auto-discovery via manifests.
    walk_for_manifests(root, root, 0, &mut components)?;

    // 2. Explicit [[components]] in versionx.toml.
    let versionx_toml = root.join("versionx.toml");
    if versionx_toml.is_file() {
        merge_declared(&versionx_toml, &mut components)?;
    }

    // 3. Stitch dependency edges from native PM declarations.
    link_native_deps(&mut components);

    Ok(Workspace { root: root.to_path_buf(), components })
}

#[allow(clippy::branches_sharing_code, clippy::useless_let_if_seq)] // per-manifest flow kept readable.
fn walk_for_manifests(
    root: &Utf8Path,
    dir: &Utf8Path,
    depth: usize,
    out: &mut IndexMap<ComponentId, Component>,
) -> WorkspaceResult<()> {
    if depth > MAX_DEPTH {
        return Ok(());
    }

    let entries = match std::fs::read_dir(dir.as_std_path()) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => return Ok(()),
        Err(source) => return Err(WorkspaceError::Io { path: dir.to_path_buf(), source }),
    };

    // Manifest detection in this specific dir.
    let package_json = dir.join("package.json");
    let cargo_toml = dir.join("Cargo.toml");
    let pyproject = dir.join("pyproject.toml");
    let gomod = dir.join("go.mod");
    let gemfile = dir.join("Gemfile");
    let pom = dir.join("pom.xml");

    let mut discovered_here: Option<ComponentKind> = None;
    if package_json.is_file()
        && let Some(c) = read_node_component(&package_json, root)?
    {
        out.insert(c.id.clone(), c);
        discovered_here = Some(ComponentKind::Node);
    }
    if cargo_toml.is_file()
        && let Some(c) = read_rust_component(&cargo_toml, root)?
    {
        out.insert(c.id.clone(), c);
        discovered_here = Some(ComponentKind::Rust);
    }
    if pyproject.is_file()
        && let Some(c) = read_python_component(&pyproject, root)?
    {
        out.insert(c.id.clone(), c);
        discovered_here = Some(ComponentKind::Python);
    }
    if gomod.is_file()
        && let Some(c) = read_generic_component(&gomod, root, ComponentKind::Go)?
    {
        out.insert(c.id.clone(), c);
        discovered_here = Some(ComponentKind::Go);
    }
    if gemfile.is_file()
        && let Some(c) = read_generic_component(&gemfile, root, ComponentKind::Ruby)?
    {
        out.insert(c.id.clone(), c);
        discovered_here = Some(ComponentKind::Ruby);
    }
    if pom.is_file()
        && let Some(c) = read_generic_component(&pom, root, ComponentKind::Jvm)?
    {
        out.insert(c.id.clone(), c);
        discovered_here = Some(ComponentKind::Jvm);
    }

    // Recurse into subdirectories, skipping noise.
    for entry in entries.flatten() {
        let Some(entry_path) = camino::Utf8PathBuf::from_path_buf(entry.path()).ok() else {
            continue;
        };
        let Some(name) = entry_path.file_name() else { continue };
        if should_skip_dir(name, discovered_here.as_ref()) {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        if meta.is_dir() {
            walk_for_manifests(root, &entry_path, depth + 1, out)?;
        }
    }

    Ok(())
}

fn should_skip_dir(name: &str, _in_component: Option<&ComponentKind>) -> bool {
    matches!(
        name,
        "target"
            | "node_modules"
            | ".venv"
            | "venv"
            | "__pycache__"
            | ".pytest_cache"
            | "dist"
            | "build"
            | ".git"
            | ".gradle"
            | ".idea"
            | ".vscode"
            | ".cache"
            | ".versionx"
    )
}

// --------- manifest readers -----------------------------------------------

#[derive(Deserialize)]
struct PackageJson {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    dependencies: IndexMap<String, String>,
    #[serde(default, rename = "devDependencies")]
    dev_dependencies: IndexMap<String, String>,
    #[serde(default, rename = "peerDependencies")]
    peer_dependencies: IndexMap<String, String>,
    #[serde(default, rename = "optionalDependencies")]
    optional_dependencies: IndexMap<String, String>,
    #[allow(dead_code)]
    #[serde(default)]
    workspaces: serde_json::Value,
}

fn read_node_component(
    manifest: &Utf8Path,
    _root: &Utf8Path,
) -> WorkspaceResult<Option<Component>> {
    let raw = std::fs::read_to_string(manifest)
        .map_err(|source| WorkspaceError::Io { path: manifest.to_path_buf(), source })?;
    let parsed: PackageJson = serde_json::from_str(&raw).map_err(|e| {
        WorkspaceError::ManifestParse { path: manifest.to_path_buf(), message: e.to_string() }
    })?;

    let Some(name) = parsed.name else {
        // Anonymous package.json (rare, but e.g. workspace roots) — skip.
        return Ok(None);
    };
    let dir = manifest.parent().unwrap_or(manifest).to_path_buf();
    let version = parsed.version.and_then(|v| semver::Version::parse(&v).ok());

    // Native Node deps that use `workspace:*` or `file:...` or `link:...`
    // are intra-workspace refs.
    let mut deps: BTreeSet<ComponentId> = BTreeSet::new();
    for dep_map in [
        &parsed.dependencies,
        &parsed.dev_dependencies,
        &parsed.peer_dependencies,
        &parsed.optional_dependencies,
    ] {
        for (dep_name, dep_spec) in dep_map {
            if is_workspace_ref(dep_spec) {
                deps.insert(ComponentId::new(dep_name));
            }
        }
    }

    Ok(Some(Component {
        id: ComponentId::new(&name),
        display_name: name,
        root: dir,
        kind: ComponentKind::Node,
        source: ComponentSource::Manifest { manifest_path: manifest.to_path_buf() },
        version,
        inputs: DEFAULT_INPUTS.iter().map(|s| (*s).to_string()).collect(),
        depends_on: deps,
    }))
}

fn is_workspace_ref(spec: &str) -> bool {
    spec.starts_with("workspace:")
        || spec.starts_with("file:")
        || spec.starts_with("link:")
        || spec.starts_with("portal:")
}

fn read_rust_component(manifest: &Utf8Path, root: &Utf8Path) -> WorkspaceResult<Option<Component>> {
    let raw = std::fs::read_to_string(manifest)
        .map_err(|source| WorkspaceError::Io { path: manifest.to_path_buf(), source })?;
    let doc: toml::Value = toml::from_str(&raw).map_err(|e| WorkspaceError::ManifestParse {
        path: manifest.to_path_buf(),
        message: e.to_string(),
    })?;

    let dir = manifest.parent().unwrap_or(manifest).to_path_buf();

    // Skip pure workspace roots (no [package], just [workspace]).
    let Some(package) = doc.get("package").and_then(|v| v.as_table()) else {
        return Ok(None);
    };
    let Some(name) = package.get("name").and_then(|v| v.as_str()) else {
        return Ok(None);
    };
    let version = package
        .get("version")
        .and_then(|v| v.as_str())
        .and_then(|v| semver::Version::parse(v).ok());

    // Intra-workspace deps are declared with `path = "..."` in Cargo.
    let mut deps: BTreeSet<ComponentId> = BTreeSet::new();
    for key in ["dependencies", "dev-dependencies", "build-dependencies"] {
        let Some(deps_tbl) = doc.get(key).and_then(|v| v.as_table()) else {
            continue;
        };
        for (dep_name, dep_val) in deps_tbl {
            let is_path = match dep_val {
                toml::Value::Table(t) => t.contains_key("path") || t.contains_key("workspace"),
                _ => false,
            };
            if is_path {
                deps.insert(ComponentId::new(dep_name));
            }
        }
    }

    let display = name.to_string();
    Ok(Some(Component {
        id: ComponentId::new(&display),
        display_name: display,
        root: dir,
        kind: ComponentKind::Rust,
        source: ComponentSource::Manifest { manifest_path: manifest.to_path_buf() },
        version,
        inputs: DEFAULT_INPUTS.iter().map(|s| (*s).to_string()).collect(),
        depends_on: deps,
    }))
    .map(|c| strip_root_prefix(c, root))
}

fn read_python_component(
    manifest: &Utf8Path,
    root: &Utf8Path,
) -> WorkspaceResult<Option<Component>> {
    let raw = std::fs::read_to_string(manifest)
        .map_err(|source| WorkspaceError::Io { path: manifest.to_path_buf(), source })?;
    let doc: toml::Value = toml::from_str(&raw).map_err(|e| WorkspaceError::ManifestParse {
        path: manifest.to_path_buf(),
        message: e.to_string(),
    })?;

    let dir = manifest.parent().unwrap_or(manifest).to_path_buf();
    let project = doc.get("project").and_then(|v| v.as_table());
    let poetry = doc.get("tool").and_then(|t| t.get("poetry")).and_then(|v| v.as_table());

    let name = project
        .and_then(|p| p.get("name").and_then(|v| v.as_str()))
        .or_else(|| poetry.and_then(|p| p.get("name").and_then(|v| v.as_str())));
    let Some(name) = name else {
        return Ok(None);
    };

    let version = project
        .and_then(|p| p.get("version").and_then(|v| v.as_str()))
        .or_else(|| poetry.and_then(|p| p.get("version").and_then(|v| v.as_str())))
        .and_then(|v| semver::Version::parse(v).ok());

    // Intra-workspace deps: uv workspace members live in
    // `[tool.uv.sources]` with `workspace = true` or `path = "..."`.
    let mut deps: BTreeSet<ComponentId> = BTreeSet::new();
    if let Some(sources) = doc
        .get("tool")
        .and_then(|t| t.get("uv"))
        .and_then(|u| u.get("sources"))
        .and_then(|v| v.as_table())
    {
        for (dep_name, dep_val) in sources {
            let is_workspace_dep = match dep_val {
                toml::Value::Table(t) => {
                    t.get("workspace").and_then(toml::Value::as_bool).unwrap_or(false)
                        || t.contains_key("path")
                }
                _ => false,
            };
            if is_workspace_dep {
                deps.insert(ComponentId::new(dep_name));
            }
        }
    }

    let display = name.to_string();
    Ok(Some(Component {
        id: ComponentId::new(&display),
        display_name: display,
        root: dir,
        kind: ComponentKind::Python,
        source: ComponentSource::Manifest { manifest_path: manifest.to_path_buf() },
        version,
        inputs: DEFAULT_INPUTS.iter().map(|s| (*s).to_string()).collect(),
        depends_on: deps,
    }))
    .map(|c| strip_root_prefix(c, root))
}

#[allow(clippy::unnecessary_wraps)] // symmetry with the other `read_*_component` readers.
fn read_generic_component(
    manifest: &Utf8Path,
    root: &Utf8Path,
    kind: ComponentKind,
) -> WorkspaceResult<Option<Component>> {
    // For Go / Ruby / JVM we don't parse the manifest for intra-repo deps in
    // v0.x — we only record the component + its directory. Native
    // workspace detection for these languages is a 0.2+ polish item.
    let dir = manifest.parent().unwrap_or(manifest).to_path_buf();
    let id = dir
        .strip_prefix(root)
        .ok()
        .and_then(|s| if s.as_str().is_empty() { None } else { Some(s.as_str().to_string()) })
        .unwrap_or_else(|| kind.as_str().to_string());
    Ok(Some(Component {
        id: ComponentId::new(&id),
        display_name: id,
        root: dir,
        kind,
        source: ComponentSource::Manifest { manifest_path: manifest.to_path_buf() },
        version: None,
        inputs: DEFAULT_INPUTS.iter().map(|s| (*s).to_string()).collect(),
        depends_on: BTreeSet::new(),
    }))
}

/// Leave component.root absolute; no-op today. Kept as a hook so we can
/// switch to root-relative paths on-disk without touching callers.
const fn strip_root_prefix(component: Option<Component>, _root: &Utf8Path) -> Option<Component> {
    component
}

// --------- explicit [[components]] ----------------------------------------

/// Pull `[[components]]` out of a `versionx.toml` using the authoritative
/// schema type. We deliberately *don't* run env interpolation here — paths
/// and names inside `[[components]]` don't need it, and we want discovery
/// to work before the full config loader has run.
fn parse_declared(raw: &str) -> Result<Vec<versionx_config::ComponentConfig>, toml::de::Error> {
    #[derive(serde::Deserialize)]
    struct OnlyComponents {
        #[serde(default)]
        components: Vec<versionx_config::ComponentConfig>,
    }
    let parsed: OnlyComponents = toml::from_str(raw)?;
    Ok(parsed.components)
}

fn merge_declared(
    versionx_toml: &Utf8Path,
    out: &mut IndexMap<ComponentId, Component>,
) -> WorkspaceResult<()> {
    let raw = std::fs::read_to_string(versionx_toml)
        .map_err(|source| WorkspaceError::Io { path: versionx_toml.to_path_buf(), source })?;
    // Use the typed config schema so declared [[components]] stay in one
    // authoritative place. Tolerate parse errors here — unrelated schema
    // problems shouldn't block workspace discovery.
    let Ok(cfg) = parse_declared(&raw) else {
        return Ok(());
    };

    let root = versionx_toml.parent().unwrap_or(versionx_toml).to_path_buf();
    for entry in cfg {
        let id = ComponentId::new(&entry.name);
        // Declared entries win over auto-discovered with the same name,
        // so user can override auto-detected specifics.
        let kind = match entry.kind.as_deref() {
            Some("node") => ComponentKind::Node,
            Some("python") => ComponentKind::Python,
            Some("rust") => ComponentKind::Rust,
            Some("go") => ComponentKind::Go,
            Some("ruby") => ComponentKind::Ruby,
            Some("jvm") => ComponentKind::Jvm,
            Some("oci") => ComponentKind::Oci,
            Some(other) => ComponentKind::Other { label: other.to_string() },
            None => ComponentKind::Other { label: "declared".into() },
        };
        let abs_root = root.join(&entry.path);
        let version = entry.version.and_then(|v| semver::Version::parse(&v).ok());
        let deps: BTreeSet<ComponentId> =
            entry.depends_on.into_iter().map(ComponentId::new).collect();
        let inputs = if entry.inputs.is_empty() {
            DEFAULT_INPUTS.iter().map(|s| (*s).to_string()).collect()
        } else {
            entry.inputs
        };
        out.insert(
            id.clone(),
            Component {
                id,
                display_name: entry.name,
                root: abs_root,
                kind,
                source: ComponentSource::Declared,
                version,
                inputs,
                depends_on: deps,
            },
        );
    }
    Ok(())
}

// --------- native-PM dep linking ------------------------------------------

/// After all components are collected, normalize intra-workspace deps.
/// Currently this just strips deps that reference names not present in the
/// workspace (e.g. a pnpm `workspace:*` pointing at a package not actually
/// in this repo).
fn link_native_deps(components: &mut IndexMap<ComponentId, Component>) {
    let known: BTreeSet<ComponentId> = components.keys().cloned().collect();
    for component in components.values_mut() {
        component.depends_on.retain(|d| known.contains(d));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmp() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn discovers_single_node_component() {
        let dir = tmp();
        let root = camino::Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        fs::write(root.join("package.json"), r#"{"name":"@acme/ui","version":"1.2.3"}"#).unwrap();
        let ws = discover(&root).unwrap();
        assert_eq!(ws.len(), 1);
        let c = ws.get(&ComponentId::new("@acme/ui")).unwrap();
        assert_eq!(c.kind, ComponentKind::Node);
        assert_eq!(c.version.as_ref().unwrap().to_string(), "1.2.3");
    }

    #[test]
    fn discovers_monorepo_with_workspace_deps() {
        let dir = tmp();
        let root = camino::Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        fs::create_dir_all(root.join("apps/app")).unwrap();
        fs::create_dir_all(root.join("packages/ui")).unwrap();
        fs::write(
            root.join("apps/app/package.json"),
            r#"{"name":"app","version":"0.1.0","dependencies":{"@acme/ui":"workspace:*"}}"#,
        )
        .unwrap();
        fs::write(
            root.join("packages/ui/package.json"),
            r#"{"name":"@acme/ui","version":"0.1.0"}"#,
        )
        .unwrap();
        let ws = discover(&root).unwrap();
        assert_eq!(ws.len(), 2);
        let app = ws.get(&ComponentId::new("app")).unwrap();
        assert!(app.depends_on.contains(&ComponentId::new("@acme/ui")));
    }

    #[test]
    fn discovers_rust_path_deps() {
        let dir = tmp();
        let root = camino::Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        fs::create_dir_all(root.join("crates/core")).unwrap();
        fs::create_dir_all(root.join("crates/cli")).unwrap();
        fs::write(
            root.join("crates/core/Cargo.toml"),
            "[package]\nname = \"core\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/cli/Cargo.toml"),
            "[package]\nname = \"cli\"\nversion = \"0.1.0\"\n[dependencies]\ncore = { path = \"../core\" }\n",
        )
        .unwrap();
        let ws = discover(&root).unwrap();
        assert_eq!(ws.len(), 2);
        let cli = ws.get(&ComponentId::new("cli")).unwrap();
        assert!(cli.depends_on.contains(&ComponentId::new("core")));
    }

    #[test]
    fn merges_declared_components() {
        let dir = tmp();
        let root = camino::Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        fs::create_dir_all(root.join("protocols/chat")).unwrap();
        fs::write(root.join("protocols/chat/chat.proto"), "syntax=\"proto3\";").unwrap();
        fs::write(
            root.join("versionx.toml"),
            r#"
[[components]]
name = "chat-protocol"
path = "protocols/chat"
kind = "other"
version = "0.5.0"
inputs = ["**/*.proto"]
"#,
        )
        .unwrap();
        let ws = discover(&root).unwrap();
        let c = ws.get(&ComponentId::new("chat-protocol")).unwrap();
        assert_eq!(c.inputs, vec!["**/*.proto"]);
        assert!(matches!(&c.kind, ComponentKind::Other { label } if label == "other"));
    }

    #[test]
    fn skip_noise_dirs() {
        let dir = tmp();
        let root = camino::Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        fs::create_dir_all(root.join("node_modules/foo")).unwrap();
        fs::write(
            root.join("node_modules/foo/package.json"),
            r#"{"name":"foo","version":"0.0.1"}"#,
        )
        .unwrap();
        let ws = discover(&root).unwrap();
        assert!(ws.is_empty(), "node_modules contents should not be discovered");
    }
}
