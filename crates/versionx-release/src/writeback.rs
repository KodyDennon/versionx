//! Write new versions back into native manifests.
//!
//! Round-trips preserve user formatting + comments:
//!   - Cargo.toml via [`toml_edit::DocumentMut`]
//!   - pyproject.toml via `toml_edit` (supports both `[project]` and
//!     `[tool.poetry]` conventions)
//!   - package.json via `serde_json::Value` (JSON has no comments to
//!     preserve; we keep key order via `serde_json::Map`'s
//!     `preserve_order` feature).
//!
//! The caller provides the component's root dir + kind; this module
//! reads the manifest, updates the `version` field, and writes the file
//! atomically.

use std::fs;

use camino::{Utf8Path, Utf8PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum WriteBackError {
    #[error("io error at {path}: {source}")]
    Io {
        path: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{path}: {message}")]
    Malformed { path: Utf8PathBuf, message: String },
    #[error("{path}: no `version` field could be located")]
    NoVersion { path: Utf8PathBuf },
    #[error("unsupported component kind `{kind}` for write-back at {path}")]
    Unsupported { kind: String, path: Utf8PathBuf },
}

pub type WriteBackResult<T> = Result<T, WriteBackError>;

/// Dispatch to the right writer based on the component kind.
pub fn write_version(
    component_root: &Utf8Path,
    kind: &str,
    new_version: &str,
) -> WriteBackResult<Utf8PathBuf> {
    match kind {
        "rust" => write_cargo(&component_root.join("Cargo.toml"), new_version),
        "node" => write_package_json(&component_root.join("package.json"), new_version),
        "python" => write_pyproject(&component_root.join("pyproject.toml"), new_version),
        other => Err(WriteBackError::Unsupported {
            kind: other.into(),
            path: component_root.to_path_buf(),
        }),
    }
}

// --------- Cargo.toml ----------------------------------------------------

/// Update `[package].version`. Respects `version.workspace = true` —
/// if the crate inherits from the workspace, we update the workspace
/// root's `[workspace.package].version` instead.
pub fn write_cargo(manifest: &Utf8Path, new_version: &str) -> WriteBackResult<Utf8PathBuf> {
    let raw = fs::read_to_string(manifest.as_std_path())
        .map_err(|source| WriteBackError::Io { path: manifest.to_path_buf(), source })?;
    let mut doc: toml_edit::DocumentMut = raw.parse().map_err(|e: toml_edit::TomlError| {
        WriteBackError::Malformed { path: manifest.to_path_buf(), message: e.to_string() }
    })?;

    // Three shapes to handle:
    //   version = "1.2.3"                 → Item::Value(String)
    //   version = { workspace = true }    → Item::Value(InlineTable{workspace})
    //   [package.version] workspace = true → Item::Table{workspace}
    //
    // Only the first writes here. The other two redirect to the
    // workspace root's [workspace.package].version.
    if let Some(package) = doc.get_mut("package").and_then(|v| v.as_table_mut()) {
        if is_workspace_inherited(package.get("version")) {
            let ws_root = find_workspace_root(manifest)?;
            write_cargo_workspace_root(&ws_root, new_version)?;
            return Ok(ws_root);
        }
        if package.contains_key("version") {
            package.insert("version", toml_edit::value(new_version.to_string()));
            write_atomic(manifest, &doc.to_string())?;
            return Ok(manifest.to_path_buf());
        }
    }

    Err(WriteBackError::NoVersion { path: manifest.to_path_buf() })
}

fn is_workspace_inherited(item: Option<&toml_edit::Item>) -> bool {
    match item {
        Some(toml_edit::Item::Table(t)) => t.contains_key("workspace"),
        Some(toml_edit::Item::Value(toml_edit::Value::InlineTable(t))) => {
            t.contains_key("workspace")
        }
        _ => false,
    }
}

/// Walk upward from `child_manifest` until we find a Cargo.toml with a
/// `[workspace]` table.
fn find_workspace_root(child_manifest: &Utf8Path) -> WriteBackResult<Utf8PathBuf> {
    let mut dir = child_manifest
        .parent()
        .map(Utf8Path::to_path_buf)
        .unwrap_or_else(|| panic!("manifest {child_manifest} has no parent"));
    loop {
        // Don't re-check the starting manifest.
        if dir != child_manifest.parent().unwrap_or(child_manifest) {
            let candidate = dir.join("Cargo.toml");
            if candidate.is_file()
                && let Ok(raw) = fs::read_to_string(candidate.as_std_path())
                && let Ok(doc) = raw.parse::<toml_edit::DocumentMut>()
                && doc.get("workspace").is_some()
            {
                return Ok(candidate);
            }
        }
        let Some(parent) = dir.parent() else {
            return Err(WriteBackError::NoVersion { path: child_manifest.to_path_buf() });
        };
        if parent == dir {
            return Err(WriteBackError::NoVersion { path: child_manifest.to_path_buf() });
        }
        dir = parent.to_path_buf();
    }
}

fn write_cargo_workspace_root(root: &Utf8Path, new_version: &str) -> WriteBackResult<()> {
    let raw = fs::read_to_string(root.as_std_path())
        .map_err(|source| WriteBackError::Io { path: root.to_path_buf(), source })?;
    let mut doc: toml_edit::DocumentMut = raw.parse().map_err(|e: toml_edit::TomlError| {
        WriteBackError::Malformed { path: root.to_path_buf(), message: e.to_string() }
    })?;
    let ws = doc.get_mut("workspace").and_then(|v| v.as_table_mut()).ok_or_else(|| {
        WriteBackError::Malformed {
            path: root.to_path_buf(),
            message: "[workspace] table missing".into(),
        }
    })?;
    let pkg = ws
        .entry("package")
        .or_insert(toml_edit::Item::Table(toml_edit::Table::new()))
        .as_table_mut()
        .ok_or_else(|| WriteBackError::Malformed {
            path: root.to_path_buf(),
            message: "[workspace.package] is not a table".into(),
        })?;
    pkg.insert("version", toml_edit::value(new_version.to_string()));
    write_atomic(root, &doc.to_string())
}

// --------- package.json --------------------------------------------------

pub fn write_package_json(manifest: &Utf8Path, new_version: &str) -> WriteBackResult<Utf8PathBuf> {
    let raw = fs::read_to_string(manifest.as_std_path())
        .map_err(|source| WriteBackError::Io { path: manifest.to_path_buf(), source })?;
    // serde_json w/ `preserve_order` preserves insertion order of keys.
    let mut v: serde_json::Value = serde_json::from_str(&raw).map_err(|e| {
        WriteBackError::Malformed { path: manifest.to_path_buf(), message: e.to_string() }
    })?;
    let Some(obj) = v.as_object_mut() else {
        return Err(WriteBackError::Malformed {
            path: manifest.to_path_buf(),
            message: "package.json top-level is not an object".into(),
        });
    };
    obj.insert("version".into(), serde_json::Value::String(new_version.into()));
    // Preserve trailing newline, which npm expects.
    let mut body = serde_json::to_string_pretty(&v).map_err(|e| WriteBackError::Malformed {
        path: manifest.to_path_buf(),
        message: e.to_string(),
    })?;
    body.push('\n');
    write_atomic(manifest, &body)?;
    Ok(manifest.to_path_buf())
}

// --------- pyproject.toml ------------------------------------------------

pub fn write_pyproject(manifest: &Utf8Path, new_version: &str) -> WriteBackResult<Utf8PathBuf> {
    let raw = fs::read_to_string(manifest.as_std_path())
        .map_err(|source| WriteBackError::Io { path: manifest.to_path_buf(), source })?;
    let mut doc: toml_edit::DocumentMut = raw.parse().map_err(|e: toml_edit::TomlError| {
        WriteBackError::Malformed { path: manifest.to_path_buf(), message: e.to_string() }
    })?;

    // Prefer `[project].version` (PEP 621). Fall back to
    // `[tool.poetry].version` for Poetry projects.
    if let Some(project) = doc.get_mut("project").and_then(|v| v.as_table_mut())
        && project.contains_key("version")
    {
        project.insert("version", toml_edit::value(new_version.to_string()));
        write_atomic(manifest, &doc.to_string())?;
        return Ok(manifest.to_path_buf());
    }
    if let Some(poetry) = doc
        .get_mut("tool")
        .and_then(|v| v.as_table_mut())
        .and_then(|t| t.get_mut("poetry"))
        .and_then(|v| v.as_table_mut())
        && poetry.contains_key("version")
    {
        poetry.insert("version", toml_edit::value(new_version.to_string()));
        write_atomic(manifest, &doc.to_string())?;
        return Ok(manifest.to_path_buf());
    }
    Err(WriteBackError::NoVersion { path: manifest.to_path_buf() })
}

// --------- atomic write --------------------------------------------------

fn write_atomic(path: &Utf8Path, body: &str) -> WriteBackResult<()> {
    let tmp = path.with_extension(format!("{}.tmp", path.extension().unwrap_or("write")));
    fs::write(tmp.as_std_path(), body)
        .map_err(|source| WriteBackError::Io { path: tmp.clone(), source })?;
    fs::rename(tmp.as_std_path(), path.as_std_path())
        .map_err(|source| WriteBackError::Io { path: path.to_path_buf(), source })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cargo_inline_version() {
        let tmp = tempfile::tempdir().unwrap();
        let p = Utf8PathBuf::from_path_buf(tmp.path().join("Cargo.toml")).unwrap();
        fs::write(p.as_std_path(), "[package]\nname = \"x\"\nversion = \"0.1.0\"\n").unwrap();
        write_cargo(&p, "0.2.0").unwrap();
        let back = fs::read_to_string(p.as_std_path()).unwrap();
        assert!(back.contains("version = \"0.2.0\""));
    }

    #[test]
    fn cargo_workspace_inheritance_writes_root() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"pkg\"]\n[workspace.package]\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::create_dir_all(root.join("pkg")).unwrap();
        fs::write(
            root.join("pkg/Cargo.toml"),
            "[package]\nname = \"pkg\"\nversion.workspace = true\n",
        )
        .unwrap();
        write_cargo(&root.join("pkg/Cargo.toml"), "0.5.0").unwrap();
        let root_cargo = fs::read_to_string(root.join("Cargo.toml")).unwrap();
        assert!(root_cargo.contains("version = \"0.5.0\""));
    }

    #[test]
    fn package_json_update() {
        let tmp = tempfile::tempdir().unwrap();
        let p = Utf8PathBuf::from_path_buf(tmp.path().join("package.json")).unwrap();
        fs::write(
            p.as_std_path(),
            "{\n  \"name\": \"x\",\n  \"version\": \"0.1.0\",\n  \"scripts\": {}\n}\n",
        )
        .unwrap();
        write_package_json(&p, "1.0.0").unwrap();
        let back = fs::read_to_string(p.as_std_path()).unwrap();
        assert!(back.contains("\"version\": \"1.0.0\""));
        assert!(back.contains("\"scripts\""));
    }

    #[test]
    fn pyproject_project_section() {
        let tmp = tempfile::tempdir().unwrap();
        let p = Utf8PathBuf::from_path_buf(tmp.path().join("pyproject.toml")).unwrap();
        fs::write(p.as_std_path(), "[project]\nname = \"x\"\nversion = \"0.1.0\"\n").unwrap();
        write_pyproject(&p, "0.2.0").unwrap();
        let back = fs::read_to_string(p.as_std_path()).unwrap();
        assert!(back.contains("version = \"0.2.0\""));
    }

    #[test]
    fn pyproject_poetry_fallback() {
        let tmp = tempfile::tempdir().unwrap();
        let p = Utf8PathBuf::from_path_buf(tmp.path().join("pyproject.toml")).unwrap();
        fs::write(p.as_std_path(), "[tool.poetry]\nname = \"x\"\nversion = \"0.1.0\"\n").unwrap();
        write_pyproject(&p, "0.2.0").unwrap();
        let back = fs::read_to_string(p.as_std_path()).unwrap();
        assert!(back.contains("version = \"0.2.0\""));
    }

    #[test]
    fn dispatch_by_kind() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        fs::write(root.join("Cargo.toml"), "[package]\nname = \"x\"\nversion = \"0.1.0\"\n")
            .unwrap();
        write_version(&root, "rust", "9.9.9").unwrap();
        let back = fs::read_to_string(root.join("Cargo.toml")).unwrap();
        assert!(back.contains("9.9.9"));
    }
}
