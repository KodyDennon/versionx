//! MCP tool catalog generator.
//!
//! Parses `crates/versionx-mcp/src/tools/mod.rs` to discover registered
//! tools, then parses each tool's source file for its `descriptor()` fn
//! to recover the tool name, title, description, and `mutating` flag.

use anyhow::{Context, Result};
use camino::{Utf8Path, Utf8PathBuf};
use quote::ToTokens;
use regex::Regex;
use syn::{Expr, ExprLit, Fields, Item, Lit, Stmt};

use super::{banner, syn_util::doc_string};

const TOOLS_DIR: &str = "crates/versionx-mcp/src/tools";

#[derive(Debug)]
struct Tool {
    module: String,
    file: Utf8PathBuf,
    name: Option<String>,
    title: Option<String>,
    description: Option<String>,
    mutating: Option<bool>,
    module_doc: String,
}

pub(super) fn generate(root: &Utf8Path) -> Result<()> {
    let dir = root.join("integrations").join("mcp");
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {dir}"))?;
    let path = dir.join("tool-catalog.md");

    let modules = discover_modules(TOOLS_DIR).context("discovering tool modules")?;
    let tools: Vec<Tool> = modules
        .into_iter()
        .map(|(module, file)| extract_tool(&module, &file))
        .collect::<Result<Vec<_>>>()?;

    let mut body = String::new();
    body.push_str("---\n");
    body.push_str("title: MCP tool catalog\n");
    body.push_str(
        "description: Every MCP tool Versionx exposes, with name, purpose, \
         and mutating flag. Auto-generated from the versionx-mcp tools module.\n",
    );
    body.push_str("sidebar_position: 7\n");
    body.push_str("---\n\n");
    body.push_str(&banner("mcp"));
    body.push_str("\n# MCP tool catalog\n\n");
    body.push_str(&format!(
        "_Generated from `{TOOLS_DIR}/`. Workflow-shaped; tool count capped at ~10._\n\n",
    ));

    body.push_str("## Summary\n\n");
    body.push_str("| Tool | Mutating | Purpose |\n|---|---|---|\n");
    for tool in &tools {
        let name = tool.name.as_deref().unwrap_or(&tool.module);
        let mutating = tool.mutating.map(|m| if m { "yes" } else { "no" }).unwrap_or("?");
        let purpose = tool
            .title
            .as_deref()
            .or(tool.description.as_deref())
            .or(Some(tool.module_doc.as_str()))
            .map(|s| s.lines().next().unwrap_or("").to_string())
            .unwrap_or_default();
        body.push_str(&format!(
            "| `{name}` | {mutating} | {purpose} |\n",
            purpose = super::syn_util::table_escape(&purpose),
        ));
    }

    body.push_str("\n## Per-tool detail\n\n");
    for tool in &tools {
        let name = tool.name.as_deref().unwrap_or(&tool.module);
        body.push_str(&format!("### `{name}`\n\n"));
        if let Some(title) = &tool.title {
            body.push_str(&format!("**{title}**\n\n"));
        }
        if let Some(desc) = &tool.description {
            body.push_str(desc);
            body.push_str("\n\n");
        } else if !tool.module_doc.is_empty() {
            body.push_str(&tool.module_doc);
            body.push_str("\n\n");
        }
        let mutating_label = match tool.mutating {
            Some(true) => "Mutating",
            Some(false) => "Read-only",
            None => "Unknown",
        };
        body.push_str(&format!("- **Kind:** {mutating_label}\n"));
        body.push_str(&format!(
            "- **Source:** [`{}`](https://github.com/KodyDennon/versionx/blob/main/{})\n\n",
            tool.file, tool.file
        ));
    }

    body.push_str("## See also\n\n");
    body.push_str("- [MCP server overview](./overview)\n");
    body.push_str("- [Plan / apply cookbook](/sdk/plan-apply-cookbook) — the safety contract every mutating tool participates in.\n");

    std::fs::write(&path, body).with_context(|| format!("writing {path}"))?;
    eprintln!("docs-mcp: wrote {path} ({} tools)", tools.len());
    Ok(())
}

fn discover_modules(dir: &str) -> Result<Vec<(String, Utf8PathBuf)>> {
    let mod_path = Utf8PathBuf::from(dir).join("mod.rs");
    let source =
        std::fs::read_to_string(&mod_path).with_context(|| format!("reading {mod_path}"))?;
    let re = Regex::new(r"pub\s+mod\s+([a-z_][a-z0-9_]*)\s*;").context("compiling mod regex")?;
    let mut out = Vec::new();
    for cap in re.captures_iter(&source) {
        let name = cap.get(1).unwrap().as_str().to_string();
        let file = Utf8PathBuf::from(dir).join(format!("{name}.rs"));
        if file.as_std_path().exists() {
            out.push((name, file));
        }
    }
    Ok(out)
}

fn extract_tool(module: &str, file: &Utf8Path) -> Result<Tool> {
    let source = std::fs::read_to_string(file).with_context(|| format!("reading {file}"))?;
    let parsed: syn::File = syn::parse_file(&source).with_context(|| format!("parsing {file}"))?;
    let module_doc = doc_string(&parsed.attrs);

    let mut tool = Tool {
        module: module.to_string(),
        file: file.to_path_buf(),
        name: None,
        title: None,
        description: None,
        mutating: None,
        module_doc,
    };

    // Find `fn descriptor()` and walk its body for `ToolDescriptor { ... }` literal fields.
    for item in &parsed.items {
        if let Item::Fn(f) = item {
            if f.sig.ident == "descriptor" {
                extract_from_descriptor(&f.block.stmts, &mut tool);
            }
        }
    }

    Ok(tool)
}

fn extract_from_descriptor(stmts: &[Stmt], tool: &mut Tool) {
    for stmt in stmts {
        match stmt {
            Stmt::Expr(expr, _) => walk_expr(expr, tool),
            Stmt::Local(local) => {
                if let Some(init) = &local.init {
                    walk_expr(&init.expr, tool);
                }
            }
            _ => {}
        }
    }
}

fn walk_expr(expr: &Expr, tool: &mut Tool) {
    match expr {
        Expr::Struct(es) => {
            for field in &es.fields {
                if let syn::Member::Named(ident) = &field.member {
                    match ident.to_string().as_str() {
                        "name" => {
                            if let Some(s) = string_lit(&field.expr) {
                                tool.name = Some(s);
                            }
                        }
                        "title" => {
                            if let Some(s) = string_lit(&field.expr) {
                                tool.title = Some(s);
                            }
                        }
                        "description" => {
                            if let Some(s) = string_lit(&field.expr) {
                                tool.description = Some(s);
                            }
                        }
                        "mutating" => {
                            tool.mutating = bool_lit(&field.expr);
                        }
                        _ => {}
                    }
                }
            }
        }
        Expr::Block(b) => {
            for stmt in &b.block.stmts {
                if let Stmt::Expr(e, _) = stmt {
                    walk_expr(e, tool);
                }
            }
        }
        Expr::Call(c) => {
            for arg in &c.args {
                walk_expr(arg, tool);
            }
        }
        Expr::MethodCall(m) => {
            walk_expr(&m.receiver, tool);
            for arg in &m.args {
                walk_expr(arg, tool);
            }
        }
        Expr::Return(r) => {
            if let Some(inner) = &r.expr {
                walk_expr(inner, tool);
            }
        }
        _ => {}
    }
}

fn string_lit(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Lit(ExprLit { lit: Lit::Str(s), .. }) => Some(s.value()),
        _ => None,
    }
}

fn bool_lit(expr: &Expr) -> Option<bool> {
    match expr {
        Expr::Lit(ExprLit { lit: Lit::Bool(b), .. }) => Some(b.value),
        _ => None,
    }
}

// Suppress unused warnings on helpers that might only be used by future
// enhancements.
#[allow(dead_code)]
fn stringify(ty: &syn::Type) -> String {
    ty.to_token_stream().to_string()
}

#[allow(dead_code)]
fn named_fields_text(fields: &Fields) -> Vec<(String, String)> {
    if let Fields::Named(n) = fields {
        n.named
            .iter()
            .filter_map(|f| Some((f.ident.as_ref()?.to_string(), stringify(&f.ty))))
            .collect()
    } else {
        Vec::new()
    }
}
