use super::*;
use toml_edit::{DocumentMut, Item, Table, value};

#[derive(Copy, Clone, Debug, ValueEnum)]
pub(crate) enum ImportSource {
    Mise,
    Asdf,
    Nvm,
    Poetry,
    Changesets,
    ReleasePlease,
}

/// `versionx import` — detect a sibling tool or release config and seed
/// `versionx.toml`. Supports mise / asdf / nvm / poetry plus release-config
/// migration for changesets and release-please.
pub(crate) fn run_import(
    from: Option<ImportSource>,
    cwd: Option<&camino::Utf8Path>,
    output: OutputFormat,
) -> Result<ExitCode> {
    let root = resolve_cwd(cwd)?;
    let detected = from.or_else(|| detect_import_source(&root));
    let Some(source) = detected else {
        return bail_with(
            output,
            "import",
            "no recognized config found (looked for .mise.toml, .tool-versions, .nvmrc, pyproject.toml, .changeset/config.json, .release-please-config.json)",
        );
    };

    match source {
        ImportSource::Mise => write_toolchain_import(&root, source, parse_mise(&root), output)?,
        ImportSource::Asdf => write_toolchain_import(&root, source, parse_asdf(&root), output)?,
        ImportSource::Nvm => write_toolchain_import(&root, source, parse_nvmrc(&root), output)?,
        ImportSource::Poetry => {
            write_toolchain_import(&root, source, parse_pyproject(&root), output)?
        }
        ImportSource::Changesets => migrate_changesets_release_config(&root, output)?,
        ImportSource::ReleasePlease => migrate_release_please_config(&root, output)?,
    };
    Ok(ExitCode::from(0))
}

fn write_toolchain_import(
    root: &camino::Utf8Path,
    source: ImportSource,
    pins: Vec<(String, String)>,
    output: OutputFormat,
) -> Result<()> {
    let mut versionx_toml_path = root.join("versionx.toml");
    if versionx_toml_path.is_file() {
        versionx_toml_path = root.join("versionx.imported.toml");
    }

    let mut body =
        String::from("# Imported by `versionx import`\nschema_version = \"1\"\n\n[runtimes]\n");
    for (tool, version) in &pins {
        use std::fmt::Write;
        let _ = writeln!(body, "{tool} = \"{version}\"");
    }
    std::fs::write(versionx_toml_path.as_std_path(), body)
        .context("writing imported versionx.toml")?;
    emit_msg(
        output,
        &format!("imported {} pins from {source:?} → {versionx_toml_path}", pins.len()),
        serde_json::json!({"source": format!("{source:?}"), "path": versionx_toml_path.to_string(), "pins": pins}),
    )?;
    Ok(())
}

pub(crate) fn detect_import_source(root: &camino::Utf8Path) -> Option<ImportSource> {
    if root.join(".mise.toml").is_file() || root.join("mise.toml").is_file() {
        return Some(ImportSource::Mise);
    }
    if root.join(".tool-versions").is_file() {
        return Some(ImportSource::Asdf);
    }
    if root.join(".nvmrc").is_file() {
        return Some(ImportSource::Nvm);
    }
    if root.join("pyproject.toml").is_file() {
        return Some(ImportSource::Poetry);
    }
    if root.join(".changeset").join("config.json").is_file() {
        return Some(ImportSource::Changesets);
    }
    if root.join(".release-please-config.json").is_file() {
        return Some(ImportSource::ReleasePlease);
    }
    None
}

fn parse_mise(root: &camino::Utf8Path) -> Vec<(String, String)> {
    for name in [".mise.toml", "mise.toml"] {
        if let Ok(raw) = std::fs::read_to_string(root.join(name).as_std_path())
            && let Ok(val) = toml::from_str::<toml::Value>(&raw)
            && let Some(tbl) = val.get("tools").and_then(|v| v.as_table())
        {
            return tbl
                .iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect();
        }
    }
    Vec::new()
}

fn parse_asdf(root: &camino::Utf8Path) -> Vec<(String, String)> {
    let Ok(raw) = std::fs::read_to_string(root.join(".tool-versions").as_std_path()) else {
        return Vec::new();
    };
    raw.lines()
        .filter_map(|line| {
            let line = line.split('#').next()?.trim();
            if line.is_empty() {
                return None;
            }
            let mut parts = line.split_whitespace();
            let tool = parts.next()?.to_string();
            let version = parts.next()?.to_string();
            Some((tool, version))
        })
        .collect()
}

fn parse_nvmrc(root: &camino::Utf8Path) -> Vec<(String, String)> {
    std::fs::read_to_string(root.join(".nvmrc").as_std_path())
        .ok()
        .map(|s| vec![("node".into(), s.trim().trim_start_matches('v').to_string())])
        .unwrap_or_default()
}

fn parse_pyproject(root: &camino::Utf8Path) -> Vec<(String, String)> {
    let Ok(raw) = std::fs::read_to_string(root.join("pyproject.toml").as_std_path()) else {
        return Vec::new();
    };
    let Ok(val) = toml::from_str::<toml::Value>(&raw) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if let Some(py) = val
        .get("tool")
        .and_then(|v| v.get("poetry"))
        .and_then(|v| v.get("dependencies"))
        .and_then(|v| v.get("python"))
        .and_then(|v| v.as_str())
    {
        out.push(("python".into(), py.trim_start_matches('^').trim_start_matches('~').to_string()));
    }
    out
}

fn migrate_changesets_release_config(
    root: &camino::Utf8Path,
    output: OutputFormat,
) -> Result<()> {
    let cfg_path = root.join(".changeset").join("config.json");
    let raw = std::fs::read_to_string(cfg_path.as_std_path())
        .with_context(|| format!("reading {}", cfg_path))?;
    let cfg: serde_json::Value =
        serde_json::from_str(&raw).with_context(|| format!("parsing {}", cfg_path))?;

    let mut applied = vec!["release.strategy = \"changesets\"".to_string()];
    let mut warnings = Vec::new();
    let path = upsert_release_config(root, |release| {
        release["strategy"] = value("changesets");
    })?;

    if let Some(linked) = cfg.get("linked").and_then(serde_json::Value::as_array)
        && !linked.is_empty()
    {
        warnings.push(
            "changesets `linked` groups still need manual translation to `[[release.groups]]`."
                .to_string(),
        );
    }
    if let Some(ignore) = cfg.get("ignore").and_then(serde_json::Value::as_array)
        && !ignore.is_empty()
    {
        warnings.push(
            "changesets `ignore` packages are not represented in the current shipped schema."
                .to_string(),
        );
    }
    for key in ["baseBranch", "access", "commit", "updateInternalDependencies"] {
        if cfg.get(key).is_some() {
            warnings.push(format!("changesets `{key}` requires manual review after migration."));
        }
    }
    if root.join(".changeset").is_dir() {
        applied.push("existing .changeset/ files left in place".to_string());
    }

    emit_msg(
        output,
        &format!("migrated changesets release settings into {path}"),
        serde_json::json!({
            "source": "changesets",
            "path": path.to_string(),
            "applied": applied,
            "warnings": warnings,
        }),
    )?;
    if matches!(output, OutputFormat::Human) {
        for warning in warnings {
            println!("  warning: {warning}");
        }
    }
    Ok(())
}

fn migrate_release_please_config(root: &camino::Utf8Path, output: OutputFormat) -> Result<()> {
    let cfg_path = root.join(".release-please-config.json");
    let raw = std::fs::read_to_string(cfg_path.as_std_path())
        .with_context(|| format!("reading {}", cfg_path))?;
    let cfg: serde_json::Value =
        serde_json::from_str(&raw).with_context(|| format!("parsing {}", cfg_path))?;

    let mut applied = vec!["release.strategy = \"pr-title\"".to_string()];
    let mut warnings = Vec::new();
    let changelog = release_please_changelog_path(&cfg);
    let include_component = release_please_include_component_in_tag(&cfg);
    let path = upsert_release_config(root, |release| {
        release["strategy"] = value("pr-title");
        if let Some(changelog) = &changelog {
            release["changelog"] = value(changelog.clone());
        }
        if include_component {
            release["tag_template"] = value("{package}-v{version}");
        }
    })?;

    if let Some(changelog) = changelog {
        applied.push(format!("release.changelog = {changelog:?}"));
    }
    if include_component {
        applied.push("release.tag_template = \"{package}-v{version}\"".to_string());
    }
    if cfg.get("packages").and_then(serde_json::Value::as_object).is_some() {
        warnings.push(
            "per-package release-please config still needs manual translation into detected components and package overrides."
                .to_string(),
        );
    }
    for key in [
        "bump-minor-pre-major",
        "bump-patch-for-minor-pre-major",
        "extra-files",
        "release-type",
    ] {
        if cfg.get(key).is_some() {
            warnings.push(format!("release-please `{key}` requires manual review after migration."));
        }
    }
    if root.join(".release-please-manifest.json").is_file() {
        warnings.push(
            ".release-please-manifest.json was left untouched; Versionx does not use it.".to_string(),
        );
    }

    emit_msg(
        output,
        &format!("migrated release-please settings into {path}"),
        serde_json::json!({
            "source": "release-please",
            "path": path.to_string(),
            "applied": applied,
            "warnings": warnings,
        }),
    )?;
    if matches!(output, OutputFormat::Human) {
        for warning in warnings {
            println!("  warning: {warning}");
        }
    }
    Ok(())
}

fn upsert_release_config(
    root: &camino::Utf8Path,
    mutate: impl FnOnce(&mut Table),
) -> Result<Utf8PathBuf> {
    let path = root.join("versionx.toml");
    let mut doc = if path.is_file() {
        let raw = std::fs::read_to_string(path.as_std_path())
            .with_context(|| format!("reading {}", path))?;
        raw.parse::<DocumentMut>().with_context(|| format!("parsing {}", path))?
    } else {
        DocumentMut::new()
    };

    let root_table = doc.as_table_mut();
    if !root_table.contains_key("versionx") {
        root_table.insert("versionx", Item::Table(Table::new()));
    }
    let versionx = root_table
        .get_mut("versionx")
        .and_then(Item::as_table_mut)
        .context("internal error: versionx section was not a table")?;
    versionx["schema_version"] = value("1");

    if !root_table.contains_key("release") {
        root_table.insert("release", Item::Table(Table::new()));
    }
    let release = root_table
        .get_mut("release")
        .and_then(Item::as_table_mut)
        .context("internal error: release section was not a table")?;
    mutate(release);

    std::fs::write(path.as_std_path(), doc.to_string())
        .with_context(|| format!("writing {}", path))?;
    Ok(path)
}

fn release_please_changelog_path(cfg: &serde_json::Value) -> Option<String> {
    cfg.get("changelog-path")
        .and_then(serde_json::Value::as_str)
        .map(ToString::to_string)
        .or_else(|| {
            cfg.get("packages")
                .and_then(serde_json::Value::as_object)
                .and_then(|packages| {
                    let mut paths = packages.values().filter_map(|pkg| {
                        pkg.get("changelog-path")
                            .and_then(serde_json::Value::as_str)
                            .map(ToString::to_string)
                    });
                    let first = paths.next()?;
                    if paths.all(|path| path == first) {
                        Some(first)
                    } else {
                        None
                    }
                })
        })
}

fn release_please_include_component_in_tag(cfg: &serde_json::Value) -> bool {
    cfg.get("include-component-in-tag")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn detect_import_source_finds_changesets_and_release_please() {
        let dir = tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();

        std::fs::create_dir_all(root.join(".changeset").as_std_path()).unwrap();
        std::fs::write(root.join(".changeset/config.json").as_std_path(), "{}").unwrap();
        assert!(matches!(detect_import_source(&root), Some(ImportSource::Changesets)));

        std::fs::remove_file(root.join(".changeset/config.json").as_std_path()).unwrap();
        std::fs::remove_dir(root.join(".changeset").as_std_path()).unwrap();
        std::fs::write(root.join(".release-please-config.json").as_std_path(), "{}").unwrap();
        assert!(matches!(detect_import_source(&root), Some(ImportSource::ReleasePlease)));
    }

    #[test]
    fn changesets_migration_creates_release_strategy() {
        let dir = tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        std::fs::create_dir_all(root.join(".changeset").as_std_path()).unwrap();
        std::fs::write(
            root.join(".changeset/config.json").as_std_path(),
            r#"{"ignore":["internal"]}"#,
        )
        .unwrap();

        migrate_changesets_release_config(&root, OutputFormat::Json).unwrap();

        let written = std::fs::read_to_string(root.join("versionx.toml").as_std_path()).unwrap();
        assert!(written.contains("[release]"));
        assert!(written.contains(r#"strategy = "changesets""#));
        assert!(written.contains("[versionx]"));
    }

    #[test]
    fn release_please_migration_preserves_existing_config_and_maps_fields() {
        let dir = tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        std::fs::write(
            root.join("versionx.toml").as_std_path(),
            "[versionx]\nschema_version = \"1\"\n\n[runtimes]\nnode = \"20\"\n",
        )
        .unwrap();
        std::fs::write(
            root.join(".release-please-config.json").as_std_path(),
            r#"{"include-component-in-tag":true,"changelog-path":"docs/CHANGELOG.md"}"#,
        )
        .unwrap();

        migrate_release_please_config(&root, OutputFormat::Json).unwrap();

        let written = std::fs::read_to_string(root.join("versionx.toml").as_std_path()).unwrap();
        assert!(written.contains(r#"node = "20""#));
        assert!(written.contains(r#"strategy = "pr-title""#));
        assert!(written.contains(r#"changelog = "docs/CHANGELOG.md""#));
        assert!(written.contains(r#"tag_template = "{package}-v{version}""#));
    }
}
