//! `link_freshness` — fail if an external link hasn't been updated in
//! `max_age_days` days.
//!
//! ```toml
//! [[policy]]
//! name = "stale-link-check"
//! kind = "link_freshness"
//! max_age_days = 30
//! ```

use super::field_i64;
use crate::context::PolicyContext;
use crate::finding::Finding;
use crate::schema::{Policy, PolicyKind};

pub fn evaluate(policy: &Policy, ctx: &PolicyContext) -> Vec<Finding> {
    let max_age = field_i64(policy, "max_age_days").unwrap_or(30);
    let mut findings = Vec::new();
    for (name, link) in &ctx.links {
        let Some(age) = link.age_days else { continue };
        if age > max_age {
            findings.push(Finding {
                policy: policy.name.clone(),
                kind: PolicyKind::LinkFreshness,
                severity: policy.severity,
                component: None,
                message: format!("link `{name}` is {age} days old (max {max_age})"),
            });
        }
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::ContextLink;
    use crate::schema::{Policy, PolicyKind, Scope, Severity};
    use std::collections::BTreeMap;

    fn policy(fields: &[(&str, toml::Value)]) -> Policy {
        let mut m = BTreeMap::new();
        for (k, v) in fields {
            m.insert((*k).into(), v.clone());
        }
        Policy {
            name: "link".into(),
            kind: PolicyKind::LinkFreshness,
            severity: Severity::Warn,
            scope: Scope::default(),
            triggers: vec![],
            sealed: false,
            message: None,
            fields: m,
        }
    }

    #[test]
    fn stale_link_flagged() {
        let p = policy(&[("max_age_days".into(), toml::Value::Integer(10))]);
        let mut ctx = PolicyContext::new("/tmp".into());
        ctx.links
            .insert("shared".into(), ContextLink { name: "shared".into(), age_days: Some(42) });
        let f = evaluate(&p, &ctx);
        assert_eq!(f.len(), 1);
        assert!(f[0].message.contains("42 days"));
    }

    #[test]
    fn fresh_link_passes() {
        let p = policy(&[("max_age_days".into(), toml::Value::Integer(10))]);
        let mut ctx = PolicyContext::new("/tmp".into());
        ctx.links.insert("shared".into(), ContextLink { name: "shared".into(), age_days: Some(3) });
        assert!(evaluate(&p, &ctx).is_empty());
    }
}
