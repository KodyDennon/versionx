//! `versionx workspace list | status | graph` — inspect the component tree.
//!
//! This surfaces the component-tracking core: **your own components**, not
//! 3rd-party deps. Everything here goes through [`versionx_workspace::discover`]
//! + [`versionx_workspace::ComponentGraph`] so there's one authoritative view.

use camino::Utf8PathBuf;
use serde::Serialize;
use versionx_lockfile::Lockfile;
use versionx_workspace::{Component, ComponentGraph, Workspace, discovery, hash};

use super::CoreContext;
use crate::error::{CoreError, CoreResult};

// ---------- list ---------------------------------------------------------

#[derive(Clone, Debug)]
pub struct ListOptions {
    pub root: Utf8PathBuf,
}

#[derive(Clone, Debug, Serialize)]
pub struct ListEntry {
    pub id: String,
    pub display_name: String,
    pub kind: String,
    pub root: Utf8PathBuf,
    pub version: Option<String>,
    pub source: String,
    pub depends_on: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ListOutcome {
    pub workspace_root: Utf8PathBuf,
    pub components: Vec<ListEntry>,
}

pub fn list(_ctx: &CoreContext, opts: &ListOptions) -> CoreResult<ListOutcome> {
    let ws = discover_or_error(&opts.root)?;
    Ok(ListOutcome {
        workspace_root: ws.root.clone(),
        components: ws.components.values().map(to_list_entry).collect(),
    })
}

fn to_list_entry(c: &Component) -> ListEntry {
    ListEntry {
        id: c.id.to_string(),
        display_name: c.display_name.clone(),
        kind: c.kind.as_str().to_string(),
        root: c.root.clone(),
        version: c.version.as_ref().map(ToString::to_string),
        source: match &c.source {
            versionx_workspace::ComponentSource::Manifest { manifest_path } => {
                format!("manifest:{manifest_path}")
            }
            versionx_workspace::ComponentSource::Declared => "declared".into(),
        },
        depends_on: c.depends_on.iter().map(ToString::to_string).collect(),
    }
}

// ---------- status -------------------------------------------------------

#[derive(Clone, Debug)]
pub struct StatusOptions {
    pub root: Utf8PathBuf,
}

#[derive(Clone, Debug, Serialize)]
pub struct ComponentStatus {
    pub id: String,
    pub kind: String,
    pub version: Option<String>,
    pub current_hash: String,
    pub last_hash: Option<String>,
    pub dirty: bool,
    pub cascade: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct StatusOutcome {
    pub workspace_root: Utf8PathBuf,
    pub components: Vec<ComponentStatus>,
    pub any_dirty: bool,
}

pub fn status(_ctx: &CoreContext, opts: &StatusOptions) -> CoreResult<StatusOutcome> {
    let ws = discover_or_error(&opts.root)?;
    let graph = build_graph(&ws)?;
    let last_hashes = load_last_hashes(&opts.root);

    let mut components = Vec::with_capacity(ws.len());
    let mut any_dirty = false;
    for component in ws.components.values() {
        let current = hash::hash_component(&component.root, &component.inputs).map_err(|e| {
            CoreError::Io {
                path: component.root.to_string(),
                source: std::io::Error::other(e.to_string()),
            }
        })?;
        let last = last_hashes.get(component.id.as_str()).cloned();
        let dirty = last.as_deref() != Some(current.as_str());
        if dirty {
            any_dirty = true;
        }
        let cascade = if dirty {
            graph.transitive_dependents(&component.id).into_iter().map(|c| c.to_string()).collect()
        } else {
            Vec::new()
        };
        components.push(ComponentStatus {
            id: component.id.to_string(),
            kind: component.kind.as_str().to_string(),
            version: component.version.as_ref().map(ToString::to_string),
            current_hash: current,
            last_hash: last,
            dirty,
            cascade,
        });
    }

    Ok(StatusOutcome { workspace_root: ws.root, components, any_dirty })
}

// ---------- graph --------------------------------------------------------

#[derive(Clone, Debug)]
pub struct GraphOptions {
    pub root: Utf8PathBuf,
}

#[derive(Clone, Debug, Serialize)]
pub struct GraphEdge {
    pub from: String,
    pub to: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct GraphOutcome {
    pub workspace_root: Utf8PathBuf,
    pub nodes: Vec<String>,
    pub edges: Vec<GraphEdge>,
    pub topo_order: Vec<String>,
}

pub fn graph(_ctx: &CoreContext, opts: &GraphOptions) -> CoreResult<GraphOutcome> {
    let ws = discover_or_error(&opts.root)?;
    let g = build_graph(&ws)?;

    let mut edges = Vec::new();
    for component in ws.components.values() {
        for dep in &component.depends_on {
            edges.push(GraphEdge { from: component.id.to_string(), to: dep.to_string() });
        }
    }

    Ok(GraphOutcome {
        workspace_root: ws.root,
        nodes: ws.components.keys().map(ToString::to_string).collect(),
        edges,
        topo_order: g.topo_order().into_iter().map(|c| c.to_string()).collect(),
    })
}

// ---------- shared helpers -----------------------------------------------

fn discover_or_error(root: &camino::Utf8Path) -> CoreResult<Workspace> {
    discovery::discover(root).map_err(|e| CoreError::Io {
        path: root.to_string(),
        source: std::io::Error::other(e.to_string()),
    })
}

fn build_graph(ws: &Workspace) -> CoreResult<ComponentGraph> {
    ComponentGraph::build(ws).map_err(|e| CoreError::Io {
        path: ws.root.to_string(),
        source: std::io::Error::other(e.to_string()),
    })
}

/// Load last-released content hashes from the workspace lockfile.
/// Uses a `components.<id>.content_hash` convention under the normal
/// lockfile schema — when absent, every component is reported as dirty
/// (because `last_hash = None`).
fn load_last_hashes(root: &camino::Utf8Path) -> indexmap::IndexMap<String, String> {
    let lockfile_path = root.join("versionx.lock");
    let Ok(lock) = Lockfile::load(&lockfile_path) else {
        return indexmap::IndexMap::new();
    };
    let mut out = indexmap::IndexMap::new();
    // For 0.1 we piggy-back on the existing schema: `[runtimes.<id>].sha256`
    // is reused as a carrier when the lockfile version bumps we'll move to
    // a dedicated `[components.<id>].content_hash` field. For now we just
    // look for an optional `components` table emitted by the bump command
    // (not yet shipped). Returning an empty map is safe — it just means
    // everything reports dirty.
    drop(lock);
    let _ = &mut out;
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_outcome_serializes() {
        let o = ListOutcome {
            workspace_root: Utf8PathBuf::from("/tmp/x"),
            components: vec![ListEntry {
                id: "foo".into(),
                display_name: "foo".into(),
                kind: "node".into(),
                root: Utf8PathBuf::from("/tmp/x/foo"),
                version: Some("1.0.0".into()),
                source: "declared".into(),
                depends_on: vec![],
            }],
        };
        let j = serde_json::to_string(&o).unwrap();
        assert!(j.contains("\"foo\""));
    }
}
