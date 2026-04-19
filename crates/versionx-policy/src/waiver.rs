//! Waiver resolver.
//!
//! Given a [`Finding`] and the available [`Waiver`]s, decide whether
//! the finding is waived. A live waiver:
//!   - Targets the finding's policy by name.
//!   - Has `expires_at` in the future.
//!   - Matches the finding's scope (if the waiver carries an explicit
//!     `scope` block).
//!
//! Expired waivers are surfaced separately so callers can audit +
//! warn about them. Waivers approaching expiry (`≤ 7 days`) are
//! flagged by the [`crate::finding::WaiverHit::expiring_soon`] field.

use chrono::{DateTime, Utc};

use crate::finding::{Finding, WaiverHit};
use crate::schema::Waiver;

/// Look for a live waiver for `finding`. Returns a [`WaiverHit`] if one
/// matches, else `None`.
#[must_use]
pub fn match_waiver<'a>(
    finding: &Finding,
    waivers: &'a [Waiver],
    now: DateTime<Utc>,
) -> Option<WaiverHit> {
    for w in waivers {
        if !w.policy.eq_ignore_ascii_case(&finding.policy) {
            continue;
        }
        if !w.is_live(now) {
            continue;
        }
        if !scope_matches(w, finding) {
            continue;
        }
        return Some(WaiverHit::from((w, now)));
    }
    None
}

/// Summary of waivers: live, expiring-soon, expired.
#[derive(Clone, Debug, Default)]
pub struct WaiverAudit {
    pub live: Vec<String>,
    pub expiring_soon: Vec<String>,
    pub expired: Vec<String>,
}

/// Audit every waiver in `waivers` against `now`.
#[must_use]
pub fn audit(waivers: &[Waiver], now: DateTime<Utc>) -> WaiverAudit {
    let mut out = WaiverAudit::default();
    for w in waivers {
        let days = w.days_until_expiry(now);
        if days < 0 {
            out.expired.push(format!("{} (expired {} days ago)", w.policy, -days));
        } else if days <= 7 {
            out.expiring_soon.push(format!("{} (expires in {days}d: {})", w.policy, w.reason));
        } else {
            out.live.push(w.policy.clone());
        }
    }
    out
}

fn scope_matches(waiver: &Waiver, finding: &Finding) -> bool {
    let Some(scope) = &waiver.scope else { return true };
    if !scope.list.is_empty() {
        return finding.component.as_ref().is_some_and(|c| scope.list.iter().any(|s| s == c));
    }
    scope.all
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::finding::Finding;
    use crate::schema::{PolicyKind, Scope, Severity};
    use chrono::Duration;

    fn finding_for(policy: &str, component: Option<&str>) -> Finding {
        Finding {
            policy: policy.into(),
            kind: PolicyKind::RuntimeVersion,
            severity: Severity::Deny,
            component: component.map(str::to_string),
            message: "x".into(),
        }
    }

    fn waiver(policy: &str, days: i64, scope: Option<Scope>) -> Waiver {
        Waiver {
            policy: policy.into(),
            reason: "r".into(),
            expires_at: Utc::now() + Duration::days(days),
            owner: None,
            scope,
        }
    }

    #[test]
    fn live_waiver_matches() {
        let w = waiver("rt", 10, None);
        let f = finding_for("rt", None);
        let hit = match_waiver(&f, &[w], Utc::now()).unwrap();
        assert!(hit.expires_at > Utc::now());
        assert!(!hit.expiring_soon);
    }

    #[test]
    fn expiring_soon_flagged() {
        let w = waiver("rt", 3, None);
        let f = finding_for("rt", None);
        let hit = match_waiver(&f, &[w], Utc::now()).unwrap();
        assert!(hit.expiring_soon);
    }

    #[test]
    fn expired_waiver_does_not_match() {
        let w = waiver("rt", -1, None);
        let f = finding_for("rt", None);
        assert!(match_waiver(&f, &[w], Utc::now()).is_none());
    }

    #[test]
    fn scope_list_restricts() {
        let scope = Scope { all: false, list: vec!["core".into()], ..Scope::default() };
        let w = waiver("rt", 30, Some(scope));
        let finding_core = finding_for("rt", Some("core"));
        let finding_app = finding_for("rt", Some("app"));
        assert!(match_waiver(&finding_core, &[w.clone()], Utc::now()).is_some());
        assert!(match_waiver(&finding_app, &[w], Utc::now()).is_none());
    }

    #[test]
    fn audit_splits_buckets() {
        let ws = [waiver("a", 30, None), waiver("b", 3, None), waiver("c", -5, None)];
        let a = audit(&ws, Utc::now());
        assert_eq!(a.live.len(), 1);
        assert_eq!(a.expiring_soon.len(), 1);
        assert_eq!(a.expired.len(), 1);
    }
}
