//! `versionx.toml` reference generator.
//!
//! Walks the `versionx-config` schema and produces a table of every
//! key / type / default / since-version. Scaffolded for now; the schema
//! walk can be dropped in once the config crate exposes a stable
//! reflection surface.

use anyhow::{Context, Result};
use camino::Utf8Path;

use super::banner;

pub(super) fn generate(root: &Utf8Path) -> Result<()> {
    let dir = root.join("reference");
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {dir}"))?;

    let path = dir.join("generated-versionx-toml.mdx");
    let body = format!(
        "---\n\
         title: versionx.toml (generated)\n\
         description: Auto-generated schema reference for versionx.toml.\n\
         sidebar_position: 99\n\
         ---\n\n\
         {banner}\n\
         # versionx.toml (generated)\n\n\
         Placeholder — the real generator walks `versionx-config`'s schema \
         and emits one row per key. See `xtask/src/docs/config.rs`.\n",
        banner = banner("config"),
    );
    std::fs::write(&path, body).with_context(|| format!("writing {path}"))?;
    eprintln!("docs-config: wrote {path}");
    Ok(())
}
