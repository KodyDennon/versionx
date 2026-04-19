//! Read-only MCP resources.
//!
//! Resources are URL-addressable blobs. We expose:
//!   - `versionx://config` — `versionx.toml` contents.
//!   - `versionx://state/lockfile` — `versionx.lock` parsed JSON.
//!   - `versionx://state/plans` — list of release plans.
//!   - `versionx://state/policy-lock` — `versionx.policy.lock`.
//!
//! Every resource is also mirrored as a tool (`config_read`,
//! `state_read`) so clients that don't surface resources still get the
//! data.

use camino::Utf8Path;
use serde::Serialize;
use serde_json::{Value, json};

#[derive(Clone, Debug, Serialize)]
pub struct ResourceDescriptor {
    pub uri: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub mime_type: &'static str,
}

#[must_use]
pub fn descriptors() -> Vec<ResourceDescriptor> {
    vec![
        ResourceDescriptor {
            uri: "versionx://config",
            name: "versionx.toml",
            description: "The workspace configuration file.",
            mime_type: "application/toml",
        },
        ResourceDescriptor {
            uri: "versionx://state/lockfile",
            name: "versionx.lock",
            description: "Resolved runtime + ecosystem + component versions.",
            mime_type: "application/toml",
        },
        ResourceDescriptor {
            uri: "versionx://state/plans",
            name: "Release plans",
            description: "Every persisted release plan under .versionx/plans/.",
            mime_type: "application/json",
        },
        ResourceDescriptor {
            uri: "versionx://state/policy-lock",
            name: "versionx.policy.lock",
            description: "Inherited-policy content-hash pins.",
            mime_type: "application/toml",
        },
    ]
}

/// Fetch the body of a resource. `content` is text (TOML or JSON).
pub fn read(uri: &str, root: &Utf8Path) -> Option<(String, String)> {
    match uri {
        "versionx://config" => {
            let path = root.join("versionx.toml");
            let body = std::fs::read_to_string(path.as_std_path()).ok()?;
            Some((body, "application/toml".into()))
        }
        "versionx://state/lockfile" => {
            let path = root.join("versionx.lock");
            let body = std::fs::read_to_string(path.as_std_path()).ok()?;
            Some((body, "application/toml".into()))
        }
        "versionx://state/plans" => {
            let dir = versionx_release::plans_dir(root);
            let plans = versionx_release::list_plans(&dir).ok()?;
            Some((serde_json::to_string_pretty(&plans).ok()?, "application/json".into()))
        }
        "versionx://state/policy-lock" => {
            let path = root.join("versionx.policy.lock");
            let body = std::fs::read_to_string(path.as_std_path()).ok()?;
            Some((body, "application/toml".into()))
        }
        _ => None,
    }
}

/// MCP `resources/read` response shape.
pub fn to_read_response(uri: &str, contents: &str, mime: &str) -> Value {
    json!({
        "contents": [{
            "uri": uri,
            "mimeType": mime,
            "text": contents,
        }]
    })
}
