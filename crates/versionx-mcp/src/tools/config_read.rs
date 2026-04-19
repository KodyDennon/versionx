//! `config_read` — mirror of the `versionx://config` resource.

use serde_json::{Value, json};

use super::{ToolDescriptor, ToolOutput, resolve_root};
use crate::McpResult;
use crate::context::McpContext;
use crate::sanitize::fence_untrusted;

#[must_use]
pub fn descriptor() -> ToolDescriptor {
    ToolDescriptor {
        name: "config_read",
        title: "Read versionx.toml",
        description: "Return the contents of the workspace's `versionx.toml`. Treated as \
                      trusted (first-party config), but still fenced to keep downstream \
                      models consistent about provenance.",
        input_schema: json!({
            "type": "object",
            "properties": { "root": { "type": "string" } },
        }),
        mutating: false,
    }
}

pub fn call(params: Value, ctx: &McpContext) -> McpResult<ToolOutput> {
    let root = resolve_root(&params, ctx);
    let path = root.join("versionx.toml");
    let raw = std::fs::read_to_string(path.as_std_path())
        .map_err(|e| crate::McpError::Tool(format!("reading {path}: {e}")))?;
    let structured = json!({ "path": path.to_string(), "contents": raw });
    let summary = format!("read {} bytes from {path}", raw.len());
    // Fence for visual/prog parity with other file-reading tools.
    let mut out = ToolOutput::ok(summary, structured);
    out.summary = fence_untrusted(&out.summary);
    Ok(out)
}
