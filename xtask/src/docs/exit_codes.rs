//! Exit-codes reference generator.
//!
//! Walks the `versionx-core` error taxonomy and emits the subsystem
//! exit-code table.

use anyhow::{Context, Result};
use camino::Utf8Path;

use super::banner;

pub(super) fn generate(root: &Utf8Path) -> Result<()> {
    let dir = root.join("reference");
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {dir}"))?;

    let path = dir.join("generated-exit-codes.mdx");
    let body = format!(
        "---\n\
         title: Exit codes (generated)\n\
         description: Auto-generated subsystem exit-code detail.\n\
         sidebar_position: 99\n\
         ---\n\n\
         {banner}\n\
         # Exit codes (generated)\n\n\
         Placeholder — the real generator walks the core error taxonomy. See \
         `xtask/src/docs/exit_codes.rs`.\n",
        banner = banner("exit-codes"),
    );
    std::fs::write(&path, body).with_context(|| format!("writing {path}"))?;
    eprintln!("docs-exit-codes: wrote {path}");
    Ok(())
}
