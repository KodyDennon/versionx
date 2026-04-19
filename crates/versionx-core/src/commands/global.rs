//! `versionx global set|get|unset` — user-level default pins.
//!
//! Writes to `$XDG_CONFIG_HOME/versionx/global.toml` (or the equivalent per
//! platform and the `VERSIONX_HOME` override). The shim + `versionx which`
//! both already look here as a fallback after walking for a repo-local
//! `versionx.toml`.

use std::fs;

use serde::Serialize;
use toml_edit::{DocumentMut, Item, Table, value};

use super::CoreContext;
use crate::error::{CoreError, CoreResult};

#[derive(Clone, Debug, Serialize)]
pub struct GlobalSetOutcome {
    pub tool: String,
    pub version: String,
    pub path: camino::Utf8PathBuf,
    pub previous: Option<String>,
}

/// Set `tool = version` in the user's global config, creating the file +
/// parent directory if needed.
///
/// # Errors
/// - [`CoreError::Io`] on filesystem or config-edit failures.
pub fn set(ctx: &CoreContext, tool: &str, version: &str) -> CoreResult<GlobalSetOutcome> {
    let path = ctx.home.global_config();
    if let Some(parent) = path.parent()
        && !parent.as_str().is_empty()
    {
        fs::create_dir_all(parent)
            .map_err(|source| CoreError::Io { path: parent.to_string(), source })?;
    }

    let raw = fs::read_to_string(&path).unwrap_or_default();
    let mut doc = raw
        .parse::<DocumentMut>()
        .map_err(|e| CoreError::Serialize(format!("parsing existing global.toml as TOML: {e}")))?;

    let previous = doc
        .get("runtimes")
        .and_then(|r| r.as_table())
        .and_then(|t| t.get(tool))
        .and_then(|v| v.as_str())
        .map(ToString::to_string);

    // Ensure `[runtimes]` exists.
    if !doc.contains_key("runtimes") {
        doc["runtimes"] = Item::Table(Table::new());
    }
    let runtimes = doc["runtimes"]
        .as_table_mut()
        .ok_or_else(|| CoreError::Serialize("`[runtimes]` in global.toml is not a table".into()))?;
    runtimes[tool] = value(version);

    fs::write(&path, doc.to_string())
        .map_err(|source| CoreError::Io { path: path.to_string(), source })?;

    Ok(GlobalSetOutcome { tool: tool.into(), version: version.into(), path, previous })
}

#[derive(Clone, Debug, Serialize)]
pub struct GlobalGetOutcome {
    pub tool: String,
    pub version: Option<String>,
    pub path: camino::Utf8PathBuf,
}

/// Read the pinned version for `tool` from the user's global config.
///
/// # Errors
/// None currently — missing file / missing entry return `version: None`. Kept
/// in `Result` shape for symmetry with [`set`] / [`unset`] and forward-compat.
#[allow(clippy::unnecessary_wraps)]
pub fn get(ctx: &CoreContext, tool: &str) -> CoreResult<GlobalGetOutcome> {
    let path = ctx.home.global_config();
    let version = fs::read_to_string(&path).ok().and_then(|raw| {
        let doc: DocumentMut = raw.parse().ok()?;
        doc.get("runtimes")
            .and_then(Item::as_table)
            .and_then(|t| t.get(tool))
            .and_then(|v| v.as_str())
            .map(ToString::to_string)
    });
    Ok(GlobalGetOutcome { tool: tool.into(), version, path })
}

#[derive(Clone, Debug, Serialize)]
pub struct GlobalUnsetOutcome {
    pub tool: String,
    pub path: camino::Utf8PathBuf,
    pub removed: bool,
    pub previous: Option<String>,
}

/// Remove a `tool` entry from the user's global config. Safe to call on a
/// missing file or missing entry — those return `removed: false`.
///
/// # Errors
/// - [`CoreError::Io`] on filesystem or TOML-edit failures.
pub fn unset(ctx: &CoreContext, tool: &str) -> CoreResult<GlobalUnsetOutcome> {
    let path = ctx.home.global_config();
    let Ok(raw) = fs::read_to_string(&path) else {
        return Ok(GlobalUnsetOutcome { tool: tool.into(), path, removed: false, previous: None });
    };
    let mut doc: DocumentMut =
        raw.parse().map_err(|e| CoreError::Serialize(format!("parsing global.toml: {e}")))?;

    let Some(runtimes) = doc.get_mut("runtimes").and_then(Item::as_table_mut) else {
        return Ok(GlobalUnsetOutcome { tool: tool.into(), path, removed: false, previous: None });
    };

    let previous = runtimes.get(tool).and_then(|v| v.as_str()).map(ToString::to_string);
    let removed = runtimes.remove(tool).is_some();

    // Leave the file on disk even when it becomes empty — users sometimes rely
    // on its existence as a sentinel; emptying is the cheapest truthful state.
    fs::write(&path, doc.to_string())
        .map_err(|source| CoreError::Io { path: path.to_string(), source })?;

    Ok(GlobalUnsetOutcome { tool: tool.into(), path, removed, previous })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::VersionxHome;
    use camino::Utf8PathBuf;
    use reqwest::Client;
    use std::sync::Arc;
    use versionx_events::EventBus;
    use versionx_runtime_trait::Platform;

    fn ctx(home_root: &Utf8PathBuf) -> CoreContext {
        let home = VersionxHome {
            data: home_root.clone(),
            cache: home_root.clone(),
            config: home_root.clone(),
            state: home_root.clone(),
            runtime: home_root.clone(),
        };
        CoreContext {
            home,
            events: EventBus::new().sender(),
            http: Client::new(),
            platform: Platform::detect(),
            registry: Arc::new(crate::runtime_registry::registry()),
            adapter_registry: Arc::new(crate::adapter_registry::registry()),
        }
    }

    #[test]
    fn set_creates_file_and_records_pin() {
        let tmp = tempfile::tempdir().unwrap();
        let home = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        let c = ctx(&home);
        let out = set(&c, "node", "22.12.0").unwrap();
        assert_eq!(out.version, "22.12.0");
        assert!(out.previous.is_none());
        let body = std::fs::read_to_string(&out.path).unwrap();
        assert!(body.contains("node = \"22.12.0\""));
    }

    #[test]
    fn set_then_get_round_trips() {
        let tmp = tempfile::tempdir().unwrap();
        let home = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        let c = ctx(&home);
        set(&c, "node", "22.12.0").unwrap();
        let got = get(&c, "node").unwrap();
        assert_eq!(got.version.as_deref(), Some("22.12.0"));
    }

    #[test]
    fn set_overwrites_returns_previous() {
        let tmp = tempfile::tempdir().unwrap();
        let home = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        let c = ctx(&home);
        set(&c, "node", "18.19.0").unwrap();
        let out = set(&c, "node", "22.12.0").unwrap();
        assert_eq!(out.previous.as_deref(), Some("18.19.0"));
    }

    #[test]
    fn unset_removes_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let home = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        let c = ctx(&home);
        set(&c, "node", "22.12.0").unwrap();
        let out = unset(&c, "node").unwrap();
        assert!(out.removed);
        assert_eq!(out.previous.as_deref(), Some("22.12.0"));
        let got = get(&c, "node").unwrap();
        assert!(got.version.is_none());
    }

    #[test]
    fn unset_on_missing_returns_not_removed() {
        let tmp = tempfile::tempdir().unwrap();
        let home = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        let c = ctx(&home);
        let out = unset(&c, "node").unwrap();
        assert!(!out.removed);
    }

    #[test]
    fn multiple_tools_coexist() {
        let tmp = tempfile::tempdir().unwrap();
        let home = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        let c = ctx(&home);
        set(&c, "node", "20").unwrap();
        set(&c, "python", "3.12").unwrap();
        set(&c, "rust", "stable").unwrap();
        assert_eq!(get(&c, "node").unwrap().version.as_deref(), Some("20"));
        assert_eq!(get(&c, "python").unwrap().version.as_deref(), Some("3.12"));
        assert_eq!(get(&c, "rust").unwrap().version.as_deref(), Some("stable"));
    }
}
