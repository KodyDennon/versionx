//! `release_gate` — generic "require condition X before release" gate.
//!
//! ```toml
//! [[policy]]
//! name = "rfc-required-for-major"
//! kind = "release_gate"
//! require_changelog = true      # a non-empty CHANGELOG section for the new version
//! max_level = "minor"           # refuse bumps above this level
//! ```
//!
//! Any single field can be set independently. Missing fields mean "no
//! opinion on that check".

use super::{field_bool, field_str};
use crate::context::PolicyContext;
use crate::finding::Finding;
use crate::schema::{Policy, PolicyKind, Trigger};

pub fn evaluate(policy: &Policy, ctx: &PolicyContext) -> Vec<Finding> {
    // Only meaningful during release_propose / release_apply.
    match ctx.trigger {
        Some(Trigger::ReleasePropose | Trigger::ReleaseApply) => {}
        _ => return Vec::new(),
    }
    let mut findings = Vec::new();

    if field_bool(policy, "require_changelog", false) {
        // We proxy on "are there commit messages"? Without commits the
        // changelog would be empty, which is what this gate is about.
        if ctx.commits.is_empty() {
            findings.push(finding(
                policy,
                "release_gate: no changelog entries (no commits since last release)",
            ));
        }
    }

    if let Some(max_level) = field_str(policy, "max_level") {
        // Caller populates `advisories` map with a synthetic
        // `proposed_bump_level = "major"` when firing from propose, so
        // we can centralize the check. We look for a `_proposed_level`
        // entry in the context-provided advisories.
        if let Some(level) = ctx.advisories.get("_proposed_level")
            && rank(level) > rank(max_level)
        {
            findings.push(finding(
                policy,
                &format!("release_gate: proposed level `{level}` exceeds `{max_level}`"),
            ));
        }
    }

    findings
}

fn rank(s: &str) -> u8 {
    match s {
        "patch" => 0,
        "minor" => 1,
        "major" => 2,
        _ => 0,
    }
}

fn finding(policy: &Policy, message: &str) -> Finding {
    Finding {
        policy: policy.name.clone(),
        kind: PolicyKind::ReleaseGate,
        severity: policy.severity,
        component: None,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{Policy, PolicyKind, Scope, Severity, Trigger};
    use std::collections::BTreeMap;

    fn policy(fields: &[(&str, toml::Value)]) -> Policy {
        let mut m = BTreeMap::new();
        for (k, v) in fields {
            m.insert((*k).into(), v.clone());
        }
        Policy {
            name: "gate".into(),
            kind: PolicyKind::ReleaseGate,
            severity: Severity::Deny,
            scope: Scope::default(),
            triggers: vec![],
            sealed: false,
            message: None,
            fields: m,
        }
    }

    #[test]
    fn require_changelog_blocks_empty_propose() {
        let p = policy(&[("require_changelog".into(), toml::Value::Boolean(true))]);
        let mut ctx = PolicyContext::new("/tmp".into());
        ctx.trigger = Some(Trigger::ReleasePropose);
        assert_eq!(evaluate(&p, &ctx).len(), 1);
    }

    #[test]
    fn max_level_caps_major() {
        let p = policy(&[("max_level".into(), toml::Value::String("minor".into()))]);
        let mut ctx = PolicyContext::new("/tmp".into());
        ctx.trigger = Some(Trigger::ReleasePropose);
        ctx.advisories.insert("_proposed_level".into(), "major".into());
        let f = evaluate(&p, &ctx);
        assert_eq!(f.len(), 1);
    }

    #[test]
    fn not_evaluated_outside_release() {
        let p = policy(&[("require_changelog".into(), toml::Value::Boolean(true))]);
        let ctx = PolicyContext::new("/tmp".into());
        assert!(evaluate(&p, &ctx).is_empty());
    }
}
