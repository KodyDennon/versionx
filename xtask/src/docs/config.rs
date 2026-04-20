//! `versionx.toml` schema reference generator.
//!
//! Parses `crates/versionx-config/src/schema.rs` as a syn AST, walks every
//! `pub struct` and `pub enum`, extracts field names, types, serde attrs,
//! and doc comments, and produces a single Reference page.

use anyhow::{Context, Result};
use camino::Utf8Path;
use quote::ToTokens;
use syn::{Attribute, Expr, ExprLit, Fields, Item, Lit, LitStr, Meta, MetaNameValue, Type};

use super::{banner, syn_util::doc_string};

const SOURCE: &str = "crates/versionx-config/src/schema.rs";

pub(super) fn generate(root: &Utf8Path) -> Result<()> {
    let dir = root.join("reference");
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {dir}"))?;
    let path = dir.join("versionx-toml.md");

    let source = std::fs::read_to_string(SOURCE).with_context(|| format!("reading {SOURCE}"))?;
    let file: syn::File = syn::parse_file(&source).with_context(|| format!("parsing {SOURCE}"))?;

    let mut body = String::new();
    body.push_str("---\n");
    body.push_str("title: versionx.toml reference\n");
    body.push_str(
        "description: Every top-level section, every field, every default \
         value for the Versionx configuration file. Auto-generated from \
         the versionx-config crate.\n",
    );
    body.push_str("sidebar_position: 3\n");
    body.push_str("---\n\n");
    body.push_str(&banner("config"));
    body.push_str("\n# `versionx.toml`\n\n");
    body.push_str(&format!(
        "_Generated from `{SOURCE}`. Edit the schema there and re-run \
         `cargo xtask docs` to update this page._\n\n",
    ));
    body.push_str(
        "`versionx.toml` is the primary configuration file. One lives at \
         the root of every repo that uses Versionx. Unknown top-level keys \
         are rejected so typos get caught.\n\n",
    );

    // Render the root VersionxConfig first if present, then every other pub item.
    let (root_item, rest) = split_root(&file.items, "VersionxConfig");
    if let Some(root_struct) = root_item {
        render_item(&mut body, root_struct, /* is_root = */ true);
    }
    for item in rest {
        render_item(&mut body, item, /* is_root = */ false);
    }

    body.push_str("\n## See also\n\n");
    body.push_str("- [`versionx.lock` reference](./versionx-lock)\n");
    body.push_str("- [Environment variables](./environment-variables)\n");
    body.push_str("- [CLI reference](./cli/versionx)\n");

    std::fs::write(&path, body).with_context(|| format!("writing {path}"))?;
    eprintln!("docs-config: wrote {path}");
    Ok(())
}

fn split_root<'a>(items: &'a [Item], root_name: &str) -> (Option<&'a Item>, Vec<&'a Item>) {
    let mut root = None;
    let mut rest = Vec::new();
    for item in items {
        if root.is_none() && matches!(item, Item::Struct(s) if s.ident == root_name) {
            root = Some(item);
        } else if is_public_doc_item(item) {
            rest.push(item);
        }
    }
    (root, rest)
}

fn is_public_doc_item(item: &Item) -> bool {
    match item {
        Item::Struct(s) => matches!(s.vis, syn::Visibility::Public(_)),
        Item::Enum(e) => matches!(e.vis, syn::Visibility::Public(_)),
        _ => false,
    }
}

fn render_item(body: &mut String, item: &Item, is_root: bool) {
    match item {
        Item::Struct(s) => render_struct(body, s, is_root),
        Item::Enum(e) => render_enum(body, e),
        _ => {}
    }
}

fn render_struct(body: &mut String, s: &syn::ItemStruct, is_root: bool) {
    let level = if is_root { "##" } else { "###" };
    body.push_str(&format!("\n{level} `{}`\n\n", s.ident));

    let doc = doc_string(&s.attrs);
    if !doc.is_empty() {
        body.push_str(&doc);
        body.push_str("\n\n");
    }

    let Fields::Named(fields) = &s.fields else {
        return;
    };
    if fields.named.is_empty() {
        return;
    }

    body.push_str("| Field | Type | Serde | Default | Description |\n");
    body.push_str("|---|---|---|---|---|\n");

    for field in &fields.named {
        let Some(ident) = field.ident.as_ref() else { continue };
        let ty = type_string(&field.ty);
        let doc = doc_string(&field.attrs);
        let serde = serde_attrs(&field.attrs);
        let default = serde
            .iter()
            .find_map(|kv| if kv.0 == "default" { Some(kv.1.clone()) } else { None })
            .unwrap_or_default();
        let rename =
            serde.iter().find_map(|kv| if kv.0 == "rename" { Some(kv.1.clone()) } else { None });
        let flatten = serde.iter().any(|kv| kv.0 == "flatten");
        let skip_none = serde.iter().any(|kv| kv.0 == "skip_serializing_if");

        let serde_cell = {
            let mut hints = Vec::new();
            if let Some(r) = rename {
                hints.push(format!("rename: `{r}`"));
            }
            if flatten {
                hints.push("flatten".into());
            }
            if skip_none {
                hints.push("optional".into());
            }
            hints.join(", ")
        };

        body.push_str(&format!(
            "| `{ident}` | `{ty}` | {serde} | {default} | {desc} |\n",
            ty = super::syn_util::table_escape(&ty),
            serde = super::syn_util::table_escape(&serde_cell),
            default = super::syn_util::table_escape(&default),
            desc = super::syn_util::table_escape(&doc),
        ));
    }
}

fn render_enum(body: &mut String, e: &syn::ItemEnum) {
    body.push_str(&format!("\n### `{}` (enum)\n\n", e.ident));
    let doc = doc_string(&e.attrs);
    if !doc.is_empty() {
        body.push_str(&doc);
        body.push_str("\n\n");
    }
    body.push_str("| Variant | Description |\n|---|---|\n");
    for v in &e.variants {
        let d = doc_string(&v.attrs);
        body.push_str(&format!(
            "| `{name}` | {desc} |\n",
            name = v.ident,
            desc = super::syn_util::table_escape(&d),
        ));
    }
}

fn type_string(ty: &Type) -> String {
    // Compact the tokens so line-broken types collapse to a single line.
    let s = ty.to_token_stream().to_string();
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Extract all serde `#[serde(key = "value")]` and `#[serde(key)]` attrs
/// as flat key/value pairs.
fn serde_attrs(attrs: &[Attribute]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for attr in attrs {
        if !attr.path().is_ident("serde") {
            continue;
        }
        let _ = attr.parse_nested_meta(|meta| {
            let key = meta.path.get_ident().map(|i| i.to_string()).unwrap_or_default();
            if meta.input.peek(syn::Token![=]) {
                let value_stream = meta.value()?;
                // Accept string literals; anything else (paths like `is_empty`)
                // we stringify via the parsed expression's tokens.
                if let Ok(lit) = value_stream.parse::<LitStr>() {
                    out.push((key, lit.value()));
                } else if let Ok(expr) = value_stream.parse::<Expr>() {
                    let s = match expr {
                        Expr::Lit(ExprLit { lit: Lit::Str(s), .. }) => s.value(),
                        other => other.to_token_stream().to_string(),
                    };
                    out.push((key, s));
                }
            } else {
                out.push((key, String::new()));
            }
            Ok(())
        });
    }
    // Also catch the `#[serde(default, skip_serializing_if = "...")]` paired form.
    // Already handled by parse_nested_meta.
    out
}

#[allow(dead_code)]
fn doc_from_name_value(meta: &Meta) -> Option<String> {
    if let Meta::NameValue(MetaNameValue {
        value: Expr::Lit(ExprLit { lit: Lit::Str(s), .. }),
        ..
    }) = meta
    {
        Some(s.value())
    } else {
        None
    }
}
