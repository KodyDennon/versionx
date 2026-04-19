//! Registry of every runtime installer we ship.
//!
//! Frontends look up a runtime by id (`"node"`, `"python"`, `"rust"`, ...)
//! and receive a boxed [`RuntimeInstaller`]. Additional installers added in
//! v1.1+ (Go, Ruby, JVM, pnpm-as-runtime, ...) land by extending this.

use std::collections::BTreeMap;
use std::sync::Arc;

use versionx_runtime_node::{NodeInstaller, PnpmInstaller, YarnInstaller};
use versionx_runtime_python::PythonInstaller;
use versionx_runtime_rust::RustInstaller;
use versionx_runtime_trait::RuntimeInstaller;

/// Immutable lookup table of installers.
pub struct RuntimeRegistry {
    installers: BTreeMap<&'static str, Arc<dyn RuntimeInstaller>>,
}

impl std::fmt::Debug for RuntimeRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimeRegistry")
            .field("installers", &self.installers.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl RuntimeRegistry {
    /// Look up an installer by tool id.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<Arc<dyn RuntimeInstaller>> {
        self.installers.get(id).cloned()
    }

    /// Iterate over all registered tool ids in stable order.
    pub fn ids(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.installers.keys().copied()
    }
}

/// Build the default registry (all installers shipped in this binary).
#[must_use]
pub fn registry() -> RuntimeRegistry {
    let mut installers: BTreeMap<&'static str, Arc<dyn RuntimeInstaller>> = BTreeMap::new();
    let node: Arc<dyn RuntimeInstaller> = Arc::new(NodeInstaller::new());
    let python: Arc<dyn RuntimeInstaller> = Arc::new(PythonInstaller::new());
    let rust: Arc<dyn RuntimeInstaller> = Arc::new(RustInstaller::new());
    let pnpm: Arc<dyn RuntimeInstaller> = Arc::new(PnpmInstaller::new());
    let yarn: Arc<dyn RuntimeInstaller> = Arc::new(YarnInstaller::new());
    installers.insert(node.id(), node);
    installers.insert(python.id(), python);
    installers.insert(rust.id(), rust);
    installers.insert(pnpm.id(), pnpm);
    installers.insert(yarn.id(), yarn);
    RuntimeRegistry { installers }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_has_expected_tools() {
        let reg = registry();
        let ids: Vec<_> = reg.ids().collect();
        assert!(ids.contains(&"node"));
        assert!(ids.contains(&"python"));
        assert!(ids.contains(&"rust"));
        assert!(ids.contains(&"pnpm"));
        assert!(ids.contains(&"yarn"));
    }

    #[test]
    fn unknown_tool_returns_none() {
        assert!(registry().get("cobol").is_none());
    }
}
