//! `lockfile_integrity` — refuses to pass if `versionx.lock` doesn't
//! match the native lockfiles. The actual check is performed upstream
//! by `versionx verify`; this rule just surfaces its result as a
//! finding with configurable severity.

use crate::context::PolicyContext;
use crate::finding::Finding;
use crate::schema::{Policy, PolicyKind};

pub fn evaluate(policy: &Policy, ctx: &PolicyContext) -> Vec<Finding> {
    match ctx.lockfile_integrity_ok {
        Some(true) | None => Vec::new(),
        Some(false) => vec![Finding {
            policy: policy.name.clone(),
            kind: PolicyKind::LockfileIntegrity,
            severity: policy.severity,
            component: None,
            message: "versionx.lock drifted from native lockfiles — run `versionx verify`".into(),
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{Policy, PolicyKind, Scope, Severity};
    use std::collections::BTreeMap;

    fn policy() -> Policy {
        Policy {
            name: "lock".into(),
            kind: PolicyKind::LockfileIntegrity,
            severity: Severity::Deny,
            scope: Scope::default(),
            triggers: vec![],
            sealed: false,
            message: None,
            fields: BTreeMap::new(),
        }
    }

    #[test]
    fn ok_means_silent() {
        let p = policy();
        let mut ctx = PolicyContext::new("/tmp".into());
        ctx.lockfile_integrity_ok = Some(true);
        assert!(evaluate(&p, &ctx).is_empty());
    }

    #[test]
    fn drift_emits_finding() {
        let p = policy();
        let mut ctx = PolicyContext::new("/tmp".into());
        ctx.lockfile_integrity_ok = Some(false);
        assert_eq!(evaluate(&p, &ctx).len(), 1);
    }
}
