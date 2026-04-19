//! Policy findings — the output of an evaluation run.
//!
//! Findings are pure data. The engine wraps them in a [`PolicyReport`]
//! that tracks aggregate state (any deny? how many warns?) and the
//! waiver match for each finding.

use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::schema::{PolicyKind, Severity, Waiver};

/// One concern raised by one policy.
#[derive(Clone, Debug, Serialize)]
pub struct Finding {
    /// The policy `name` that produced this.
    pub policy: String,
    pub kind: PolicyKind,
    pub severity: Severity,
    /// Component id when the finding targets a specific one; None for
    /// workspace-wide findings (e.g. lockfile_integrity).
    pub component: Option<String>,
    /// Human-facing explanation of what went wrong.
    pub message: String,
}

/// Rolled-up evaluation output.
#[derive(Clone, Debug, Serialize, Default)]
pub struct PolicyReport {
    pub findings: Vec<ReportedFinding>,
    pub evaluated_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ReportedFinding {
    #[serde(flatten)]
    pub finding: Finding,
    /// If a waiver matched, the live waiver.
    pub waiver: Option<WaiverHit>,
}

#[derive(Clone, Debug, Serialize)]
pub struct WaiverHit {
    pub policy: String,
    pub reason: String,
    pub expires_at: DateTime<Utc>,
    pub owner: Option<String>,
    pub days_until_expiry: i64,
    /// True when ≤ 7 days left — callers display this as a yellow warning.
    pub expiring_soon: bool,
}

impl From<(&Waiver, DateTime<Utc>)> for WaiverHit {
    fn from((w, now): (&Waiver, DateTime<Utc>)) -> Self {
        let days = w.days_until_expiry(now);
        Self {
            policy: w.policy.clone(),
            reason: w.reason.clone(),
            expires_at: w.expires_at,
            owner: w.owner.clone(),
            days_until_expiry: days,
            expiring_soon: days <= 7 && days >= 0,
        }
    }
}

impl PolicyReport {
    /// Any unwaivered `Deny` finding?
    #[must_use]
    pub fn has_blocking(&self) -> bool {
        self.findings.iter().any(|f| f.finding.severity == Severity::Deny && f.waiver.is_none())
    }

    /// Count per-severity, after waiver resolution (waived findings are
    /// downgraded out of the denier bucket for the purposes of the
    /// overall gate).
    #[must_use]
    pub fn tally(&self) -> Tally {
        let mut t = Tally::default();
        for f in &self.findings {
            match (f.finding.severity, f.waiver.is_some()) {
                (Severity::Deny, false) => t.deny += 1,
                (Severity::Deny, true) => t.waived += 1,
                (Severity::Warn, _) => t.warn += 1,
                (Severity::Info, _) => t.info += 1,
            }
        }
        t
    }
}

#[derive(Copy, Clone, Debug, Default, Serialize)]
pub struct Tally {
    pub deny: usize,
    pub warn: usize,
    pub info: usize,
    pub waived: usize,
}
