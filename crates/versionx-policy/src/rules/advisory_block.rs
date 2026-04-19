//! `advisory_block` — fail if any named advisory is present in the
//! resolver output.
//!
//! ```toml
//! [[policy]]
//! name = "no-blocked-cves"
//! kind = "advisory_block"
//! ids = ["CVE-2024-1234", "GHSA-xxxx-yyyy-zzzz"]
//! ```
//!
//! Advisories are fed into the context by whoever ran the dependency
//! resolver (for 0.5, `versionx sync` will populate them on demand).
//! An empty `ids` list means "block any advisory the resolver flagged".

use super::field_str_list;
use crate::context::PolicyContext;
use crate::finding::Finding;
use crate::schema::{Policy, PolicyKind};

pub fn evaluate(policy: &Policy, ctx: &PolicyContext) -> Vec<Finding> {
    let block_ids = field_str_list(policy, "ids");
    let block_any = block_ids.is_empty();
    let mut findings = Vec::new();
    for (id, affected) in &ctx.advisories {
        if block_any || block_ids.iter().any(|target| target == id) {
            findings.push(Finding {
                policy: policy.name.clone(),
                kind: PolicyKind::AdvisoryBlock,
                severity: policy.severity,
                component: None,
                message: format!("advisory {id} affects {affected}"),
            });
        }
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{Policy, PolicyKind, Scope, Severity};
    use std::collections::BTreeMap;

    fn policy(ids: &[&str]) -> Policy {
        let mut fields = BTreeMap::new();
        if !ids.is_empty() {
            let arr: Vec<toml::Value> =
                ids.iter().map(|s| toml::Value::String((*s).into())).collect();
            fields.insert("ids".into(), toml::Value::Array(arr));
        }
        Policy {
            name: "cves".into(),
            kind: PolicyKind::AdvisoryBlock,
            severity: Severity::Deny,
            scope: Scope::default(),
            triggers: vec![],
            sealed: false,
            message: None,
            fields,
        }
    }

    #[test]
    fn explicit_list_matches() {
        let p = policy(&["CVE-1"]);
        let mut ctx = PolicyContext::new("/tmp".into());
        ctx.advisories.insert("CVE-1".into(), "lodash@3.10.0".into());
        let f = evaluate(&p, &ctx);
        assert_eq!(f.len(), 1);
    }

    #[test]
    fn empty_list_blocks_any() {
        let p = policy(&[]);
        let mut ctx = PolicyContext::new("/tmp".into());
        ctx.advisories.insert("CVE-99".into(), "pkg@1.0.0".into());
        assert_eq!(evaluate(&p, &ctx).len(), 1);
    }
}
