//! Release plan persistence — `.versionx/plans/<blake3>.toml`.
//!
//! A plan is the immutable artifact emitted by `versionx release propose`
//! and consumed by `versionx release apply`. Its structure:
//!
//! ```toml
//! plan_id = "blake3:abc…"
//! workspace_root = "/Users/kody/myrepo"
//! pre_requisite_hash = "blake3:def…"      # hash of the lockfile when the plan was made
//! created_at = 2026-04-18T12:00:00Z
//! expires_at = 2026-04-19T12:00:00Z
//! versionx_version = "0.1.0-dev"
//! approved = false
//! strategy = "conventional"                # "pr-title" | "conventional" | "changesets" | "manual"
//!
//! [[bumps]]
//! id = "core"
//! kind = "rust"
//! from = "1.2.3"
//! to = "1.2.4"
//! level = "patch"
//! reason = { kind = "direct_change" }
//! changelog = "fix: off-by-one in parser"
//! ```
//!
//! ### Invariants
//!
//! - `plan_id` is the BLAKE3 of the *canonical* plan body (fields
//!   sorted, without `plan_id` itself, without `approved`). Two
//!   identical proposals produce the same id — useful for idempotence.
//! - `pre_requisite_hash` is compared at apply time. If the lockfile
//!   moved, the plan is refused.
//! - `expires_at` is likewise checked.

use std::fs;
use std::path::PathBuf;

use camino::{Utf8Path, Utf8PathBuf};
use chrono::{DateTime, Duration, Utc};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::conventional::BumpLevel;

/// A persisted release plan. Fully round-trips through TOML.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReleasePlan {
    pub plan_id: String,
    pub workspace_root: Utf8PathBuf,
    pub pre_requisite_hash: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub versionx_version: String,
    #[serde(default)]
    pub approved: bool,
    pub strategy: String,
    #[serde(default)]
    pub bumps: Vec<PlannedBump>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlannedBump {
    pub id: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    pub to: String,
    pub level: BumpLevel,
    pub reason: BumpReason,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub changelog: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BumpReason {
    DirectChange,
    Cascaded { from: Vec<String> },
    GroupLockstep { group: String, via: String },
}

#[derive(Debug, thiserror::Error)]
pub enum PlanError {
    #[error("io error at {path}: {source}")]
    Io {
        path: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("parse error at {path}: {message}")]
    Parse { path: Utf8PathBuf, message: String },
    #[error("plan {plan_id} has expired (expired_at={expires_at})")]
    Expired { plan_id: String, expires_at: DateTime<Utc> },
    #[error(
        "plan {plan_id} is stale: lockfile hash changed (plan expected {expected}, got {actual})"
    )]
    StaleHash { plan_id: String, expected: String, actual: String },
    #[error("plan {plan_id} is not approved")]
    NotApproved { plan_id: String },
}

pub type PlanResult<T> = Result<T, PlanError>;

impl ReleasePlan {
    /// Build a new plan from proposed bumps. Computes `plan_id` by
    /// hashing the canonical body.
    ///
    /// `ttl` bounds how long the plan stays valid; the default matches
    /// the spec (24h).
    pub fn new(
        workspace_root: Utf8PathBuf,
        pre_requisite_hash: String,
        strategy: impl Into<String>,
        bumps: Vec<PlannedBump>,
        ttl: Duration,
    ) -> Self {
        let now = Utc::now();
        let mut plan = Self {
            plan_id: String::new(),
            workspace_root,
            pre_requisite_hash,
            created_at: now,
            expires_at: now + ttl,
            versionx_version: env!("CARGO_PKG_VERSION").into(),
            approved: false,
            strategy: strategy.into(),
            bumps,
        };
        plan.plan_id = plan.compute_id();
        plan
    }

    /// Canonical BLAKE3 of the plan body. Deterministic so two plans with
    /// the same content (even produced on different machines) hash to
    /// the same id.
    ///
    /// We omit `plan_id`, `approved`, `created_at`, and `expires_at`
    /// from the hash — those are metadata that shouldn't change the
    /// content identity of the plan.
    fn compute_id(&self) -> String {
        #[derive(Serialize)]
        struct Canonical<'a> {
            workspace_root: &'a Utf8PathBuf,
            pre_requisite_hash: &'a str,
            strategy: &'a str,
            bumps: Vec<CanonicalBump<'a>>,
        }
        #[derive(Serialize)]
        struct CanonicalBump<'a> {
            id: &'a str,
            kind: &'a str,
            from: Option<&'a str>,
            to: &'a str,
            level: BumpLevel,
            reason: &'a BumpReason,
        }
        let mut bumps: Vec<CanonicalBump> = self
            .bumps
            .iter()
            .map(|b| CanonicalBump {
                id: &b.id,
                kind: &b.kind,
                from: b.from.as_deref(),
                to: &b.to,
                level: b.level,
                reason: &b.reason,
            })
            .collect();
        bumps.sort_by(|a, b| a.id.cmp(b.id));
        let canonical = Canonical {
            workspace_root: &self.workspace_root,
            pre_requisite_hash: &self.pre_requisite_hash,
            strategy: &self.strategy,
            bumps,
        };
        let bytes = serde_json::to_vec(&canonical).expect("canonical serialize");
        format!("blake3:{}", blake3::hash(&bytes).to_hex())
    }

    /// True if the plan's `expires_at` is in the past.
    pub fn is_expired(&self, now: DateTime<Utc>) -> bool {
        self.expires_at <= now
    }

    /// Mutate to mark approved.
    pub fn approve(&mut self) {
        self.approved = true;
    }

    /// Serialize to a TOML string.
    pub fn to_toml(&self) -> PlanResult<String> {
        toml::to_string_pretty(self).map_err(|e| PlanError::Parse {
            path: Utf8PathBuf::from("<memory>"),
            message: e.to_string(),
        })
    }

    /// Parse from a TOML string at a known path (for error context).
    pub fn from_toml(source: &str, path: &Utf8Path) -> PlanResult<Self> {
        toml::from_str(source)
            .map_err(|e| PlanError::Parse { path: path.to_path_buf(), message: e.to_string() })
    }

    /// Atomic write to `<plans_dir>/<plan_id>.toml`. Idempotent — if the
    /// file already exists with the same contents we don't overwrite.
    pub fn save(&self, plans_dir: &Utf8Path) -> PlanResult<Utf8PathBuf> {
        fs::create_dir_all(plans_dir.as_std_path())
            .map_err(|source| PlanError::Io { path: plans_dir.to_path_buf(), source })?;
        // File name uses the raw blake3 hex (strip `"blake3:"` prefix).
        let stem = self.plan_id.strip_prefix("blake3:").unwrap_or(&self.plan_id);
        let path = plans_dir.join(format!("{stem}.toml"));
        let body = self.to_toml()?;
        let tmp = path.with_extension("toml.tmp");
        fs::write(tmp.as_std_path(), &body)
            .map_err(|source| PlanError::Io { path: tmp.clone(), source })?;
        fs::rename(tmp.as_std_path(), path.as_std_path())
            .map_err(|source| PlanError::Io { path: path.clone(), source })?;
        Ok(path)
    }

    /// Load a specific plan by id (`"blake3:…"` or the bare hex stem).
    pub fn load_by_id(plans_dir: &Utf8Path, plan_id: &str) -> PlanResult<Self> {
        let stem = plan_id.strip_prefix("blake3:").unwrap_or(plan_id);
        let path = plans_dir.join(format!("{stem}.toml"));
        Self::load(&path)
    }

    /// Load from a concrete path.
    pub fn load(path: &Utf8Path) -> PlanResult<Self> {
        let raw = fs::read_to_string(path.as_std_path())
            .map_err(|source| PlanError::Io { path: path.to_path_buf(), source })?;
        Self::from_toml(&raw, path)
    }
}

/// List every plan file in `plans_dir`, oldest-first by `created_at`.
pub fn list_plans(plans_dir: &Utf8Path) -> PlanResult<Vec<ReleasePlan>> {
    if !plans_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in fs::read_dir(plans_dir.as_std_path())
        .map_err(|source| PlanError::Io { path: plans_dir.to_path_buf(), source })?
    {
        let Ok(entry) = entry else { continue };
        let p: PathBuf = entry.path();
        if p.extension().and_then(|s| s.to_str()) != Some("toml") {
            continue;
        }
        let Ok(utf8) = Utf8PathBuf::from_path_buf(p.clone()) else { continue };
        match ReleasePlan::load(&utf8) {
            Ok(plan) => out.push(plan),
            Err(e) => {
                tracing::warn!("skipping malformed plan {p:?}: {e}");
            }
        }
    }
    out.sort_by_key(|p| p.created_at);
    Ok(out)
}

/// Remove plans whose `expires_at` is before `now`. Returns the list of
/// removed plan ids.
pub fn expire_plans(plans_dir: &Utf8Path, now: DateTime<Utc>) -> PlanResult<Vec<String>> {
    let mut removed = Vec::new();
    for plan in list_plans(plans_dir)? {
        if plan.is_expired(now) {
            let stem = plan.plan_id.strip_prefix("blake3:").unwrap_or(&plan.plan_id);
            let path = plans_dir.join(format!("{stem}.toml"));
            let _ = fs::remove_file(path.as_std_path());
            removed.push(plan.plan_id);
        }
    }
    Ok(removed)
}

/// Compute a stable BLAKE3 of the current lockfile state. Used both for
/// the plan's `pre_requisite_hash` and for the apply-time validation.
pub fn lockfile_hash(workspace_root: &Utf8Path) -> String {
    let lockfile_path = workspace_root.join("versionx.lock");
    let bytes = std::fs::read(lockfile_path.as_std_path()).unwrap_or_default();
    format!("blake3:{}", blake3::hash(&bytes).to_hex())
}

/// Validate `plan` is safe to apply right now. Callers should invoke
/// this immediately before writing any bumps.
pub fn validate_for_apply(
    plan: &ReleasePlan,
    current_lockfile_hash: &str,
    now: DateTime<Utc>,
) -> PlanResult<()> {
    if !plan.approved {
        return Err(PlanError::NotApproved { plan_id: plan.plan_id.clone() });
    }
    if plan.is_expired(now) {
        return Err(PlanError::Expired {
            plan_id: plan.plan_id.clone(),
            expires_at: plan.expires_at,
        });
    }
    if plan.pre_requisite_hash != current_lockfile_hash {
        return Err(PlanError::StaleHash {
            plan_id: plan.plan_id.clone(),
            expected: plan.pre_requisite_hash.clone(),
            actual: current_lockfile_hash.to_string(),
        });
    }
    Ok(())
}

/// Convenience: the canonical on-disk location inside a workspace.
#[must_use]
pub fn plans_dir(workspace_root: &Utf8Path) -> Utf8PathBuf {
    workspace_root.join(".versionx/plans")
}

/// Coerce a loose IndexMap of bumps into a Vec in stable (alphabetical)
/// order. Helpful for callers that produce bumps via a map keyed by
/// component id.
#[must_use]
pub fn sort_bumps(map: IndexMap<String, PlannedBump>) -> Vec<PlannedBump> {
    let mut v: Vec<PlannedBump> = map.into_values().collect();
    v.sort_by(|a, b| a.id.cmp(&b.id));
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_bump() -> PlannedBump {
        PlannedBump {
            id: "core".into(),
            kind: "rust".into(),
            from: Some("1.2.3".into()),
            to: "1.2.4".into(),
            level: BumpLevel::Patch,
            reason: BumpReason::DirectChange,
            changelog: "fix: off-by-one".into(),
        }
    }

    #[test]
    fn plan_id_is_deterministic() {
        let root = Utf8PathBuf::from("/tmp/ws");
        let p1 = ReleasePlan::new(
            root.clone(),
            "blake3:lh1".into(),
            "conventional",
            vec![sample_bump()],
            Duration::hours(24),
        );
        let p2 = ReleasePlan::new(
            root,
            "blake3:lh1".into(),
            "conventional",
            vec![sample_bump()],
            Duration::hours(24),
        );
        assert_eq!(p1.plan_id, p2.plan_id);
        assert!(p1.plan_id.starts_with("blake3:"));
    }

    #[test]
    fn round_trip_toml() {
        let plan = ReleasePlan::new(
            Utf8PathBuf::from("/tmp/ws"),
            "blake3:x".into(),
            "conventional",
            vec![sample_bump()],
            Duration::hours(1),
        );
        let s = plan.to_toml().unwrap();
        let back = ReleasePlan::from_toml(&s, Utf8Path::new("<test>")).unwrap();
        assert_eq!(back.plan_id, plan.plan_id);
        assert_eq!(back.bumps[0].id, "core");
        assert_eq!(back.bumps[0].level, BumpLevel::Patch);
    }

    #[test]
    fn save_and_load() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        let plan = ReleasePlan::new(
            dir.clone(),
            "blake3:y".into(),
            "conventional",
            vec![sample_bump()],
            Duration::hours(1),
        );
        let path = plan.save(&dir).unwrap();
        let back = ReleasePlan::load(&path).unwrap();
        assert_eq!(back.plan_id, plan.plan_id);
    }

    #[test]
    fn expired_is_detected() {
        let past = Utc::now() - Duration::hours(25);
        let mut plan = ReleasePlan::new(
            Utf8PathBuf::from("/tmp"),
            "blake3:a".into(),
            "manual",
            vec![sample_bump()],
            Duration::hours(24),
        );
        plan.expires_at = past;
        assert!(plan.is_expired(Utc::now()));
    }

    #[test]
    fn apply_validation_refuses_unapproved() {
        let plan = ReleasePlan::new(
            Utf8PathBuf::from("/tmp"),
            "blake3:x".into(),
            "manual",
            vec![sample_bump()],
            Duration::hours(24),
        );
        let err = validate_for_apply(&plan, "blake3:x", Utc::now()).unwrap_err();
        assert!(matches!(err, PlanError::NotApproved { .. }));
    }

    #[test]
    fn apply_validation_refuses_stale_hash() {
        let mut plan = ReleasePlan::new(
            Utf8PathBuf::from("/tmp"),
            "blake3:x".into(),
            "manual",
            vec![sample_bump()],
            Duration::hours(24),
        );
        plan.approve();
        let err = validate_for_apply(&plan, "blake3:OTHER", Utc::now()).unwrap_err();
        assert!(matches!(err, PlanError::StaleHash { .. }));
    }

    #[test]
    fn list_and_expire() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        let mut stale = ReleasePlan::new(
            dir.clone(),
            "blake3:a".into(),
            "manual",
            vec![sample_bump()],
            Duration::hours(1),
        );
        stale.expires_at = Utc::now() - Duration::minutes(1);
        stale.save(&dir).unwrap();

        let fresh = ReleasePlan::new(
            dir.clone(),
            "blake3:b".into(),
            "manual",
            vec![sample_bump()],
            Duration::hours(24),
        );
        fresh.save(&dir).unwrap();

        let removed = expire_plans(&dir, Utc::now()).unwrap();
        assert_eq!(removed.len(), 1);
        let remaining = list_plans(&dir).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].plan_id, fresh.plan_id);
    }
}
