//! `commit_format` — commits must match a pattern or Conventional Commits.
//!
//! ```toml
//! [[policy]]
//! name = "conventional-commits"
//! kind = "commit_format"
//! style = "conventional"    # "conventional" | "regex"
//! pattern = "^[A-Z]+-\\d+"   # only meaningful when style = "regex"
//! ```

use super::field_str;
use crate::context::{ContextCommit, PolicyContext};
use crate::finding::Finding;
use crate::schema::{Policy, PolicyKind};

pub fn evaluate(policy: &Policy, ctx: &PolicyContext) -> Vec<Finding> {
    let style = field_str(policy, "style").unwrap_or("conventional");

    let matcher: Box<dyn Fn(&ContextCommit) -> bool> = match style {
        "regex" => {
            let Some(pattern) = field_str(policy, "pattern") else {
                return vec![bad(policy, "commit_format: style=regex but no `pattern`")];
            };
            let Ok(re) = regex::Regex::new(pattern) else {
                return vec![bad(policy, &format!("commit_format: invalid regex `{pattern}`"))];
            };
            Box::new(move |c: &ContextCommit| re.is_match(c.message.lines().next().unwrap_or("")))
        }
        "conventional" => Box::new(is_conventional),
        other => {
            return vec![bad(policy, &format!("commit_format: unknown style `{other}`"))];
        }
    };

    let mut findings = Vec::new();
    for c in &ctx.commits {
        if !matcher(c) {
            let first_line = c.message.lines().next().unwrap_or("").trim();
            findings.push(Finding {
                policy: policy.name.clone(),
                kind: PolicyKind::CommitFormat,
                severity: policy.severity,
                component: None,
                message: format!(
                    "commit {} does not match required format: {first_line}",
                    short_sha(&c.sha),
                ),
            });
        }
    }
    findings
}

fn is_conventional(c: &ContextCommit) -> bool {
    // Same regex as versionx-release::conventional::parse_header.
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| {
        regex::Regex::new(r"^(?P<type>[a-zA-Z]+)(?:\([^)]+\))?!?:\s*.+$").expect("static")
    });
    re.is_match(c.message.lines().next().unwrap_or("").trim())
}

fn short_sha(s: &str) -> &str {
    s.get(..s.len().min(8)).unwrap_or(s)
}

fn bad(policy: &Policy, message: &str) -> Finding {
    Finding {
        policy: policy.name.clone(),
        kind: PolicyKind::CommitFormat,
        severity: policy.severity,
        component: None,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{Policy, PolicyKind, Scope, Severity};
    use std::collections::BTreeMap;

    fn policy(fields: &[(&str, &str)]) -> Policy {
        let mut m = BTreeMap::new();
        for (k, v) in fields {
            m.insert((*k).into(), toml::Value::String((*v).into()));
        }
        Policy {
            name: "cf".into(),
            kind: PolicyKind::CommitFormat,
            severity: Severity::Deny,
            scope: Scope::default(),
            triggers: vec![],
            sealed: false,
            message: None,
            fields: m,
        }
    }

    fn ctx_with(messages: &[&str]) -> PolicyContext {
        let mut ctx = PolicyContext::new("/tmp".into());
        for (i, m) in messages.iter().enumerate() {
            ctx.commits.push(ContextCommit { sha: format!("{:040}", i), message: (*m).into() });
        }
        ctx
    }

    #[test]
    fn conventional_passes_mixed_valid() {
        let p = policy(&[]);
        let ctx = ctx_with(&["feat: x", "fix(parser): y", "chore: z"]);
        assert!(evaluate(&p, &ctx).is_empty());
    }

    #[test]
    fn conventional_flags_bad_format() {
        let p = policy(&[]);
        let ctx = ctx_with(&["feat: x", "just some text"]);
        let f = evaluate(&p, &ctx);
        assert_eq!(f.len(), 1);
    }

    #[test]
    fn regex_style_requires_pattern() {
        let p = policy(&[("style", "regex")]);
        let ctx = ctx_with(&["anything"]);
        let f = evaluate(&p, &ctx);
        assert!(f[0].message.contains("no `pattern`"));
    }
}
