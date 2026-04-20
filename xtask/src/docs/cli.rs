//! CLI reference generator.
//!
//! Invokes `versionx --help-json` and walks the resulting tree. Emits:
//!  - `reference/cli/versionx.md` — root page with every top-level flag
//!    and a subcommand index.
//!  - `reference/cli/<name>.md` per top-level subcommand, recursively
//!    rendering sub-subcommands inline.

use std::collections::BTreeMap;

use anyhow::{Context, Result, anyhow};
use camino::{Utf8Path, Utf8PathBuf};
use serde::Deserialize;

use super::banner;

#[derive(Debug, Deserialize)]
struct HelpRoot {
    #[serde(default)]
    versionx_version: String,
    command: CommandNode,
}

#[derive(Debug, Deserialize)]
struct CommandNode {
    name: String,
    #[serde(default)]
    about: Option<String>,
    #[serde(default)]
    long_about: Option<String>,
    #[serde(default)]
    args: Vec<ArgNode>,
    #[serde(default)]
    subcommands: Vec<CommandNode>,
}

#[derive(Debug, Deserialize)]
struct ArgNode {
    id: String,
    #[serde(default)]
    long: Option<String>,
    #[serde(default)]
    short: Option<String>,
    #[serde(default)]
    help: Option<String>,
    #[serde(default)]
    value_name: Option<Vec<String>>,
    #[serde(default)]
    required: bool,
    #[serde(default)]
    global: bool,
    #[serde(default)]
    default_values: Vec<String>,
    #[serde(default)]
    possible_values: Vec<String>,
}

pub(super) fn generate(root: &Utf8Path) -> Result<()> {
    let dir = root.join("reference").join("cli");
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {dir}"))?;

    // Build versionx-cli in release mode so invocation is fast and stable.
    // Use a flag path to avoid ambient shell PATH issues.
    duct::cmd!("cargo", "build", "--quiet", "-p", "versionx-cli", "--release")
        .run()
        .context("building versionx-cli for docs-cli")?;

    let bin = Utf8PathBuf::from("target/release/versionx");
    if !bin.as_std_path().exists() {
        return Err(anyhow!("expected {bin} to exist after build"));
    }

    let out =
        duct::cmd!(bin.as_str(), "--help-json").read().context("running versionx --help-json")?;

    let help: HelpRoot = serde_json::from_str(&out).context("parsing --help-json output")?;

    // Clean out any previously-generated files so renames don't leave stragglers.
    for entry in std::fs::read_dir(&dir).with_context(|| format!("reading {dir}"))? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if name.ends_with(".md") || name.ends_with(".mdx") {
            std::fs::remove_file(entry.path())?;
        }
    }

    // Root page.
    write_root(&dir, &help).context("writing CLI root page")?;

    // One page per top-level subcommand.
    for (idx, sub) in help.command.subcommands.iter().enumerate() {
        let filename = dir.join(format!("{}.md", sub.name));
        let body = render_subcommand_page(sub, idx + 2);
        std::fs::write(&filename, body).with_context(|| format!("writing {filename}"))?;
        eprintln!("docs-cli: wrote {filename}");
    }

    eprintln!(
        "docs-cli: rendered root + {} subcommands from versionx {}",
        help.command.subcommands.len(),
        help.versionx_version,
    );
    Ok(())
}

fn write_root(dir: &Utf8Path, help: &HelpRoot) -> Result<()> {
    let path = dir.join("versionx.md");

    let mut body = String::new();
    body.push_str("---\n");
    body.push_str("title: versionx (CLI root)\n");
    body.push_str(
        "description: Auto-generated CLI reference for the versionx root command and its subcommands.\n",
    );
    body.push_str("sidebar_position: 1\n");
    body.push_str("---\n\n");
    body.push_str(&banner("cli"));
    body.push_str(&format!("\n# `versionx`\n\n"));
    body.push_str(&format!(
        "_Generated from `versionx --help-json` at version {}._\n\n",
        help.versionx_version,
    ));

    if let Some(about) = help.command.about.as_deref() {
        body.push_str(about);
        body.push_str("\n\n");
    }
    if let Some(long) = help.command.long_about.as_deref() {
        body.push_str(long);
        body.push_str("\n\n");
    }

    body.push_str("## Global flags\n\n");
    body.push_str(&render_args_table(&help.command.args));

    body.push_str("\n## Subcommands\n\n");
    body.push_str("| Command | Summary |\n|---|---|\n");

    let sorted: BTreeMap<_, _> =
        help.command.subcommands.iter().map(|s| (s.name.clone(), s)).collect();
    for (name, sub) in &sorted {
        let summary =
            sub.about.as_deref().unwrap_or("").lines().next().unwrap_or("").trim().to_string();
        body.push_str(&format!(
            "| [`{name}`](./{name}) | {summary} |\n",
            name = name,
            summary = crate::docs::syn_util::table_escape(&summary),
        ));
    }

    body.push_str("\n## See also\n\n");
    body.push_str("- [`versionx.toml` reference](../versionx-toml)\n");
    body.push_str("- [Environment variables](../environment-variables)\n");
    body.push_str("- [Exit codes](../exit-codes)\n");

    std::fs::write(&path, body).with_context(|| format!("writing {path}"))?;
    eprintln!("docs-cli: wrote {path}");
    Ok(())
}

fn render_subcommand_page(cmd: &CommandNode, sidebar_position: usize) -> String {
    let mut body = String::new();
    body.push_str("---\n");
    body.push_str(&format!("title: versionx {}\n", cmd.name));
    body.push_str(
        &format!("description: Auto-generated reference for `versionx {}`.\n", cmd.name,),
    );
    body.push_str(&format!("sidebar_position: {sidebar_position}\n"));
    body.push_str("---\n\n");
    body.push_str(&banner("cli"));
    body.push_str(&format!("\n# `versionx {}`\n\n", cmd.name));

    render_command_body(&mut body, cmd, 2);
    body.push_str("\n## See also\n\n");
    body.push_str("- [`versionx` root](./versionx)\n");
    body.push_str("- [`versionx.toml` reference](../versionx-toml)\n");
    body
}

fn render_command_body(body: &mut String, cmd: &CommandNode, heading_level: usize) {
    if let Some(about) = cmd.about.as_deref() {
        body.push_str(about);
        body.push_str("\n\n");
    }
    if let Some(long) = cmd.long_about.as_deref() {
        body.push_str(long);
        body.push_str("\n\n");
    }

    if !cmd.args.is_empty() {
        body.push_str(&heading(heading_level, "Flags"));
        body.push_str(&render_args_table(&cmd.args));
        body.push('\n');
    }

    if !cmd.subcommands.is_empty() {
        body.push_str(&heading(heading_level, "Subcommands"));
        for sub in &cmd.subcommands {
            body.push_str(&heading(heading_level + 1, &format!("`{}`", sub.name)));
            render_command_body(body, sub, heading_level + 2);
        }
    }
}

fn heading(level: usize, text: &str) -> String {
    format!("{} {text}\n\n", "#".repeat(level.min(6)))
}

fn render_args_table(args: &[ArgNode]) -> String {
    if args.is_empty() {
        return "_No flags._\n".into();
    }
    let mut out = String::from("| Flag | Value | Default | Required | Global | Description |\n");
    out.push_str("|---|---|---|---|---|---|\n");
    for arg in args {
        let flag = render_flag(arg);
        let value = arg
            .value_name
            .as_ref()
            .map(|names| names.iter().map(|n| format!("`<{n}>`")).collect::<Vec<_>>().join(" "))
            .unwrap_or_default();
        let default = if arg.default_values.is_empty() {
            String::new()
        } else {
            format!("`{}`", arg.default_values.join(","))
        };
        let required = if arg.required { "yes" } else { "" };
        let global = if arg.global { "yes" } else { "" };
        let help = arg.help.as_deref().unwrap_or("");
        let help_trimmed = help.trim().trim_end_matches('.');
        let possibles = if arg.possible_values.is_empty() {
            String::new()
        } else {
            format!(" Possible values: `{}`.", arg.possible_values.join("`, `"))
        };
        let full_help = if possibles.is_empty() {
            help_trimmed.to_string()
        } else {
            format!("{help_trimmed}.{possibles}")
        };
        out.push_str(&format!(
            "| {flag} | {value} | {default} | {required} | {global} | {help} |\n",
            flag = flag,
            value = value,
            default = default,
            required = required,
            global = global,
            help = crate::docs::syn_util::table_escape(&full_help),
        ));
    }
    out
}

fn render_flag(arg: &ArgNode) -> String {
    let mut parts = Vec::new();
    if let Some(s) = &arg.short {
        parts.push(format!("`-{s}`"));
    }
    if let Some(l) = &arg.long {
        parts.push(format!("`--{l}`"));
    }
    if parts.is_empty() {
        parts.push(format!("`{}`", arg.id));
    }
    parts.join(" / ")
}
