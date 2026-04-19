//! Policy + waiver TOML schema.
//!
//! A policy file (`.versionx/policies/*.toml` or inline under
//! `[policies]` in `versionx.toml`) contains `[[policy]]` entries and
//! optional `[[waiver]]` entries:
//!
//! ```toml
//! [[policy]]
//! name = "no-ancient-node"
//! kind = "runtime_version"
//! severity = "deny"          # deny | warn | info
//! scope = { all = true }     # all | { tag = "…" } | { path = "…" } | { list = [...] }
//! sealed = false
//! runtime = "node"
//! min = "20"
//!
//! [[waiver]]
//! policy = "no-ancient-node"
//! reason = "Legacy app, Q2 migration"
//! expires_at = 2026-06-01T00:00:00Z
//! owner = "kody@example.com"
//! ```
//!
//! The canonical `Policy` / `Waiver` types here round-trip through
//! `toml` and `serde_json`. Rule-specific fields live under a
//! `#[serde(flatten)]` hashmap so we don't need a new type per kind;
//! the rule evaluators cast into whatever shape they want.

use std::collections::BTreeMap;

use camino::Utf8PathBuf;
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// Maximum schema version this binary writes + fully validates.
pub const SUPPORTED_SCHEMA_VERSION: &str = "1";

/// Root document: one TOML file may contain any mix of policies + waivers.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyDocument {
    #[serde(default = "default_schema_version")]
    pub schema_version: String,
    #[serde(default, rename = "policy", skip_serializing_if = "Vec::is_empty")]
    pub policies: Vec<Policy>,
    #[serde(default, rename = "waiver", skip_serializing_if = "Vec::is_empty")]
    pub waivers: Vec<Waiver>,
}

fn default_schema_version() -> String {
    SUPPORTED_SCHEMA_VERSION.into()
}

/// One declarative policy. Rule-specific fields land in [`fields`] and
/// are decoded lazily by the rule evaluator.
///
/// We deliberately *don't* `deny_unknown_fields` here because
/// rule-specific fields (e.g. `min`, `package`) live under
/// `#[serde(flatten)]` and serde's deny applies before flatten. Typos
/// in the standard fields still fall into `fields`, which the
/// evaluator can flag at runtime.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Policy {
    /// Stable identifier. Referenced by waivers + inheritance.
    pub name: String,
    /// Rule kind. One of the ten built-in kinds or `"custom"` for Luau.
    pub kind: PolicyKind,
    /// How hard the finding bites.
    #[serde(default = "default_severity")]
    pub severity: Severity,
    /// Which components / paths / tags the policy applies to.
    #[serde(default)]
    pub scope: Scope,
    /// When to evaluate. Omitted = every phase.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub triggers: Vec<Trigger>,
    /// When true, downstream repos can't disable this policy.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub sealed: bool,
    /// Optional human-readable explanation shown in policy findings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Rule-specific fields. Kept loose so each evaluator owns its own
    /// parsing.
    #[serde(flatten)]
    pub fields: BTreeMap<String, toml::Value>,
}

/// Canonical rule kind. Unknown kinds (e.g. from a future schema) are
/// accepted but treated as `Custom { label }` when deserialized via
/// JSON bridging; in TOML we stay strict so typos fail loudly.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyKind {
    RuntimeVersion,
    DependencyVersion,
    DependencyPresence,
    AdvisoryBlock,
    ReleaseGate,
    CommitFormat,
    LockfileIntegrity,
    LinkFreshness,
    ProvenanceRequired,
    /// Luau-backed user-defined rule.
    Custom,
}

/// Severity levels.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Deny,
    Warn,
    Info,
}

fn default_severity() -> Severity {
    Severity::Deny
}

/// Where the policy applies. `All` matches every component/path.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    #[serde(default = "default_scope_all")]
    pub all: bool,
    /// Restrict to components whose `tags` (from config) include this.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    /// Restrict to a single path prefix (matched against component root).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<Utf8PathBuf>,
    /// Restrict to an explicit list of component ids.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub list: Vec<String>,
}

impl Default for Scope {
    fn default() -> Self {
        Self { all: true, tag: None, path: None, list: Vec::new() }
    }
}

fn default_scope_all() -> bool {
    true
}

/// Canonical evaluation phases the engine exposes. A policy with no
/// triggers fires in every phase it's relevant to.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Trigger {
    /// Before any bump plan is persisted.
    ReleasePropose,
    /// Before apply writes to disk.
    ReleaseApply,
    /// During `versionx sync`.
    Sync,
    /// Ad-hoc `versionx policy check`.
    Check,
}

/// One waiver = a time-boxed exception to a specific policy.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Waiver {
    /// Policy name this waiver targets.
    pub policy: String,
    /// Human-facing reason. Required — silent waivers hide rot.
    pub reason: String,
    /// Mandatory expiry. Checked by [`Waiver::is_live`] at evaluation
    /// time and warned about 7 days in advance.
    ///
    /// Accepts both a TOML native datetime literal
    /// (`expires_at = 2026-12-31T00:00:00Z`) and an RFC-3339 string
    /// (`"2026-12-31T00:00:00Z"`).
    #[serde(with = "flex_datetime")]
    pub expires_at: DateTime<Utc>,
    /// Who approved the waiver (email / handle).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    /// Optional scope override — if omitted, the waiver applies
    /// wherever the underlying policy scope matches.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<Scope>,
}

impl Waiver {
    /// Still valid at `now`?
    #[must_use]
    pub fn is_live(&self, now: DateTime<Utc>) -> bool {
        self.expires_at > now
    }

    /// Days remaining until expiry. Negative = already expired.
    #[must_use]
    pub fn days_until_expiry(&self, now: DateTime<Utc>) -> i64 {
        (self.expires_at - now).num_days()
    }
}

/// Parse a TOML source into a [`PolicyDocument`].
pub fn from_toml(source: &str) -> Result<PolicyDocument, PolicyParseError> {
    let doc: PolicyDocument = toml::from_str(source).map_err(PolicyParseError::Toml)?;
    validate(&doc)?;
    Ok(doc)
}

/// Render to a TOML string.
pub fn to_toml(doc: &PolicyDocument) -> Result<String, PolicyParseError> {
    toml::to_string_pretty(doc).map_err(PolicyParseError::TomlSer)
}

/// Post-parse invariant checks that serde can't encode.
fn validate(doc: &PolicyDocument) -> Result<(), PolicyParseError> {
    // Policy name uniqueness.
    let mut seen: IndexMap<&str, ()> = IndexMap::new();
    for p in &doc.policies {
        if seen.contains_key(p.name.as_str()) {
            return Err(PolicyParseError::DuplicateName { name: p.name.clone() });
        }
        seen.insert(&p.name, ());
    }
    // Waiver targets resolving to a policy name that isn't in this
    // document isn't a hard error — the waiver might point at a
    // sibling file — so we leave that enforcement to the engine level.
    let _ = &doc.waivers;
    Ok(())
}

/// Serde adapter that accepts either a TOML datetime literal or an
/// RFC-3339 string and normalizes to [`DateTime<Utc>`].
mod flex_datetime {
    use chrono::{DateTime, Utc};
    use serde::{Deserialize, Deserializer, Serializer};

    pub(super) fn serialize<S: Serializer>(dt: &DateTime<Utc>, s: S) -> Result<S::Ok, S::Error> {
        // Always serialize as RFC-3339 string so the output is portable
        // across both TOML and JSON consumers.
        s.serialize_str(&dt.to_rfc3339())
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<DateTime<Utc>, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Shape {
            Native(toml::value::Datetime),
            Str(String),
        }
        match Shape::deserialize(d)? {
            Shape::Native(dt) => {
                let s = dt.to_string();
                DateTime::parse_from_rfc3339(&s)
                    .map(|d| d.with_timezone(&Utc))
                    .map_err(serde::de::Error::custom)
            }
            Shape::Str(s) => DateTime::parse_from_rfc3339(&s)
                .map(|d| d.with_timezone(&Utc))
                .map_err(serde::de::Error::custom),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PolicyParseError {
    #[error("toml parse error: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("toml serialize error: {0}")]
    TomlSer(#[from] toml::ser::Error),
    #[error("duplicate policy name: {name}")]
    DuplicateName { name: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_minimal_runtime_rule() {
        let src = r#"
            schema_version = "1"

            [[policy]]
            name = "no-ancient-node"
            kind = "runtime_version"
            severity = "deny"
            runtime = "node"
            min = "20"
        "#;
        let doc = from_toml(src).unwrap();
        assert_eq!(doc.policies.len(), 1);
        let p = &doc.policies[0];
        assert_eq!(p.name, "no-ancient-node");
        assert_eq!(p.kind, PolicyKind::RuntimeVersion);
        assert_eq!(p.severity, Severity::Deny);
        assert!(p.scope.all);
        assert_eq!(p.fields.get("runtime").unwrap().as_str(), Some("node"));
        assert_eq!(p.fields.get("min").unwrap().as_str(), Some("20"));
    }

    #[test]
    fn duplicate_names_fail() {
        let src = r#"
            [[policy]]
            name = "dup"
            kind = "release_gate"

            [[policy]]
            name = "dup"
            kind = "commit_format"
        "#;
        let err = from_toml(src).unwrap_err();
        assert!(matches!(err, PolicyParseError::DuplicateName { .. }));
    }

    #[test]
    fn waiver_round_trip() {
        let src = r#"
            [[policy]]
            name = "gate"
            kind = "release_gate"

            [[waiver]]
            policy = "gate"
            reason = "in-flight migration"
            expires_at = 2026-12-31T00:00:00Z
            owner = "ops@example.com"
        "#;
        let doc = from_toml(src).unwrap();
        assert_eq!(doc.waivers.len(), 1);
        assert!(doc.waivers[0].is_live(Utc::now()));
    }

    #[test]
    fn severity_variants_serialize_lowercase() {
        let policy = Policy {
            name: "x".into(),
            kind: PolicyKind::CommitFormat,
            severity: Severity::Warn,
            scope: Scope::default(),
            triggers: vec![],
            sealed: false,
            message: None,
            fields: BTreeMap::new(),
        };
        let doc = PolicyDocument { policies: vec![policy], ..Default::default() };
        let ser = to_toml(&doc).unwrap();
        assert!(ser.contains("severity = \"warn\""));
    }

    #[test]
    fn unknown_kind_rejected() {
        let src = r#"
            [[policy]]
            name = "x"
            kind = "teleport_users"
        "#;
        let err = from_toml(src).unwrap_err();
        assert!(matches!(err, PolicyParseError::Toml(_)));
    }

    #[test]
    fn waiver_expiry_countdown() {
        let w = Waiver {
            policy: "x".into(),
            reason: "y".into(),
            expires_at: Utc::now() + chrono::Duration::days(3),
            owner: None,
            scope: None,
        };
        let days = w.days_until_expiry(Utc::now());
        assert!((2..=3).contains(&days));
    }
}
