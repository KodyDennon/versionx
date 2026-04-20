//! JSON-RPC method reference generator.
//!
//! Walks the `versionx-daemon` handler registry and emits one section per
//! method with its params / result shapes and streaming semantics.

use anyhow::{Context, Result};
use camino::Utf8Path;

use super::banner;

pub(super) fn generate(root: &Utf8Path) -> Result<()> {
    let dir = root.join("integrations");
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {dir}"))?;

    let path = dir.join("generated-json-rpc.mdx");
    let body = format!(
        "---\n\
         title: JSON-RPC methods (generated)\n\
         description: Auto-generated per-method reference for versiond.\n\
         sidebar_position: 99\n\
         ---\n\n\
         {banner}\n\
         # JSON-RPC methods (generated)\n\n\
         Placeholder — the real generator reflects the daemon handler \
         registry. See `xtask/src/docs/rpc.rs`.\n",
        banner = banner("rpc"),
    );
    std::fs::write(&path, body).with_context(|| format!("writing {path}"))?;
    eprintln!("docs-rpc: wrote {path}");
    Ok(())
}
