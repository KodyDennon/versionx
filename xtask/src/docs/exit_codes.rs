//! Exit-codes reference generator.
//!
//! Parses `crates/versionx-core/src/error.rs`, walks the `CoreError` enum,
//! and emits one row per variant with its docstring and `#[error(...)]`
//! template — plus the stable top-level exit-code table that maps classes
//! of error to exit codes.

use anyhow::{Context, Result};
use camino::Utf8Path;
use quote::ToTokens;
use syn::{Attribute, Expr, ExprLit, Fields, Item, ItemEnum, Lit, Meta};

use super::{banner, syn_util::doc_string};

const SOURCE: &str = "crates/versionx-core/src/error.rs";
const ENUM_NAME: &str = "CoreError";

pub(super) fn generate(root: &Utf8Path) -> Result<()> {
    let dir = root.join("reference");
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {dir}"))?;
    let path = dir.join("exit-codes.md");

    let source = std::fs::read_to_string(SOURCE).with_context(|| format!("reading {SOURCE}"))?;
    let file: syn::File = syn::parse_file(&source).with_context(|| format!("parsing {SOURCE}"))?;

    let target = find_enum(&file.items, ENUM_NAME)
        .with_context(|| format!("enum {ENUM_NAME} not found in {SOURCE}"))?;

    let mut body = String::new();
    body.push_str("---\n");
    body.push_str("title: Exit codes\n");
    body.push_str(
        "description: Every exit code Versionx can return, with the error \
         variant that produces it.\n",
    );
    body.push_str("sidebar_position: 8\n");
    body.push_str("---\n\n");
    body.push_str(&banner("exit-codes"));
    body.push_str("\n# Exit codes\n\n");
    body.push_str(&format!(
        "_Generated from `{SOURCE}` (`{ENUM_NAME}` enum). Run `cargo xtask \
         docs --only exit-codes` after changing error variants._\n\n",
    ));

    body.push_str("## Stable top-level codes\n\n");
    body.push_str("| Code | Meaning |\n|---|---|\n");
    body.push_str("| `0` | Success. |\n");
    body.push_str("| `1` | Generic user error. Message on stderr describes it. |\n");
    body.push_str(
        "| `2` | Config error. `versionx.toml` is missing, unparseable, or fails validation. |\n",
    );
    body.push_str("| `3` | Policy violation. Rerun with `--explain` for specifics. |\n");
    body.push_str("| `4` | Network or I/O error. Check `VERSIONX_LOG=debug`. |\n");
    body.push_str("| `5` | Prerequisite mismatch during `apply`. Regenerate the plan. |\n");
    body.push_str("| `6` | Git error. |\n");
    body.push_str("| `7` | Daemon unavailable and `--no-daemon` was not set. |\n");
    body.push_str("| `8` | Waiver expired. |\n");
    body.push_str("| `9` | Saga failure. `versionx saga status` for recovery. |\n");
    body.push_str("| `10+` | Subsystem-specific — see the `CoreError` variants below. |\n\n");

    body.push_str("## `CoreError` variants\n\n");
    body.push_str("| Variant | Message template | Description |\n|---|---|---|\n");

    for variant in &target.variants {
        let name = variant.ident.to_string();
        let doc = doc_string(&variant.attrs);
        let error_template = error_template(&variant.attrs).unwrap_or_default();
        let fields = fields_summary(&variant.fields);
        let variant_label =
            if fields.is_empty() { format!("`{name}`") } else { format!("`{name}` {fields}") };

        body.push_str(&format!(
            "| {variant} | `{template}` | {desc} |\n",
            variant = super::syn_util::table_escape(&variant_label),
            template = super::syn_util::table_escape(&error_template),
            desc = super::syn_util::table_escape(&doc),
        ));
    }

    body.push_str("\n## In scripts\n\n");
    body.push_str("```bash\n");
    body.push_str("versionx release plan > plan.json\n");
    body.push_str("rc=$?\n");
    body.push_str("if [ \"$rc\" -eq 3 ]; then\n");
    body.push_str("    echo \"Policy violation. See above.\"\n");
    body.push_str("    exit 1\n");
    body.push_str("fi\n");
    body.push_str("```\n\n");

    body.push_str("## See also\n\n");
    body.push_str("- [Environment variables](./environment-variables) — turn on `VERSIONX_LOG=debug` when diagnosing.\n");
    body.push_str("- [Debugging & tracing](/contributing/debugging-and-tracing) — for contributors adding new error variants.\n");

    std::fs::write(&path, body).with_context(|| format!("writing {path}"))?;
    eprintln!("docs-exit-codes: wrote {path}");
    Ok(())
}

fn find_enum<'a>(items: &'a [Item], name: &str) -> Option<&'a ItemEnum> {
    items.iter().find_map(|item| match item {
        Item::Enum(e) if e.ident == name => Some(e),
        _ => None,
    })
}

fn error_template(attrs: &[Attribute]) -> Option<String> {
    for attr in attrs {
        if !attr.path().is_ident("error") {
            continue;
        }
        // #[error("...")] or #[error(transparent)]
        if let Meta::List(list) = &attr.meta {
            let tokens = list.tokens.to_string();
            let trimmed = tokens.trim();
            if trimmed == "transparent" {
                return Some("<delegated>".into());
            }
            // Try to parse as a string literal.
            if let Ok(lit) = syn::parse_str::<syn::LitStr>(trimmed) {
                return Some(lit.value());
            }
            return Some(trimmed.to_string());
        }
        // #[error = "..."] (unusual but possible)
        if let Meta::NameValue(nv) = &attr.meta {
            if let Expr::Lit(ExprLit { lit: Lit::Str(s), .. }) = &nv.value {
                return Some(s.value());
            }
        }
    }
    None
}

fn fields_summary(fields: &Fields) -> String {
    match fields {
        Fields::Unit => String::new(),
        Fields::Unnamed(un) => {
            let parts: Vec<String> = un
                .unnamed
                .iter()
                .map(|f| f.ty.to_token_stream().to_string())
                .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "))
                .collect();
            format!("({})", parts.join(", "))
        }
        Fields::Named(n) => {
            let parts: Vec<String> = n
                .named
                .iter()
                .filter_map(|f| {
                    let ident = f.ident.as_ref()?;
                    let ty = f.ty.to_token_stream().to_string();
                    Some(format!(
                        "{ident}: {ty}",
                        ty = ty.split_whitespace().collect::<Vec<_>>().join(" ")
                    ))
                })
                .collect();
            format!("{{ {} }}", parts.join(", "))
        }
    }
}
