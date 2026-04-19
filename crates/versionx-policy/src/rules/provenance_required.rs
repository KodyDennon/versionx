//! `provenance_required` — emit a finding for every in-scope component
//! that has no recorded provenance (sigstore attestation bundle, SBOM,
//! etc.). The mere existence of `ctx.provenance[id]` satisfies this
//! rule; verifying the attestation is a separate step.

use super::scope_includes;
use crate::context::PolicyContext;
use crate::finding::Finding;
use crate::schema::{Policy, PolicyKind};

pub fn evaluate(policy: &Policy, ctx: &PolicyContext) -> Vec<Finding> {
    let mut findings = Vec::new();
    for comp in ctx.components.values() {
        if !scope_includes(&policy.scope, &comp.id, &comp.tags) {
            continue;
        }
        if !ctx.provenance.contains_key(&comp.id) {
            findings.push(Finding {
                policy: policy.name.clone(),
                kind: PolicyKind::ProvenanceRequired,
                severity: policy.severity,
                component: Some(comp.id.clone()),
                message: format!("component `{}` has no provenance attestation", comp.id),
            });
        }
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::ContextComponent;
    use crate::schema::{Policy, PolicyKind, Scope, Severity};
    use std::collections::BTreeMap;

    fn policy() -> Policy {
        Policy {
            name: "prov".into(),
            kind: PolicyKind::ProvenanceRequired,
            severity: Severity::Deny,
            scope: Scope::default(),
            triggers: vec![],
            sealed: false,
            message: None,
            fields: BTreeMap::new(),
        }
    }

    fn ctx(components: &[&str], provenance: &[(&str, &str)]) -> PolicyContext {
        let mut c = PolicyContext::new("/tmp".into());
        for id in components {
            c.components.insert(
                (*id).into(),
                ContextComponent {
                    id: (*id).into(),
                    kind: "rust".into(),
                    root: "/tmp".into(),
                    version: None,
                    dependencies: BTreeMap::new(),
                    tags: vec![],
                },
            );
        }
        for (k, v) in provenance {
            c.provenance.insert((*k).into(), (*v).into());
        }
        c
    }

    #[test]
    fn unattested_components_are_flagged() {
        let ctx = ctx(&["a", "b"], &[("a", "sigstore://...")]);
        let f = evaluate(&policy(), &ctx);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].component.as_deref(), Some("b"));
    }
}
