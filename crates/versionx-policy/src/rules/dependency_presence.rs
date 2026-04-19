//! `dependency_presence` — require or forbid a dependency.
//!
//! ```toml
//! [[policy]]
//! name = "require-opentelemetry"
//! kind = "dependency_presence"
//! package = "@opentelemetry/api"
//! mode = "require"     # "require" | "forbid"
//! ```

use super::{field_str, scope_includes};
use crate::context::PolicyContext;
use crate::finding::Finding;
use crate::schema::{Policy, PolicyKind};

pub fn evaluate(policy: &Policy, ctx: &PolicyContext) -> Vec<Finding> {
    let Some(package) = field_str(policy, "package") else {
        return vec![mk(policy, None, "dependency_presence missing `package` field".into())];
    };
    let mode = field_str(policy, "mode").unwrap_or("require");
    let require = matches!(mode, "require");

    let mut findings = Vec::new();
    for comp in ctx.components.values() {
        if !scope_includes(&policy.scope, &comp.id, &comp.tags) {
            continue;
        }
        let present = comp.dependencies.contains_key(package);
        if require && !present {
            findings.push(mk(
                policy,
                Some(comp.id.clone()),
                format!("missing required dep `{package}`"),
            ));
        } else if !require && present {
            findings.push(mk(
                policy,
                Some(comp.id.clone()),
                format!("forbidden dep `{package}` present"),
            ));
        }
    }
    findings
}

fn mk(policy: &Policy, component: Option<String>, message: String) -> Finding {
    Finding {
        policy: policy.name.clone(),
        kind: PolicyKind::DependencyPresence,
        severity: policy.severity,
        component,
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::ContextComponent;
    use crate::schema::{Policy, PolicyKind, Scope, Severity};
    use std::collections::BTreeMap;

    fn policy(fields: &[(&str, &str)]) -> Policy {
        let mut m = BTreeMap::new();
        for (k, v) in fields {
            m.insert((*k).into(), toml::Value::String((*v).into()));
        }
        Policy {
            name: "pres".into(),
            kind: PolicyKind::DependencyPresence,
            severity: Severity::Deny,
            scope: Scope::default(),
            triggers: vec![],
            sealed: false,
            message: None,
            fields: m,
        }
    }

    fn ctx_with(id: &str, deps: &[(&str, &str)]) -> PolicyContext {
        let mut ctx = PolicyContext::new("/tmp".into());
        let mut d = BTreeMap::new();
        for (k, v) in deps {
            d.insert((*k).into(), (*v).into());
        }
        ctx.components.insert(
            id.into(),
            ContextComponent {
                id: id.into(),
                kind: "node".into(),
                root: "/tmp".into(),
                version: None,
                dependencies: d,
                tags: vec![],
            },
        );
        ctx
    }

    #[test]
    fn require_missing() {
        let p = policy(&[("package", "x"), ("mode", "require")]);
        let ctx = ctx_with("app", &[("y", "1.0")]);
        assert_eq!(evaluate(&p, &ctx).len(), 1);
    }

    #[test]
    fn forbid_present() {
        let p = policy(&[("package", "x"), ("mode", "forbid")]);
        let ctx = ctx_with("app", &[("x", "1.0")]);
        assert_eq!(evaluate(&p, &ctx).len(), 1);
    }

    #[test]
    fn require_present_passes() {
        let p = policy(&[("package", "x"), ("mode", "require")]);
        let ctx = ctx_with("app", &[("x", "1.0")]);
        assert!(evaluate(&p, &ctx).is_empty());
    }
}
