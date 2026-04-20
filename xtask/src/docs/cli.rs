//! CLI reference generator.
//!
//! Invokes `versionx --help-json` to get the full command tree and
//! renders one MDX page per subcommand into `reference/cli/`.
//!
//! For now this is a scaffold: the output file is stamped with a banner
//! and a TODO that directs maintainers to either implement the walk here
//! or wire `versionx --help-json` once it stabilizes. The infrastructure
//! (path, banner, CI drift check) is in place so the first real walk
//! drops in without any further plumbing.

use anyhow::{Context, Result};
use camino::Utf8Path;

use super::banner;

pub(super) fn generate(root: &Utf8Path) -> Result<()> {
    let dir = root.join("reference").join("cli");
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {dir}"))?;

    let path = dir.join("generated-index.mdx");
    let body = format!(
        "---\n\
         title: CLI index (generated)\n\
         description: Auto-generated catalog of every versionx subcommand.\n\
         sidebar_position: 99\n\
         ---\n\n\
         {banner}\n\
         # CLI index (generated)\n\n\
         This page is the placeholder for the auto-generated CLI catalog. Once the \
         generator walks `versionx --help-json` (see `xtask/src/docs/cli.rs`), \
         this page is rewritten with one section per subcommand. The authoring \
         side of `reference/cli/versionx.md` is hand-written and stable.\n",
        banner = banner("cli"),
    );

    std::fs::write(&path, body).with_context(|| format!("writing {path}"))?;
    eprintln!("docs-cli: wrote {path}");
    Ok(())
}
