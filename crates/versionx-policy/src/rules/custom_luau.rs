//! `custom` — Luau-scripted policy.
//!
//! ```toml
//! [[policy]]
//! name = "big-cli-needs-docs"
//! kind = "custom"
//! script = """
//!   for id, c in pairs(vx.components) do
//!     if c.kind == 'node' and c.deps['commander'] and not c.deps['@types/commander'] then
//!       report(id .. ': commander without @types')
//!     end
//!   end
//! """
//! ```
//!
//! Each `report(msg)` call becomes one [`Finding`]. Any uncaught Luau
//! error turns into a single finding explaining the failure.

use super::field_str;
use crate::context::PolicyContext;
use crate::finding::Finding;
use crate::sandbox::LuauSandbox;
use crate::schema::{Policy, PolicyKind};

pub fn evaluate(
    policy: &Policy,
    ctx: &PolicyContext,
    sandbox: Option<&LuauSandbox>,
) -> Vec<Finding> {
    let Some(script) = field_str(policy, "script") else {
        return vec![mk(policy, "custom policy missing `script` field".into())];
    };

    // Build an ephemeral sandbox when the caller didn't supply one (the
    // engine usually reuses a single sandbox across an evaluation pass).
    let owned: Option<LuauSandbox> = if sandbox.is_none() {
        match LuauSandbox::new() {
            Ok(s) => Some(s),
            Err(e) => return vec![mk(policy, format!("custom policy: sandbox init failed: {e}"))],
        }
    } else {
        None
    };
    let sb = sandbox.unwrap_or_else(|| owned.as_ref().expect("just built"));

    match sb.run(script, ctx) {
        Ok(messages) => messages
            .into_iter()
            .map(|m| Finding {
                policy: policy.name.clone(),
                kind: PolicyKind::Custom,
                severity: policy.severity,
                component: None,
                message: m,
            })
            .collect(),
        Err(err) => vec![mk(policy, format!("custom policy execution failed: {err}"))],
    }
}

fn mk(policy: &Policy, message: String) -> Finding {
    Finding {
        policy: policy.name.clone(),
        kind: PolicyKind::Custom,
        severity: policy.severity,
        component: None,
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::ContextComponent;
    use crate::schema::{Policy, PolicyKind, Scope, Severity};
    use std::collections::BTreeMap;

    fn policy(script: &str) -> Policy {
        let mut m = BTreeMap::new();
        m.insert("script".into(), toml::Value::String(script.into()));
        Policy {
            name: "custom".into(),
            kind: PolicyKind::Custom,
            severity: Severity::Deny,
            scope: Scope::default(),
            triggers: vec![],
            sealed: false,
            message: None,
            fields: m,
        }
    }

    fn ctx_with_node_component() -> PolicyContext {
        let mut ctx = PolicyContext::new("/tmp".into());
        let mut deps = BTreeMap::new();
        deps.insert("commander".into(), "^12.0.0".into());
        ctx.components.insert(
            "cli".into(),
            ContextComponent {
                id: "cli".into(),
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
    fn report_becomes_finding() {
        let p = policy(
            r#"
                for id, c in pairs(vx.components) do
                    if c.deps['commander'] and not c.deps['@types/commander'] then
                        report(id .. ': missing types')
                    end
                end
            "#,
        );
        let ctx = ctx_with_node_component();
        let findings = evaluate(&p, &ctx, None);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].message.contains("missing types"));
    }

    #[test]
    fn script_error_produces_one_finding() {
        let p = policy("this is not valid lua");
        let ctx = ctx_with_node_component();
        let findings = evaluate(&p, &ctx, None);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].message.contains("execution failed"));
    }
}
