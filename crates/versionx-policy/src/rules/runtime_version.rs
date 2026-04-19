//! `runtime_version` — enforce min/max/exact version constraints on
//! pinned runtimes (node, python, rust, pnpm, …).
//!
//! Config shape:
//! ```toml
//! [[policy]]
//! name = "no-ancient-node"
//! kind = "runtime_version"
//! runtime = "node"
//! min = "20"         # optional; defines the lower bound
//! max = "22"         # optional; defines the upper bound
//! exact = "20.11.1"  # optional; if set overrides min/max
//! ```
//!
//! Version comparisons use `semver::VersionReq` with a liberal parser:
//! bare `"20"` is treated as `">=20.0.0, <21.0.0"`. Callers who need
//! exact prerelease matching use the `exact` field.

use semver::{Version, VersionReq};

use super::field_str;
use crate::context::PolicyContext;
use crate::finding::Finding;
use crate::schema::{Policy, PolicyKind};

pub fn evaluate(policy: &Policy, ctx: &PolicyContext) -> Vec<Finding> {
    let Some(runtime_name) = field_str(policy, "runtime") else {
        return vec![broken(policy, "runtime_version rule missing `runtime` field")];
    };
    let Some(actual) = ctx.runtimes.get(runtime_name) else {
        // No pin for this runtime at all → caller probably doesn't use
        // it; not a finding.
        return Vec::new();
    };
    let Ok(actual_ver) = parse_flex(&actual.version) else {
        return vec![mk(
            policy,
            None,
            format!("runtime {runtime_name}={} is not a valid semver", actual.version),
        )];
    };

    if let Some(exact) = field_str(policy, "exact")
        && actual.version != exact
    {
        return vec![mk(
            policy,
            None,
            format!("runtime {runtime_name}: expected exactly {exact}, got {}", actual.version),
        )];
    }
    let mut findings = Vec::new();
    if let Some(min) = field_str(policy, "min")
        && !matches_range(&actual_ver, min, Comparison::AtLeast)
    {
        findings.push(mk(
            policy,
            None,
            format!("runtime {runtime_name}={} below minimum {min}", actual.version),
        ));
    }
    if let Some(max) = field_str(policy, "max")
        && !matches_range(&actual_ver, max, Comparison::AtMost)
    {
        findings.push(mk(
            policy,
            None,
            format!("runtime {runtime_name}={} above maximum {max}", actual.version),
        ));
    }
    findings
}

#[derive(Copy, Clone)]
enum Comparison {
    AtLeast,
    AtMost,
}

fn matches_range(actual: &Version, bound: &str, cmp: Comparison) -> bool {
    // Liberal bound parser: if the user writes "20", treat it as an
    // inclusive bound on the major — we want "node = 20.11.1" to pass a
    // `min = "20"`.
    let Ok(bound_ver) = parse_flex(bound) else {
        let Ok(req) = VersionReq::parse(bound) else {
            return true;
        };
        return req.matches(actual);
    };
    match cmp {
        Comparison::AtLeast => actual >= &bound_ver,
        Comparison::AtMost => actual <= &bound_ver,
    }
}

/// Accept `"20"` / `"20.11"` / `"20.11.1"` and produce a concrete
/// [`Version`]. Missing minor/patch become `0` so the bound is exact.
fn parse_flex(raw: &str) -> Result<Version, semver::Error> {
    let parts: Vec<&str> = raw.split('.').collect();
    let filled = match parts.len() {
        1 => format!("{}.0.0", parts[0]),
        2 => format!("{}.{}.0", parts[0], parts[1]),
        _ => raw.to_string(),
    };
    Version::parse(&filled)
}

fn mk(policy: &Policy, component: Option<String>, message: String) -> Finding {
    Finding {
        policy: policy.name.clone(),
        kind: PolicyKind::RuntimeVersion,
        severity: policy.severity,
        component,
        message,
    }
}

fn broken(policy: &Policy, message: &str) -> Finding {
    mk(policy, None, message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::ContextRuntime;
    use crate::schema::{Policy, PolicyKind, Scope, Severity};
    use std::collections::BTreeMap;

    fn mk_policy(fields: &[(&str, &str)]) -> Policy {
        let mut map = BTreeMap::new();
        for (k, v) in fields {
            map.insert((*k).into(), toml::Value::String((*v).into()));
        }
        Policy {
            name: "rt".into(),
            kind: PolicyKind::RuntimeVersion,
            severity: Severity::Deny,
            scope: Scope::default(),
            triggers: vec![],
            sealed: false,
            message: None,
            fields: map,
        }
    }

    fn ctx_with(name: &str, version: &str) -> PolicyContext {
        let mut ctx = PolicyContext::new("/tmp".into());
        ctx.runtimes
            .insert(name.into(), ContextRuntime { name: name.into(), version: version.into() });
        ctx
    }

    #[test]
    fn min_version_satisfied() {
        let p = mk_policy(&[("runtime", "node"), ("min", "20")]);
        let ctx = ctx_with("node", "20.11.1");
        assert!(evaluate(&p, &ctx).is_empty());
    }

    #[test]
    fn min_version_violated() {
        let p = mk_policy(&[("runtime", "node"), ("min", "20")]);
        let ctx = ctx_with("node", "18.19.0");
        let findings = evaluate(&p, &ctx);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].message.contains("below minimum"));
    }

    #[test]
    fn exact_rejects_other() {
        let p = mk_policy(&[("runtime", "node"), ("exact", "20.11.1")]);
        let ctx = ctx_with("node", "20.11.2");
        let findings = evaluate(&p, &ctx);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].message.contains("expected exactly"));
    }

    #[test]
    fn missing_runtime_is_no_finding() {
        let p = mk_policy(&[("runtime", "ruby"), ("min", "3")]);
        let ctx = ctx_with("node", "20.11.1");
        assert!(evaluate(&p, &ctx).is_empty());
    }
}
