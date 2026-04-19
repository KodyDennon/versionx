//! Shared context passed to every tool handler.
//!
//! The MCP server is stateless between sessions; all the state a tool
//! needs travels through [`McpContext`]. This keeps tool implementations
//! pure-function-shaped + trivially testable.

use std::sync::Arc;

use camino::Utf8PathBuf;

use crate::audit::AuditLog;

/// Per-server state, cloned into every tool call.
#[derive(Clone, Debug)]
pub struct McpContext {
    /// Working directory the server was launched from. Every tool that
    /// takes a workspace path defaults to this.
    pub workspace_root: Utf8PathBuf,
    /// Append-only log of every tool call. Wrapped in Arc so clones
    /// share a single file handle.
    pub audit: Arc<AuditLog>,
    /// Where to place artifacts (changelog drafts, plan files) when the
    /// tool writes. Defaults to `workspace_root/.versionx/mcp/`.
    pub artifacts_dir: Utf8PathBuf,
}

impl McpContext {
    /// Build a context rooted at `workspace_root`. Creates the audit log
    /// eagerly so the first tool call doesn't race with file creation.
    pub fn new(workspace_root: Utf8PathBuf) -> anyhow::Result<Self> {
        let artifacts_dir = workspace_root.join(".versionx/mcp");
        std::fs::create_dir_all(artifacts_dir.as_std_path())?;
        let audit = Arc::new(AuditLog::open(&artifacts_dir.join("audit.ndjson"))?);
        Ok(Self { workspace_root, audit, artifacts_dir })
    }
}
