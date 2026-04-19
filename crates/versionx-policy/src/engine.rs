//! Policy engine — load → evaluate → report.
//!
//! The engine is a coordinator, not a rule implementation. It:
//!   1. Loads every policy document from a list of paths (main
//!      `versionx.toml`, `.versionx/policies/*.toml`, inherited files).
//!   2. Concatenates their policies + waivers, enforcing uniqueness +
//!      seal invariants.
//!   3. Runs each policy through [`crate::rules::evaluate`].
//!   4. Resolves waivers via [`crate::waiver::match_waiver`].
//!   5. Returns a [`PolicyReport`] with per-finding waiver hits.
//!
//! The engine is stateless apart from an optional shared
//! [`LuauSandbox`] instance, which is pre-warmed once per run to
//! amortize VM startup cost.

use std::fs;
use std::path::PathBuf;

use camino::{Utf8Path, Utf8PathBuf};
use chrono::Utc;

use crate::context::PolicyContext;
use crate::finding::{PolicyReport, ReportedFinding};
use crate::lockfile::{self, LockfileError, PolicyLockfile};
use crate::rules;
use crate::sandbox::LuauSandbox;
use crate::schema::{self, Policy, PolicyDocument, Trigger, Waiver};
use crate::waiver;

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("io error at {path}: {source}")]
    Io {
        path: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("parse error at {path}: {source}")]
    Parse {
        path: Utf8PathBuf,
        #[source]
        source: schema::PolicyParseError,
    },
    #[error("duplicate policy `{name}` from `{path}`")]
    DuplicatePolicy { name: String, path: Utf8PathBuf },
    #[error("sandbox init failed: {0}")]
    Sandbox(#[from] crate::sandbox::SandboxError),
    #[error("policy lockfile error: {0}")]
    Lockfile(#[from] LockfileError),
    #[error(
        "policy lockfile mismatch at `{path}`: recorded blake3 `{recorded}`, current `{current}`"
    )]
    HashMismatch { path: Utf8PathBuf, recorded: String, current: String },
    #[error("sealed policy `{name}` is missing from the loaded set (recorded by `{source_path}`)")]
    SealedMissing { name: String, source_path: String },
    #[error("sealed policy `{name}` is no longer sealed in the loaded set")]
    SealedUnsealed { name: String },
}

pub type EngineResult<T> = Result<T, EngineError>;

/// One loaded policy document paired with its source path.
#[derive(Clone, Debug)]
pub struct LoadedDocument {
    pub path: Utf8PathBuf,
    pub document: PolicyDocument,
}

/// Flattened set of policies + waivers gathered from every loaded
/// source.
#[derive(Clone, Debug, Default)]
pub struct PolicySet {
    pub policies: Vec<Policy>,
    pub waivers: Vec<Waiver>,
}

impl PolicySet {
    /// Build a set from a list of [`LoadedDocument`]s. Enforces policy
    /// name uniqueness across the merged set.
    pub fn from_documents(docs: &[LoadedDocument]) -> EngineResult<Self> {
        let mut out = Self::default();
        let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for d in docs {
            for p in &d.document.policies {
                if !seen.insert(p.name.clone()) {
                    return Err(EngineError::DuplicatePolicy {
                        name: p.name.clone(),
                        path: d.path.clone(),
                    });
                }
                out.policies.push(p.clone());
            }
            for w in &d.document.waivers {
                out.waivers.push(w.clone());
            }
        }
        Ok(out)
    }
}

/// Load every `*.toml` under `dir` (non-recursive) + an optional
/// explicit list of extra files. Skips unreadable / empty files.
pub fn load_dir(dir: &Utf8Path, extras: &[Utf8PathBuf]) -> EngineResult<Vec<LoadedDocument>> {
    let mut out = Vec::new();
    if dir.is_dir() {
        for entry in fs::read_dir(dir.as_std_path())
            .map_err(|source| EngineError::Io { path: dir.to_path_buf(), source })?
        {
            let Ok(entry) = entry else { continue };
            let p: PathBuf = entry.path();
            if p.extension().and_then(|s| s.to_str()) != Some("toml") {
                continue;
            }
            let Ok(utf8) = Utf8PathBuf::from_path_buf(p.clone()) else { continue };
            out.push(load_one(&utf8)?);
        }
    }
    for extra in extras {
        if extra.is_file() {
            out.push(load_one(extra)?);
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

fn load_one(path: &Utf8Path) -> EngineResult<LoadedDocument> {
    let raw = fs::read_to_string(path.as_std_path())
        .map_err(|source| EngineError::Io { path: path.to_path_buf(), source })?;
    let doc = schema::from_toml(&raw)
        .map_err(|source| EngineError::Parse { path: path.to_path_buf(), source })?;
    Ok(LoadedDocument { path: path.to_path_buf(), document: doc })
}

/// Run every policy in `set` against `ctx`. Only policies whose
/// triggers include `ctx.trigger` (or that declare no triggers) are
/// evaluated.
pub fn evaluate(set: &PolicySet, ctx: &PolicyContext) -> EngineResult<PolicyReport> {
    let sandbox = LuauSandbox::new()?;
    evaluate_with_sandbox(set, ctx, &sandbox)
}

/// Same as [`evaluate`] but re-uses a caller-owned sandbox — useful
/// for batched runs.
pub fn evaluate_with_sandbox(
    set: &PolicySet,
    ctx: &PolicyContext,
    sandbox: &LuauSandbox,
) -> EngineResult<PolicyReport> {
    let now = Utc::now();
    let mut report = PolicyReport::default();
    report.evaluated_at = Some(now);

    for policy in &set.policies {
        if !trigger_matches(policy, ctx.trigger) {
            continue;
        }
        let raw_findings = rules::evaluate(policy, ctx, Some(sandbox));
        for f in raw_findings {
            let hit = waiver::match_waiver(&f, &set.waivers, now);
            report.findings.push(ReportedFinding { finding: f, waiver: hit });
        }
    }
    Ok(report)
}

fn trigger_matches(policy: &Policy, ctx_trigger: Option<Trigger>) -> bool {
    if policy.triggers.is_empty() {
        return true;
    }
    match ctx_trigger {
        Some(t) => policy.triggers.contains(&t),
        None => true, // no-context passes: caller is doing an ad-hoc check
    }
}

/// Default plans-dir for a workspace, mirroring the release crate's
/// `.versionx/plans` convention.
#[must_use]
pub fn default_policies_dir(workspace_root: &Utf8Path) -> Utf8PathBuf {
    workspace_root.join(".versionx/policies")
}

/// Conventional location of the policy lockfile (sibling to
/// `versionx.toml`).
#[must_use]
pub fn default_lockfile_path(workspace_root: &Utf8Path) -> Utf8PathBuf {
    workspace_root.join("versionx.policy.lock")
}

/// Verify a freshly-loaded set of policy documents against a lockfile.
///
/// For each [`crate::lockfile::LockedSource`] we:
///   1. Hash the file at `LockedSource.path` (relative to
///      `workspace_root`) and compare against the recorded blake3.
///      A mismatch is a [`EngineError::HashMismatch`].
///   2. Verify that every name in `LockedSource.sealed` is still
///      present in `set` and still has `sealed = true`. A missing or
///      unsealed name is a hard error.
///
/// Sources listed in the lockfile but absent on disk are silently
/// skipped — those represent vendored upstream files that the user
/// pruned. Hash drift on a present file is what we actually want to
/// catch.
pub fn verify_lockfile(
    lockfile: &PolicyLockfile,
    set: &PolicySet,
    workspace_root: &Utf8Path,
) -> EngineResult<()> {
    let policy_index: std::collections::BTreeMap<&str, &Policy> =
        set.policies.iter().map(|p| (p.name.as_str(), p)).collect();

    for source in &lockfile.sources {
        let path = workspace_root.join(&source.path);
        if path.is_file() {
            let current = lockfile::hash_source(&path)
                .map_err(|e| EngineError::Io { path: path.clone(), source: e })?;
            if current != source.blake3 {
                return Err(EngineError::HashMismatch {
                    path,
                    recorded: source.blake3.clone(),
                    current,
                });
            }
        }
        for sealed_name in &source.sealed {
            match policy_index.get(sealed_name.as_str()) {
                None => {
                    return Err(EngineError::SealedMissing {
                        name: sealed_name.clone(),
                        source_path: source.path.clone(),
                    });
                }
                Some(p) if !p.sealed => {
                    return Err(EngineError::SealedUnsealed { name: sealed_name.clone() });
                }
                Some(_) => {}
            }
        }
    }
    Ok(())
}

/// Convenience: load + flatten + verify-against-lockfile in one shot.
///
/// If `versionx.policy.lock` exists at `workspace_root`, its seals
/// are enforced against the loaded set. Absence is treated as
/// "lockfile not in use" (warn-on-CI is the caller's responsibility).
pub fn load_and_verify(
    workspace_root: &Utf8Path,
    extras: &[Utf8PathBuf],
) -> EngineResult<PolicySet> {
    let dir = default_policies_dir(workspace_root);
    let docs = load_dir(&dir, extras)?;
    let set = PolicySet::from_documents(&docs)?;
    let lock_path = default_lockfile_path(workspace_root);
    if lock_path.is_file() {
        let lockfile = PolicyLockfile::load(&lock_path)?;
        verify_lockfile(&lockfile, &set, workspace_root)?;
    }
    Ok(set)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::ContextRuntime;
    use crate::schema::{PolicyKind, Severity};
    use std::collections::BTreeMap;

    #[test]
    fn duplicate_policies_rejected() {
        let doc_a = LoadedDocument {
            path: "a.toml".into(),
            document: PolicyDocument {
                policies: vec![make_policy("dup", PolicyKind::ReleaseGate)],
                ..Default::default()
            },
        };
        let doc_b = LoadedDocument {
            path: "b.toml".into(),
            document: PolicyDocument {
                policies: vec![make_policy("dup", PolicyKind::CommitFormat)],
                ..Default::default()
            },
        };
        let err = PolicySet::from_documents(&[doc_a, doc_b]).unwrap_err();
        assert!(matches!(err, EngineError::DuplicatePolicy { .. }));
    }

    #[test]
    fn trigger_gating_works() {
        let mut p = make_policy("p", PolicyKind::ReleaseGate);
        p.triggers = vec![Trigger::ReleaseApply];
        let set = PolicySet { policies: vec![p], waivers: vec![] };

        let mut apply_ctx = PolicyContext::new("/tmp".into());
        apply_ctx.trigger = Some(Trigger::ReleaseApply);
        let r = evaluate(&set, &apply_ctx).unwrap();
        let _ = r; // release_gate produces no findings without fields; we just want it to have been considered.

        let mut sync_ctx = PolicyContext::new("/tmp".into());
        sync_ctx.trigger = Some(Trigger::Sync);
        let r2 = evaluate(&set, &sync_ctx).unwrap();
        // policy was skipped entirely, so the report is empty.
        assert!(r2.findings.is_empty());
    }

    #[test]
    fn waiver_matches_finding() {
        let mut p = make_policy("rt", PolicyKind::RuntimeVersion);
        p.fields.insert("runtime".into(), toml::Value::String("node".into()));
        p.fields.insert("min".into(), toml::Value::String("20".into()));
        let w = Waiver {
            policy: "rt".into(),
            reason: "legacy".into(),
            expires_at: Utc::now() + chrono::Duration::days(30),
            owner: None,
            scope: None,
        };
        let set = PolicySet { policies: vec![p], waivers: vec![w] };

        let mut ctx = PolicyContext::new("/tmp".into());
        ctx.runtimes.insert(
            "node".into(),
            ContextRuntime { name: "node".into(), version: "18.0.0".into() },
        );
        let r = evaluate(&set, &ctx).unwrap();
        assert_eq!(r.findings.len(), 1);
        assert!(r.findings[0].waiver.is_some(), "waiver should match");
        assert!(!r.has_blocking(), "waived finding shouldn't block");
    }

    fn make_policy(name: &str, kind: PolicyKind) -> Policy {
        Policy {
            name: name.into(),
            kind,
            severity: Severity::Deny,
            scope: Default::default(),
            triggers: vec![],
            sealed: false,
            message: None,
            fields: BTreeMap::new(),
        }
    }

    #[test]
    fn lockfile_verify_passes_on_intact_set() {
        use crate::lockfile::{LockedSource, PolicyLockfile};
        let tmp = tempfile::tempdir().unwrap();
        let root = camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        let policies_dir = default_policies_dir(&root);
        std::fs::create_dir_all(policies_dir.as_std_path()).unwrap();
        let pol_path = policies_dir.join("main.toml");
        std::fs::write(
            pol_path.as_std_path(),
            "[[policy]]\nname = \"keep\"\nkind = \"release_gate\"\nseverity = \"deny\"\nsealed = true\n",
        )
        .unwrap();
        let docs = load_dir(&policies_dir, &[]).unwrap();
        let set = PolicySet::from_documents(&docs).unwrap();

        let mut lf = PolicyLockfile::new();
        lf.sources.push(LockedSource {
            path: ".versionx/policies/main.toml".to_string(),
            blake3: crate::lockfile::hash_source(&pol_path).unwrap(),
            sealed: vec!["keep".into()],
        });
        verify_lockfile(&lf, &set, &root).expect("verify passes on intact set");
    }

    #[test]
    fn lockfile_verify_catches_hash_drift() {
        use crate::lockfile::{LockedSource, PolicyLockfile};
        let tmp = tempfile::tempdir().unwrap();
        let root = camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        let policies_dir = default_policies_dir(&root);
        std::fs::create_dir_all(policies_dir.as_std_path()).unwrap();
        let pol_path = policies_dir.join("main.toml");
        std::fs::write(
            pol_path.as_std_path(),
            "[[policy]]\nname = \"keep\"\nkind = \"release_gate\"\nseverity = \"deny\"\nsealed = true\n",
        )
        .unwrap();
        let docs = load_dir(&policies_dir, &[]).unwrap();
        let set = PolicySet::from_documents(&docs).unwrap();

        let mut lf = PolicyLockfile::new();
        lf.sources.push(LockedSource {
            path: ".versionx/policies/main.toml".to_string(),
            blake3: "blake3:not-the-real-hash".into(),
            sealed: vec!["keep".into()],
        });
        let err = verify_lockfile(&lf, &set, &root).unwrap_err();
        assert!(matches!(err, EngineError::HashMismatch { .. }));
    }

    #[test]
    fn lockfile_verify_catches_unsealed_policy() {
        use crate::lockfile::{LockedSource, PolicyLockfile};
        let tmp = tempfile::tempdir().unwrap();
        let root = camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        let policies_dir = default_policies_dir(&root);
        std::fs::create_dir_all(policies_dir.as_std_path()).unwrap();
        let pol_path = policies_dir.join("main.toml");
        // Note: `sealed = true` is deliberately omitted here.
        std::fs::write(
            pol_path.as_std_path(),
            "[[policy]]\nname = \"keep\"\nkind = \"release_gate\"\nseverity = \"deny\"\n",
        )
        .unwrap();
        let docs = load_dir(&policies_dir, &[]).unwrap();
        let set = PolicySet::from_documents(&docs).unwrap();

        let mut lf = PolicyLockfile::new();
        lf.sources.push(LockedSource {
            path: ".versionx/policies/main.toml".to_string(),
            blake3: crate::lockfile::hash_source(&pol_path).unwrap(),
            sealed: vec!["keep".into()],
        });
        let err = verify_lockfile(&lf, &set, &root).unwrap_err();
        assert!(matches!(err, EngineError::SealedUnsealed { .. }));
    }
}
