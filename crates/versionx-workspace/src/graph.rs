//! Directed dependency graph between [`Component`]s.
//!
//! Cheap wrapper over `petgraph::Graph<ComponentId, ()>` with a couple of
//! workspace-specific operations:
//!
//! - `cascade_from(id)` returns every component that transitively depends
//!   on `id` — this is the "who needs to re-bump when X changes?" answer.
//! - `topo_order()` returns components in release-safe order (leaves before
//!   roots).

use std::collections::{BTreeSet, HashMap};

use petgraph::Direction;
use petgraph::graph::{DiGraph, NodeIndex};

use crate::error::{WorkspaceError, WorkspaceResult};
use crate::model::{Component, ComponentId, Workspace};

#[derive(Debug)]
pub struct ComponentGraph {
    graph: DiGraph<ComponentId, ()>,
    index: HashMap<ComponentId, NodeIndex>,
}

impl ComponentGraph {
    /// Build the graph from a resolved workspace. Verifies every dep-id
    /// exists and no cycles are present.
    ///
    /// # Errors
    /// Returns [`WorkspaceError::UnknownDep`] for a reference to a missing
    /// component, or [`WorkspaceError::Cycle`] when a cycle is detected.
    pub fn build(workspace: &Workspace) -> WorkspaceResult<Self> {
        let mut graph: DiGraph<ComponentId, ()> = DiGraph::new();
        let mut index: HashMap<ComponentId, NodeIndex> = HashMap::new();

        for id in workspace.components.keys() {
            let node = graph.add_node(id.clone());
            index.insert(id.clone(), node);
        }

        for component in workspace.components.values() {
            let from = index[&component.id];
            for dep in &component.depends_on {
                let to = *index.get(dep).ok_or_else(|| WorkspaceError::UnknownDep {
                    id: component.id.to_string(),
                    dep: dep.to_string(),
                })?;
                graph.add_edge(from, to, ());
            }
        }

        // Cycle check.
        if petgraph::algo::is_cyclic_directed(&graph) {
            let cycle: Vec<String> = petgraph::algo::tarjan_scc(&graph)
                .into_iter()
                .find(|scc| scc.len() > 1)
                .map(|scc| scc.into_iter().map(|n| graph[n].to_string()).collect())
                .unwrap_or_default();
            return Err(WorkspaceError::Cycle { path: cycle });
        }

        Ok(Self { graph, index })
    }

    /// Topological sort leaves-first (dependencies before their dependents).
    /// Stable: nodes with the same "depth" are returned in insertion order.
    #[must_use]
    pub fn topo_order(&self) -> Vec<ComponentId> {
        let mut order = petgraph::algo::toposort(&self.graph, None).unwrap_or_default();
        // petgraph returns dependents first; we want dependencies first.
        order.reverse();
        order.into_iter().map(|n| self.graph[n].clone()).collect()
    }

    /// Return every component that directly depends on `id` (incoming edges).
    #[must_use]
    pub fn direct_dependents(&self, id: &ComponentId) -> Vec<ComponentId> {
        let Some(&node) = self.index.get(id) else {
            return Vec::new();
        };
        self.graph
            .neighbors_directed(node, Direction::Incoming)
            .map(|n| self.graph[n].clone())
            .collect()
    }

    /// Return every component that transitively depends on `id`.
    /// (`id` itself is **not** included.)
    #[must_use]
    pub fn transitive_dependents(&self, id: &ComponentId) -> BTreeSet<ComponentId> {
        let mut out = BTreeSet::new();
        let Some(&start) = self.index.get(id) else {
            return out;
        };
        let mut stack = vec![start];
        while let Some(n) = stack.pop() {
            for dep in self.graph.neighbors_directed(n, Direction::Incoming) {
                let dep_id = self.graph[dep].clone();
                if out.insert(dep_id) {
                    stack.push(dep);
                }
            }
        }
        out
    }

    /// Number of components in the graph.
    #[must_use]
    pub fn len(&self) -> usize {
        self.graph.node_count()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.graph.node_count() == 0
    }
}

/// Return every [`Component`] in `workspace` that transitively depends on
/// any of `changed`. Includes the changed components themselves.
pub fn cascade_from<'a>(
    workspace: &'a Workspace,
    graph: &ComponentGraph,
    changed: &[ComponentId],
) -> Vec<&'a Component> {
    let mut affected: BTreeSet<ComponentId> = changed.iter().cloned().collect();
    for c in changed {
        affected.extend(graph.transitive_dependents(c));
    }
    affected.iter().filter_map(|id| workspace.get(id)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ComponentKind, ComponentSource};
    use camino::Utf8PathBuf;
    use indexmap::IndexMap;

    fn comp(name: &str, deps: &[&str]) -> Component {
        Component {
            id: ComponentId::new(name),
            display_name: name.into(),
            root: Utf8PathBuf::from(format!("/tmp/{name}")),
            kind: ComponentKind::Rust,
            source: ComponentSource::Declared,
            version: None,
            inputs: vec!["**/*".into()],
            depends_on: deps.iter().map(|d| ComponentId::new(*d)).collect(),
        }
    }

    fn make_workspace(components: Vec<Component>) -> Workspace {
        let mut map = IndexMap::new();
        for c in components {
            map.insert(c.id.clone(), c);
        }
        Workspace { root: Utf8PathBuf::from("/tmp"), components: map }
    }

    #[test]
    fn topo_order_leaves_first() {
        let ws = make_workspace(vec![
            comp("app", &["ui", "core"]),
            comp("ui", &["core"]),
            comp("core", &[]),
        ]);
        let g = ComponentGraph::build(&ws).unwrap();
        let order: Vec<String> = g.topo_order().into_iter().map(|c| c.to_string()).collect();
        // core must come first, then ui, then app.
        let core_idx = order.iter().position(|s| s == "core").unwrap();
        let ui_idx = order.iter().position(|s| s == "ui").unwrap();
        let app_idx = order.iter().position(|s| s == "app").unwrap();
        assert!(core_idx < ui_idx && ui_idx < app_idx);
    }

    #[test]
    fn direct_dependents() {
        let ws =
            make_workspace(vec![comp("core", &[]), comp("ui", &["core"]), comp("app", &["ui"])]);
        let g = ComponentGraph::build(&ws).unwrap();
        let deps = g.direct_dependents(&ComponentId::new("core"));
        assert_eq!(deps, vec![ComponentId::new("ui")]);
    }

    #[test]
    fn transitive_dependents() {
        let ws = make_workspace(vec![
            comp("core", &[]),
            comp("ui", &["core"]),
            comp("app", &["ui"]),
            comp("docs", &["core"]),
        ]);
        let g = ComponentGraph::build(&ws).unwrap();
        let deps = g.transitive_dependents(&ComponentId::new("core"));
        assert_eq!(
            deps,
            [ComponentId::new("app"), ComponentId::new("docs"), ComponentId::new("ui"),]
                .iter()
                .cloned()
                .collect()
        );
    }

    #[test]
    fn unknown_dep_errors() {
        let ws = make_workspace(vec![comp("app", &["nonexistent"])]);
        let err = ComponentGraph::build(&ws).unwrap_err();
        assert!(matches!(err, WorkspaceError::UnknownDep { .. }));
    }

    #[test]
    fn cycle_detected() {
        let ws = make_workspace(vec![comp("a", &["b"]), comp("b", &["a"])]);
        let err = ComponentGraph::build(&ws).unwrap_err();
        assert!(matches!(err, WorkspaceError::Cycle { .. }));
    }

    #[test]
    fn cascade_includes_changed_and_dependents() {
        let ws = make_workspace(vec![
            comp("core", &[]),
            comp("ui", &["core"]),
            comp("app", &["ui"]),
            comp("unrelated", &[]),
        ]);
        let g = ComponentGraph::build(&ws).unwrap();
        let affected: Vec<String> = cascade_from(&ws, &g, &[ComponentId::new("core")])
            .into_iter()
            .map(|c| c.id.to_string())
            .collect();
        assert!(affected.contains(&"core".to_string()));
        assert!(affected.contains(&"ui".to_string()));
        assert!(affected.contains(&"app".to_string()));
        assert!(!affected.contains(&"unrelated".to_string()));
    }
}
