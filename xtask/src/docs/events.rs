//! Events catalog generator.
//!
//! Reflects on the `versionx-events` enum variants and emits one entry
//! per event kind with its data fields and since-version.

use anyhow::{Context, Result};
use camino::Utf8Path;

use super::banner;

pub(super) fn generate(root: &Utf8Path) -> Result<()> {
    let dir = root.join("reference");
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {dir}"))?;

    let path = dir.join("generated-events.mdx");
    let body = format!(
        "---\n\
         title: Events (generated)\n\
         description: Auto-generated catalog of every structured event versionx emits.\n\
         sidebar_position: 99\n\
         ---\n\n\
         {banner}\n\
         # Events (generated)\n\n\
         Placeholder — the real generator reflects on the event enum in \
         `versionx-events` and emits one section per variant. See \
         `xtask/src/docs/events.rs`.\n",
        banner = banner("events"),
    );
    std::fs::write(&path, body).with_context(|| format!("writing {path}"))?;
    eprintln!("docs-events: wrote {path}");
    Ok(())
}
