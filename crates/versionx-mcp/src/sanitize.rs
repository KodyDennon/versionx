//! Output sanitization for prompt-injection defense.
//!
//! Any text a tool returns that originated from an untrusted source
//! (commit messages, file contents, dependency descriptions) must be
//! fenced so the consuming model can visually — and programmatically —
//! distinguish it from server instructions.
//!
//! Wrapping strategy:
//! ````text
//! <!-- UNTRUSTED_INPUT_BEGIN id={uuid} -->
//! ```
//! {raw content — any backticks inside get escaped}
//! ```
//! <!-- UNTRUSTED_INPUT_END id={uuid} -->
//! ````
//!
//! The `id` in the begin/end markers is unique per call so adversarial
//! content can't forge matching end markers (they'd need the runtime
//! UUID).

use uuid::Uuid;

/// Wrap `raw` in fenced markers that callers can trust will match
/// end-for-end. Escapes any backticks inside the fence by prepending a
/// zero-width space, keeping the visual form intact.
#[must_use]
pub fn fence_untrusted(raw: &str) -> String {
    let id = Uuid::now_v7();
    // Zero-width space = U+200B, which renders as nothing but still
    // breaks up a literal triple-backtick run.
    let safe = raw.replace("```", "\u{200b}```");
    format!(
        "<!-- UNTRUSTED_INPUT_BEGIN id={id} -->\n```\n{safe}\n```\n<!-- UNTRUSTED_INPUT_END id={id} -->",
    )
}

/// Short label to flag the following content as untrusted for text-only
/// consumers. Used when the caller wants inline annotations instead of a
/// full fence.
pub const UNTRUSTED_PREFIX: &str = "⚠ untrusted-input: ";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fence_wraps_content() {
        let out = fence_untrusted("hello world");
        assert!(out.contains("UNTRUSTED_INPUT_BEGIN"));
        assert!(out.contains("UNTRUSTED_INPUT_END"));
        assert!(out.contains("hello world"));
    }

    #[test]
    fn fence_escapes_nested_fences() {
        let out = fence_untrusted("```rust\nfn x() {}\n```");
        // The nested triple-backtick run should be broken up.
        assert!(out.contains("\u{200b}```"));
    }

    #[test]
    fn begin_end_ids_match() {
        let out = fence_untrusted("x");
        let begin = out.find("UNTRUSTED_INPUT_BEGIN id=").unwrap();
        let end = out.find("UNTRUSTED_INPUT_END id=").unwrap();
        let begin_id = &out[begin + "UNTRUSTED_INPUT_BEGIN id=".len()..begin + 61];
        let end_id = &out[end + "UNTRUSTED_INPUT_END id=".len()..end + 59];
        assert_eq!(begin_id, end_id);
    }
}
