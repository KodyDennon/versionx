//! MCP tool catalog generator.
//!
//! Inventories the tools registered in `versionx-mcp` and emits one
//! section per tool with its argument and return schema.

use anyhow::{Context, Result};
use camino::Utf8Path;

use super::banner;

pub(super) fn generate(root: &Utf8Path) -> Result<()> {
    let dir = root.join("integrations").join("mcp");
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {dir}"))?;

    let path = dir.join("generated-tool-catalog.mdx");
    let body = format!(
        "---\n\
         title: MCP tool catalog (generated)\n\
         description: Auto-generated per-tool MCP reference.\n\
         sidebar_position: 99\n\
         ---\n\n\
         {banner}\n\
         # MCP tool catalog (generated)\n\n\
         Placeholder — the real generator reflects the `rmcp` tool registry \
         in `versionx-mcp`. See `xtask/src/docs/mcp.rs`.\n",
        banner = banner("mcp"),
    );
    std::fs::write(&path, body).with_context(|| format!("writing {path}"))?;
    eprintln!("docs-mcp: wrote {path}");
    Ok(())
}
