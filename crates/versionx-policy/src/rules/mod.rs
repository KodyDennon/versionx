//! Rule evaluators.
//!
//! Each [`PolicyKind`] has exactly one implementation here. Rules are
//! pure functions of `(Policy, PolicyContext) -> Vec<Finding>` — no
//! I/O, no clock access (anything time-based comes in through the
//! context so tests stay deterministic).
//!
//! The [`evaluate`] dispatcher picks the right module based on
//! `policy.kind`.

use crate::context::PolicyContext;
use crate::finding::Finding;
use crate::sandbox::LuauSandbox;
use crate::schema::{Policy, PolicyKind, Scope};

pub mod advisory_block;
pub mod commit_format;
pub mod custom_luau;
pub mod dependency_presence;
pub mod dependency_version;
pub mod link_freshness;
pub mod lockfile_integrity;
pub mod provenance_required;
pub mod release_gate;
pub mod runtime_version;

/// Dispatch a single policy to its evaluator.
///
/// The `sandbox` argument is only used by the `Custom` (Luau) kind;
/// other kinds ignore it. We pass it in rather than constructing lazily
/// so tests can inject a pre-warmed sandbox.
pub fn evaluate(
    policy: &Policy,
    ctx: &PolicyContext,
    sandbox: Option<&LuauSandbox>,
) -> Vec<Finding> {
    match policy.kind {
        PolicyKind::RuntimeVersion => runtime_version::evaluate(policy, ctx),
        PolicyKind::DependencyVersion => dependency_version::evaluate(policy, ctx),
        PolicyKind::DependencyPresence => dependency_presence::evaluate(policy, ctx),
        PolicyKind::AdvisoryBlock => advisory_block::evaluate(policy, ctx),
        PolicyKind::ReleaseGate => release_gate::evaluate(policy, ctx),
        PolicyKind::CommitFormat => commit_format::evaluate(policy, ctx),
        PolicyKind::LockfileIntegrity => lockfile_integrity::evaluate(policy, ctx),
        PolicyKind::LinkFreshness => link_freshness::evaluate(policy, ctx),
        PolicyKind::ProvenanceRequired => provenance_required::evaluate(policy, ctx),
        PolicyKind::Custom => custom_luau::evaluate(policy, ctx, sandbox),
    }
}

/// Does `scope` include this component? Centralized so every rule
/// consistently respects the same semantics.
pub(crate) fn scope_includes(scope: &Scope, component_id: &str, component_tags: &[String]) -> bool {
    if !scope.list.is_empty() {
        return scope.list.iter().any(|s| s == component_id);
    }
    if let Some(tag) = &scope.tag
        && component_tags.iter().any(|t| t == tag)
    {
        return true;
    }
    if scope.path.is_some() {
        // Path-scope is resolved at the engine level (it has workspace
        // root + component root to compare). We default-accept here so
        // rules don't need to know about filesystem layout.
        return true;
    }
    scope.all
}

/// Short helper for the common "pull a string field" pattern used by
/// almost every rule.
pub(crate) fn field_str<'a>(policy: &'a Policy, key: &str) -> Option<&'a str> {
    policy.fields.get(key).and_then(|v| v.as_str())
}

/// Pull an array of strings.
pub(crate) fn field_str_list(policy: &Policy, key: &str) -> Vec<String> {
    policy
        .fields
        .get(key)
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}

/// Pull a bool field with a default.
pub(crate) fn field_bool(policy: &Policy, key: &str, default: bool) -> bool {
    policy.fields.get(key).and_then(|v| v.as_bool()).unwrap_or(default)
}

/// Pull a signed-integer field.
pub(crate) fn field_i64(policy: &Policy, key: &str) -> Option<i64> {
    policy.fields.get(key).and_then(|v| v.as_integer())
}
