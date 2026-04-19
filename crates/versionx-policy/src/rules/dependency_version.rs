//! `dependency_version` — constrain a named dependency across components.
//!
//! ```toml
//! [[policy]]
//! name = "no-left-pad-v0"
//! kind = "dependency_version"
//! package = "left-pad"
//! min = "1.0.0"
//! ```
//!
//! Matches across all components the scope covers. Each component that
//! declares `package` with a version violating the constraint produces
//! one finding.

use super::{field_str, scope_includes};
use crate::context::PolicyContext;
use crate::finding::Finding;
use crate::schema::{Policy, PolicyKind};

pub fn evaluate(policy: &Policy, ctx: &PolicyContext) -> Vec<Finding> {
    let Some(package) = field_str(policy, "package") else {
        return vec![mk(policy, None, "dependency_version missing `package` field".into())];
    };
    let mut findings = Vec::new();
    for comp in ctx.components.values() {
        if !scope_includes(&policy.scope, &comp.id, &comp.tags) {
            continue;
        }
        let Some(spec) = comp.dependencies.get(package) else {
            continue;
        };
        let evaluated = evaluate_constraint(policy, spec);
        if let Some(msg) = evaluated {
            findings.push(mk(policy, Some(comp.id.clone()), msg));
        }
    }
    findings
}

fn evaluate_constraint(policy: &Policy, spec: &str) -> Option<String> {
    // Strip common prefixes like `^`, `~`, `>=`. Version "locked" might
    // be `"1.0.0"`, `"^1.0.0"`, `"workspace:^"`, `"file:…"`. For the
    // workspace:/file: case we can't meaningfully bound it; skip.
    if spec.starts_with("workspace:") || spec.starts_with("file:") || spec.starts_with("link:") {
        return None;
    }
    let cleaned = spec.trim_start_matches(['^', '~', '>', '<', '=', ' ']);
    let Ok(ver) = semver::Version::parse(cleaned) else {
        return None; // Unparseable specs don't trigger findings.
    };
    if let Some(min) = field_str(policy, "min")
        && let Ok(min_ver) = semver::Version::parse(min)
        && ver < min_ver
    {
        return Some(format!("{} is below minimum {}", cleaned, min));
    }
    if let Some(max) = field_str(policy, "max")
        && let Ok(max_ver) = semver::Version::parse(max)
        && ver > max_ver
    {
        return Some(format!("{} is above maximum {}", cleaned, max));
    }
    if let Some(exact) = field_str(policy, "exact")
        && cleaned != exact
    {
        return Some(format!("{} does not equal required {}", cleaned, exact));
    }
    None
}

fn mk(policy: &Policy, component: Option<String>, message: String) -> Finding {
    Finding {
        policy: policy.name.clone(),
        kind: PolicyKind::DependencyVersion,
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
            name: "dep".into(),
            kind: PolicyKind::DependencyVersion,
            severity: Severity::Deny,
            scope: Scope::default(),
            triggers: vec![],
            sealed: false,
            message: None,
            fields: m,
        }
    }

    fn ctx_with_dep(id: &str, pkg: &str, spec: &str) -> PolicyContext {
        let mut ctx = PolicyContext::new("/tmp".into());
        let mut deps = BTreeMap::new();
        deps.insert(pkg.into(), spec.into());
        ctx.components.insert(
            id.into(),
            ContextComponent {
                id: id.into(),
                kind: "node".into(),
                root: "/tmp".into(),
                version: None,
                dependencies: deps,
                tags: vec![],
            },
        );
        ctx
    }

    #[test]
    fn min_finds_old_dep() {
        let p = policy(&[("package", "lodash"), ("min", "4.0.0")]);
        let ctx = ctx_with_dep("app", "lodash", "^3.10.0");
        let findings = evaluate(&p, &ctx);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].component.as_deref(), Some("app"));
    }

    #[test]
    fn workspace_spec_is_skipped() {
        let p = policy(&[("package", "ui"), ("min", "2.0.0")]);
        let ctx = ctx_with_dep("app", "ui", "workspace:*");
        assert!(evaluate(&p, &ctx).is_empty());
    }
}
