//! JSON-RPC method reference generator.
//!
//! Parses `crates/versionx-daemon/src/protocol.rs` and pulls every
//! `pub const X: &str = "..."` from the `methods` and `notifications`
//! submodules. Each constant's doc comment (if any) becomes the entry's
//! description.

use anyhow::{Context, Result};
use camino::Utf8Path;
use syn::{Expr, ExprLit, Item, Lit};

use super::{banner, syn_util::doc_string};

const SOURCE: &str = "crates/versionx-daemon/src/protocol.rs";

#[derive(Debug)]
struct Entry {
    ident: String,
    value: String,
    doc: String,
}

pub(super) fn generate(root: &Utf8Path) -> Result<()> {
    let dir = root.join("integrations");
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {dir}"))?;
    let path = dir.join("json-rpc-methods.md");

    let source = std::fs::read_to_string(SOURCE).with_context(|| format!("reading {SOURCE}"))?;
    let file: syn::File = syn::parse_file(&source).with_context(|| format!("parsing {SOURCE}"))?;

    let methods = collect_consts(&file.items, "methods");
    let notifications = collect_consts(&file.items, "notifications");

    let mut body = String::new();
    body.push_str("---\n");
    body.push_str("title: JSON-RPC methods & notifications\n");
    body.push_str(
        "description: Every JSON-RPC 2.0 method and notification the versiond \
         daemon understands, extracted from the protocol module.\n",
    );
    body.push_str("sidebar_position: 4\n");
    body.push_str("---\n\n");
    body.push_str(&banner("rpc"));
    body.push_str("\n# JSON-RPC methods & notifications\n\n");
    body.push_str(&format!("_Generated from `{SOURCE}`._\n\n",));

    body.push_str(
        "The `versiond` daemon speaks JSON-RPC 2.0 over a per-user local \
         socket (Unix Domain Socket on Linux/macOS; named pipe on Windows). \
         Messages are length-prefixed (4-byte BE) with a maximum frame size \
         of 2 MiB. See [JSON-RPC daemon](./json-rpc-daemon) for the transport \
         overview and framing details.\n\n",
    );

    body.push_str("## Methods\n\n");
    if methods.is_empty() {
        body.push_str("_No method constants detected._\n");
    } else {
        body.push_str("| Constant | Method name | Description |\n|---|---|---|\n");
        for e in &methods {
            body.push_str(&format!(
                "| `{ident}` | `{value}` | {desc} |\n",
                ident = e.ident,
                value = e.value,
                desc = super::syn_util::table_escape(&e.doc),
            ));
        }
    }

    body.push_str("\n## Notifications\n\n");
    if notifications.is_empty() {
        body.push_str("_No notification constants detected._\n");
    } else {
        body.push_str("| Constant | Notification name | Description |\n|---|---|---|\n");
        for e in &notifications {
            body.push_str(&format!(
                "| `{ident}` | `{value}` | {desc} |\n",
                ident = e.ident,
                value = e.value,
                desc = super::syn_util::table_escape(&e.doc),
            ));
        }
    }

    body.push_str("\n## Error codes\n\n");
    body.push_str(
        "Standard JSON-RPC 2.0 error codes (`-32700` parse error, `-32600` \
         invalid request, `-32601` method not found, `-32602` invalid params, \
         `-32603` internal error) apply. Application-level codes are \
         reserved in the `-32000..=-32099` range. Current in-use codes: \n\n",
    );
    body.push_str("| Code | Meaning |\n|---|---|\n");
    body.push_str("| `-32001` | Shutting down. |\n");
    body.push_str("| `-32002` | Busy. |\n");
    body.push_str("| `-32003` | Workspace failed. |\n\n");

    body.push_str("## See also\n\n");
    body.push_str("- [JSON-RPC daemon](./json-rpc-daemon) — transport and framing detail.\n");
    body.push_str("- [HTTP API](./http-api) — the same surface over HTTP.\n");
    body.push_str(
        "- [MCP server overview](./mcp/overview) — the agent-facing layer on top of the daemon.\n",
    );

    std::fs::write(&path, body).with_context(|| format!("writing {path}"))?;
    eprintln!(
        "docs-rpc: wrote {path} ({} methods, {} notifications)",
        methods.len(),
        notifications.len(),
    );
    Ok(())
}

fn collect_consts(items: &[Item], module_name: &str) -> Vec<Entry> {
    let module = items.iter().find_map(|item| match item {
        Item::Mod(m) if m.ident == module_name => m.content.as_ref(),
        _ => None,
    });
    let Some((_, body)) = module else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for item in body {
        if let Item::Const(c) = item {
            let doc = doc_string(&c.attrs);
            let value = match &*c.expr {
                Expr::Lit(ExprLit { lit: Lit::Str(s), .. }) => s.value(),
                _ => continue,
            };
            out.push(Entry { ident: c.ident.to_string(), value, doc });
        }
    }
    out.sort_by(|a, b| a.value.cmp(&b.value));
    out
}
