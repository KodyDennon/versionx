//! Cross-repo release saga.
//!
//! Three modes:
//!   1. **Independent** — launch all member releases in parallel, no
//!      atomicity. If one fails the others still run; summary reports
//!      per-member status.
//!   2. **Gated** — execute in dependency order (topological).
//!      Stop on the first failure; successful members stay landed.
//!   3. **Coordinated** — two-phase with rollback:
//!        * Phase 1 (`dry_run`): validate every member can apply.
//!        * Phase 2 (`tag`): create annotated tags in all members.
//!        * Phase 3 (`publish`): land the releases in topo order.
//!        * On failure mid-phase-3, consult the [`RollbackStrategy`]:
//!          - `ManualRescue` — stop, leave state as-is, surface a
//!            report with the exact commits/tags that landed.
//!          - `AutoRevert` — create revert commits on every member
//!            that published, delete tags, push.
//!          - `Yank` — best-effort: delete tags locally + attempt
//!            registry unpublish (stubbed in 0.7; a true yank lives
//!            in 0.8 alongside OIDC publish).
//!
//! The saga is built on top of [`MemberStep`] — a thin trait every
//! per-member action must implement. The saga just sequences them.

use std::collections::BTreeMap;

use camino::{Utf8Path, Utf8PathBuf};
use serde::Serialize;

use crate::fleet::{FleetConfig, ReleaseSet};

#[derive(Debug, thiserror::Error)]
pub enum SagaError {
    #[error("unknown set `{name}`")]
    UnknownSet { name: String },
    #[error("step failed on member `{member}`: {message}")]
    StepFailed { member: String, message: String },
    #[error("member `{name}` path `{path}` is not a git repo")]
    NotARepo { name: String, path: Utf8PathBuf },
}

pub type SagaResult<T> = Result<T, SagaError>;

/// One per-member action. Must be implemented by the caller (the CLI
/// wires this to release-propose / apply against each member repo).
///
/// `Send + Sync` is required so [`run_independent`] can fan members
/// out across threads. Implementations that hold mutable shared state
/// should wrap it in a synchronisation primitive (Mutex / RwLock /
/// atomics).
pub trait MemberStep: Send + Sync {
    /// Phase 1: validate — return Ok if this member can land without
    /// mutating anything. Errors surface as dry-run failures.
    fn dry_run(&self, member_name: &str, member_root: &Utf8Path) -> SagaResult<()>;

    /// Phase 2: create the release tag locally (and whatever local
    /// artifacts the release needs). Idempotent.
    fn tag(&self, member_name: &str, member_root: &Utf8Path) -> SagaResult<TagInfo>;

    /// Phase 3: publish — push, upload, whatever "land" means for this
    /// member. On failure the saga decides whether to roll back.
    fn publish(&self, member_name: &str, member_root: &Utf8Path) -> SagaResult<()>;

    /// Rollback hook — undo whatever publish did. Should be
    /// best-effort; errors are logged but don't block.
    fn rollback(&self, member_name: &str, member_root: &Utf8Path, tag: &TagInfo) -> SagaResult<()>;
}

/// Info the caller hands back from `tag` so the saga can reference it
/// later (e.g. in the rollback step).
#[derive(Clone, Debug, Serialize)]
pub struct TagInfo {
    pub tag_name: String,
    pub commit_sha: String,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RollbackStrategy {
    ManualRescue,
    AutoRevert,
    Yank,
}

/// One member's final state in the saga report.
#[derive(Clone, Debug, Serialize)]
pub struct MemberOutcome {
    pub name: String,
    pub path: Utf8PathBuf,
    pub phase_reached: Phase,
    pub tag: Option<TagInfo>,
    pub error: Option<String>,
    pub rolled_back: bool,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Start,
    DryRun,
    Tagged,
    Published,
    Failed,
}

/// The saga's overall result.
#[derive(Clone, Debug, Serialize)]
pub struct SagaReport {
    pub mode: String,
    pub set: String,
    pub members: Vec<MemberOutcome>,
    pub succeeded: bool,
}

/// Execute a coordinated saga across every member of `set_name` in
/// `fleet`. `steps` is keyed by member name so the caller can pre-wire
/// per-member state.
pub fn run(
    fleet: &FleetConfig,
    fleet_root: &Utf8Path,
    set_name: &str,
    steps: &BTreeMap<String, Box<dyn MemberStep>>,
    rollback: RollbackStrategy,
) -> SagaResult<SagaReport> {
    let set = fleet.set(set_name).ok_or_else(|| SagaError::UnknownSet { name: set_name.into() })?;
    match set.release_mode.as_str() {
        "independent" => run_independent(fleet, fleet_root, set, steps),
        "gated" => run_gated(fleet, fleet_root, set, steps),
        _ => run_coordinated(fleet, fleet_root, set, steps, rollback),
    }
}

fn run_independent(
    fleet: &FleetConfig,
    fleet_root: &Utf8Path,
    set: &ReleaseSet,
    steps: &BTreeMap<String, Box<dyn MemberStep>>,
) -> SagaResult<SagaReport> {
    // True parallel fan-out: each member runs in its own thread.
    // Order of `outcomes` mirrors `set.members` (not completion order)
    // so reports stay deterministic.
    let outcomes: Vec<MemberOutcome> = std::thread::scope(|scope| {
        // We MUST collect handles eagerly: lazy iteration would
        // serialize the work because each `.map(|h| h.join())` would
        // block before the next `scope.spawn`. The `needless_collect`
        // clippy hint is a false positive here.
        #[allow(clippy::needless_collect)]
        let handles: Vec<_> = set
            .members
            .iter()
            .map(|name| {
                let name = name.clone();
                scope.spawn(move || apply_single(fleet, fleet_root, &name, steps))
            })
            .collect();
        handles
            .into_iter()
            .map(|h| {
                h.join().unwrap_or_else(|_| MemberOutcome {
                    name: "<panic>".into(),
                    path: Utf8PathBuf::from("."),
                    phase_reached: Phase::Failed,
                    tag: None,
                    error: Some("worker thread panicked".into()),
                    rolled_back: false,
                })
            })
            .collect()
    });

    let ok_total = outcomes.iter().all(|o| o.error.is_none());
    Ok(SagaReport {
        mode: "independent".into(),
        set: set.name.clone(),
        members: outcomes,
        succeeded: ok_total,
    })
}

fn run_gated(
    fleet: &FleetConfig,
    fleet_root: &Utf8Path,
    set: &ReleaseSet,
    steps: &BTreeMap<String, Box<dyn MemberStep>>,
) -> SagaResult<SagaReport> {
    let order = topological_order(set);
    let mut outcomes = Vec::new();
    let mut ok_total = true;
    for name in order {
        let outcome = apply_single(fleet, fleet_root, &name, steps);
        let failed = outcome.error.is_some();
        outcomes.push(outcome);
        if failed {
            ok_total = false;
            break;
        }
    }
    Ok(SagaReport {
        mode: "gated".into(),
        set: set.name.clone(),
        members: outcomes,
        succeeded: ok_total,
    })
}

fn run_coordinated(
    fleet: &FleetConfig,
    fleet_root: &Utf8Path,
    set: &ReleaseSet,
    steps: &BTreeMap<String, Box<dyn MemberStep>>,
    rollback: RollbackStrategy,
) -> SagaResult<SagaReport> {
    let order = topological_order(set);
    let mut outcomes: Vec<MemberOutcome> =
        order.iter().map(|n| new_outcome(fleet, n)).collect::<SagaResult<_>>()?;

    // Phase 1: dry-run everyone.
    for (i, name) in order.iter().enumerate() {
        let Some(step) = steps.get(name) else { continue };
        let member_root = member_path(fleet, fleet_root, name)?;
        if let Err(e) = step.dry_run(name, &member_root) {
            outcomes[i].error = Some(e.to_string());
            outcomes[i].phase_reached = Phase::Failed;
            return Ok(SagaReport {
                mode: "coordinated".into(),
                set: set.name.clone(),
                members: outcomes,
                succeeded: false,
            });
        }
        outcomes[i].phase_reached = Phase::DryRun;
    }

    // Phase 2: tag all.
    for (i, name) in order.iter().enumerate() {
        let Some(step) = steps.get(name) else { continue };
        let member_root = member_path(fleet, fleet_root, name)?;
        match step.tag(name, &member_root) {
            Ok(info) => {
                outcomes[i].tag = Some(info);
                outcomes[i].phase_reached = Phase::Tagged;
            }
            Err(e) => {
                outcomes[i].error = Some(e.to_string());
                outcomes[i].phase_reached = Phase::Failed;
                // Partial-tag failure: best we can do is surface; no
                // rollback needed because nothing's been published.
                return Ok(SagaReport {
                    mode: "coordinated".into(),
                    set: set.name.clone(),
                    members: outcomes,
                    succeeded: false,
                });
            }
        }
    }

    // Phase 3: publish in order. On failure, apply rollback.
    for (i, name) in order.iter().enumerate() {
        let Some(step) = steps.get(name) else { continue };
        let member_root = member_path(fleet, fleet_root, name)?;
        match step.publish(name, &member_root) {
            Ok(()) => {
                outcomes[i].phase_reached = Phase::Published;
            }
            Err(e) => {
                outcomes[i].error = Some(e.to_string());
                outcomes[i].phase_reached = Phase::Failed;
                apply_rollback(fleet, fleet_root, &order, &mut outcomes, steps, rollback);
                return Ok(SagaReport {
                    mode: "coordinated".into(),
                    set: set.name.clone(),
                    members: outcomes,
                    succeeded: false,
                });
            }
        }
    }

    Ok(SagaReport {
        mode: "coordinated".into(),
        set: set.name.clone(),
        members: outcomes,
        succeeded: true,
    })
}

fn apply_rollback(
    fleet: &FleetConfig,
    fleet_root: &Utf8Path,
    order: &[String],
    outcomes: &mut [MemberOutcome],
    steps: &BTreeMap<String, Box<dyn MemberStep>>,
    rollback: RollbackStrategy,
) {
    if matches!(rollback, RollbackStrategy::ManualRescue) {
        // Nothing to do — user will inspect outcomes and decide.
        return;
    }
    for (i, name) in order.iter().enumerate() {
        let Some(outcome) = outcomes.get_mut(i) else { continue };
        if outcome.phase_reached == Phase::Published
            && let Some(step) = steps.get(name)
            && let Some(tag) = &outcome.tag
            && let Ok(member_root) = member_path(fleet, fleet_root, name)
            && step.rollback(name, &member_root, tag).is_ok()
        {
            outcome.rolled_back = true;
        }
    }
}

fn apply_single(
    fleet: &FleetConfig,
    fleet_root: &Utf8Path,
    name: &str,
    steps: &BTreeMap<String, Box<dyn MemberStep>>,
) -> MemberOutcome {
    let mut outcome = match new_outcome(fleet, name) {
        Ok(o) => o,
        Err(e) => {
            return MemberOutcome {
                name: name.to_string(),
                path: Utf8PathBuf::from("."),
                phase_reached: Phase::Failed,
                tag: None,
                error: Some(e.to_string()),
                rolled_back: false,
            };
        }
    };
    let member_root = match member_path(fleet, fleet_root, name) {
        Ok(p) => p,
        Err(e) => {
            outcome.error = Some(e.to_string());
            outcome.phase_reached = Phase::Failed;
            return outcome;
        }
    };
    let Some(step) = steps.get(name) else {
        outcome.error = Some(format!("no MemberStep registered for `{name}`"));
        outcome.phase_reached = Phase::Failed;
        return outcome;
    };
    if let Err(e) = step.dry_run(name, &member_root) {
        outcome.error = Some(format!("dry_run: {e}"));
        outcome.phase_reached = Phase::Failed;
        return outcome;
    }
    outcome.phase_reached = Phase::DryRun;

    match step.tag(name, &member_root) {
        Ok(info) => {
            outcome.tag = Some(info);
            outcome.phase_reached = Phase::Tagged;
        }
        Err(e) => {
            outcome.error = Some(format!("tag: {e}"));
            outcome.phase_reached = Phase::Failed;
            return outcome;
        }
    }

    match step.publish(name, &member_root) {
        Ok(()) => outcome.phase_reached = Phase::Published,
        Err(e) => {
            outcome.error = Some(format!("publish: {e}"));
            outcome.phase_reached = Phase::Failed;
        }
    }
    outcome
}

fn new_outcome(fleet: &FleetConfig, name: &str) -> SagaResult<MemberOutcome> {
    let member = fleet.member(name).ok_or_else(|| SagaError::StepFailed {
        member: name.into(),
        message: "member not found in fleet".into(),
    })?;
    Ok(MemberOutcome {
        name: name.to_string(),
        path: member.path.clone(),
        phase_reached: Phase::Start,
        tag: None,
        error: None,
        rolled_back: false,
    })
}

fn member_path(fleet: &FleetConfig, fleet_root: &Utf8Path, name: &str) -> SagaResult<Utf8PathBuf> {
    let member = fleet.member(name).ok_or_else(|| SagaError::StepFailed {
        member: name.into(),
        message: "member not found".into(),
    })?;
    Ok(fleet_root.join(&member.path))
}

/// Deterministic topological sort honoring `set.depends_on` when
/// present; otherwise honors `set.members` order verbatim.
fn topological_order(set: &ReleaseSet) -> Vec<String> {
    if set.depends_on.is_empty() {
        return set.members.clone();
    }
    // Kahn's algorithm.
    let mut incoming: BTreeMap<String, usize> =
        set.members.iter().map(|m| (m.clone(), 0)).collect();
    let mut outgoing: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (member, deps) in &set.depends_on {
        for dep in deps {
            *incoming.entry(member.clone()).or_insert(0) += 1;
            outgoing.entry(dep.clone()).or_default().push(member.clone());
        }
    }
    let mut ready: Vec<String> =
        incoming.iter().filter(|(_, n)| **n == 0).map(|(k, _)| k.clone()).collect();
    // Preserve declaration order among the "ready" pool.
    ready.sort_by_key(|k| set.members.iter().position(|m| m == k).unwrap_or(usize::MAX));

    let mut out = Vec::new();
    while let Some(next) = ready.pop() {
        out.push(next.clone());
        if let Some(children) = outgoing.get(&next) {
            for child in children {
                if let Some(count) = incoming.get_mut(child) {
                    *count = count.saturating_sub(1);
                    if *count == 0 {
                        ready.push(child.clone());
                    }
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fleet::{Member, ReleaseSet};

    fn make_fleet() -> (FleetConfig, Utf8PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        let cfg = FleetConfig {
            schema_version: "1".into(),
            members: vec![
                Member {
                    name: "frontend".into(),
                    path: "frontend".into(),
                    remote: None,
                    branch: "main".into(),
                    tags: vec![],
                },
                Member {
                    name: "api".into(),
                    path: "api".into(),
                    remote: None,
                    branch: "main".into(),
                    tags: vec![],
                },
            ],
            sets: vec![ReleaseSet {
                name: "s".into(),
                members: vec!["frontend".into(), "api".into()],
                release_mode: "gated".into(),
                depends_on: indexmap::IndexMap::new(),
            }],
        };
        // Leak the tempdir so the test stays sane — not a real concern
        // for a unit test that doesn't touch disk.
        std::mem::forget(tmp);
        (cfg, root)
    }

    struct OkStep;
    impl MemberStep for OkStep {
        fn dry_run(&self, _: &str, _: &Utf8Path) -> SagaResult<()> {
            Ok(())
        }
        fn tag(&self, member: &str, _: &Utf8Path) -> SagaResult<TagInfo> {
            Ok(TagInfo { tag_name: format!("{member}@v1"), commit_sha: "a".repeat(40) })
        }
        fn publish(&self, _: &str, _: &Utf8Path) -> SagaResult<()> {
            Ok(())
        }
        fn rollback(&self, _: &str, _: &Utf8Path, _: &TagInfo) -> SagaResult<()> {
            Ok(())
        }
    }

    struct FailingPublishStep;
    impl MemberStep for FailingPublishStep {
        fn dry_run(&self, _: &str, _: &Utf8Path) -> SagaResult<()> {
            Ok(())
        }
        fn tag(&self, member: &str, _: &Utf8Path) -> SagaResult<TagInfo> {
            Ok(TagInfo { tag_name: format!("{member}@v1"), commit_sha: "b".repeat(40) })
        }
        fn publish(&self, member: &str, _: &Utf8Path) -> SagaResult<()> {
            Err(SagaError::StepFailed { member: member.into(), message: "simulated".into() })
        }
        fn rollback(&self, _: &str, _: &Utf8Path, _: &TagInfo) -> SagaResult<()> {
            Ok(())
        }
    }

    #[test]
    fn coordinated_happy_path() {
        let (cfg, root) = make_fleet();
        let mut steps: BTreeMap<String, Box<dyn MemberStep>> = BTreeMap::new();
        steps.insert("frontend".into(), Box::new(OkStep));
        steps.insert("api".into(), Box::new(OkStep));
        let mut cfg = cfg;
        cfg.sets[0].release_mode = "coordinated".into();
        let report = run(&cfg, &root, "s", &steps, RollbackStrategy::ManualRescue).unwrap();
        assert!(report.succeeded);
        assert_eq!(report.members.len(), 2);
        assert!(report.members.iter().all(|m| m.phase_reached == Phase::Published));
    }

    #[test]
    fn coordinated_rollback_reverts_earlier_members() {
        let (mut cfg, root) = make_fleet();
        cfg.sets[0].release_mode = "coordinated".into();
        let mut steps: BTreeMap<String, Box<dyn MemberStep>> = BTreeMap::new();
        steps.insert("frontend".into(), Box::new(OkStep));
        steps.insert("api".into(), Box::new(FailingPublishStep));
        let report = run(&cfg, &root, "s", &steps, RollbackStrategy::AutoRevert).unwrap();
        assert!(!report.succeeded);
        let fe = report.members.iter().find(|m| m.name == "frontend").unwrap();
        assert!(fe.rolled_back, "frontend should have been rolled back");
    }

    #[test]
    fn gated_stops_on_first_failure() {
        let (cfg, root) = make_fleet();
        let mut steps: BTreeMap<String, Box<dyn MemberStep>> = BTreeMap::new();
        steps.insert("frontend".into(), Box::new(FailingPublishStep));
        steps.insert("api".into(), Box::new(OkStep));
        let report = run(&cfg, &root, "s", &steps, RollbackStrategy::ManualRescue).unwrap();
        assert!(!report.succeeded);
        assert_eq!(report.members.len(), 1); // stopped after frontend failed
    }

    #[test]
    fn independent_runs_in_parallel() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::time::{Duration, Instant};

        // Each step sleeps 200ms in `tag`. Sequential = 800ms+.
        // Parallel = ~200ms.
        struct SlowStep {
            counter: Arc<AtomicUsize>,
        }
        impl MemberStep for SlowStep {
            fn dry_run(&self, _: &str, _: &Utf8Path) -> SagaResult<()> {
                Ok(())
            }
            fn tag(&self, member: &str, _: &Utf8Path) -> SagaResult<TagInfo> {
                std::thread::sleep(Duration::from_millis(200));
                self.counter.fetch_add(1, Ordering::Relaxed);
                Ok(TagInfo { tag_name: format!("{member}@v1"), commit_sha: "c".repeat(40) })
            }
            fn publish(&self, _: &str, _: &Utf8Path) -> SagaResult<()> {
                Ok(())
            }
            fn rollback(&self, _: &str, _: &Utf8Path, _: &TagInfo) -> SagaResult<()> {
                Ok(())
            }
        }

        let tmp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        let cfg = FleetConfig {
            schema_version: "1".into(),
            members: vec![
                Member {
                    name: "a".into(),
                    path: "a".into(),
                    remote: None,
                    branch: "main".into(),
                    tags: vec![],
                },
                Member {
                    name: "b".into(),
                    path: "b".into(),
                    remote: None,
                    branch: "main".into(),
                    tags: vec![],
                },
                Member {
                    name: "c".into(),
                    path: "c".into(),
                    remote: None,
                    branch: "main".into(),
                    tags: vec![],
                },
                Member {
                    name: "d".into(),
                    path: "d".into(),
                    remote: None,
                    branch: "main".into(),
                    tags: vec![],
                },
            ],
            sets: vec![ReleaseSet {
                name: "s".into(),
                members: vec!["a".into(), "b".into(), "c".into(), "d".into()],
                release_mode: "independent".into(),
                depends_on: indexmap::IndexMap::new(),
            }],
        };
        let counter = Arc::new(AtomicUsize::new(0));
        let mut steps: BTreeMap<String, Box<dyn MemberStep>> = BTreeMap::new();
        for name in ["a", "b", "c", "d"] {
            steps.insert(name.into(), Box::new(SlowStep { counter: counter.clone() }));
        }

        let started = Instant::now();
        let report = run(&cfg, &root, "s", &steps, RollbackStrategy::ManualRescue).unwrap();
        let elapsed = started.elapsed();

        assert!(report.succeeded);
        assert_eq!(counter.load(Ordering::Relaxed), 4);
        // Sequential = 4 * 200ms = 800ms. Parallel should finish in
        // well under 600ms even on a loaded CI box.
        assert!(elapsed < Duration::from_millis(600), "took {elapsed:?}");
    }

    #[test]
    fn topo_order_respects_depends_on() {
        let mut depends_on = indexmap::IndexMap::new();
        depends_on.insert("frontend".into(), vec!["api".into()]);
        let set = ReleaseSet {
            name: "s".into(),
            members: vec!["frontend".into(), "api".into()],
            release_mode: "gated".into(),
            depends_on,
        };
        let order = topological_order(&set);
        let api_idx = order.iter().position(|x| x == "api").unwrap();
        let fe_idx = order.iter().position(|x| x == "frontend").unwrap();
        assert!(api_idx < fe_idx, "api must come before frontend");
    }
}
