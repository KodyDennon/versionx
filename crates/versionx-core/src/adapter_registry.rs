//! Registry of every package-manager adapter we ship.
//!
//! Parallels `runtime_registry`. Frontends look up an adapter by ecosystem id
//! (`"node"`, `"python"`, `"rust"`) and receive a boxed
//! [`PackageManagerAdapter`]. v0.1.0 only ships the Node adapter end-to-end;
//! Python + Rust adapters land in 0.2.

use std::collections::BTreeMap;
use std::sync::Arc;

use versionx_adapter_node::NodeAdapter;
use versionx_adapter_python::PythonAdapter;
use versionx_adapter_rust::RustAdapter;
use versionx_adapter_trait::PackageManagerAdapter;

/// Immutable lookup table of adapters.
pub struct AdapterRegistry {
    adapters: BTreeMap<&'static str, Arc<dyn PackageManagerAdapter>>,
}

impl std::fmt::Debug for AdapterRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AdapterRegistry")
            .field("adapters", &self.adapters.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl AdapterRegistry {
    /// Look up an adapter by its id (`"node"`, `"python"`, `"rust"`).
    #[must_use]
    pub fn get(&self, id: &str) -> Option<Arc<dyn PackageManagerAdapter>> {
        self.adapters.get(id).cloned()
    }

    /// Iterate every registered adapter id in stable order.
    pub fn ids(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.adapters.keys().copied()
    }
}

/// Build the default registry (all adapters shipped in this binary).
#[must_use]
pub fn registry() -> AdapterRegistry {
    let mut adapters: BTreeMap<&'static str, Arc<dyn PackageManagerAdapter>> = BTreeMap::new();
    let node: Arc<dyn PackageManagerAdapter> = Arc::new(NodeAdapter::new());
    let python: Arc<dyn PackageManagerAdapter> = Arc::new(PythonAdapter::new());
    let rust: Arc<dyn PackageManagerAdapter> = Arc::new(RustAdapter::new());
    adapters.insert(node.id(), node);
    adapters.insert(python.id(), python);
    adapters.insert(rust.id(), rust);
    AdapterRegistry { adapters }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn has_node_python_rust_adapters() {
        let reg = registry();
        assert!(reg.get("node").is_some());
        assert!(reg.get("python").is_some());
        assert!(reg.get("rust").is_some());
        assert!(reg.get("cobol").is_none());
    }
}
