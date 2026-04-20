//! Shared helpers for syn-based generators.

use syn::{Attribute, Expr, ExprLit, Lit, Meta, MetaNameValue};

/// Extract rustdoc text from a slice of attributes.
///
/// Concatenates every `#[doc = "..."]` line with a single newline between
/// them, dedents, and trims. Returns an empty string when there are no doc
/// attributes.
pub(crate) fn doc_string(attrs: &[Attribute]) -> String {
    let lines: Vec<String> = attrs
        .iter()
        .filter_map(|attr| {
            if !attr.path().is_ident("doc") {
                return None;
            }
            match &attr.meta {
                Meta::NameValue(MetaNameValue {
                    value: Expr::Lit(ExprLit { lit: Lit::Str(s), .. }),
                    ..
                }) => {
                    let raw = s.value();
                    let trimmed = raw.strip_prefix(' ').unwrap_or(&raw);
                    Some(trimmed.to_string())
                }
                _ => None,
            }
        })
        .collect();

    lines.join("\n").trim().to_string()
}

/// Escape a string for embedding in a Markdown table cell — collapse
/// newlines to spaces, escape pipes. Also escapes `{` and `}` so MDX
/// doesn't try to parse them as JSX expression delimiters.
pub(crate) fn table_escape(s: &str) -> String {
    s.replace('\n', " ")
        .replace('|', "\\|")
        .replace('{', "\\{")
        .replace('}', "\\}")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .trim()
        .to_string()
}
