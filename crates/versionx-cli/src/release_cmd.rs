use super::*;

#[allow(clippy::too_many_lines)]
pub(crate) fn run_release(
    sub: ReleaseCommand,
    cwd: Option<&camino::Utf8Path>,
    output: OutputFormat,
) -> Result<ExitCode> {
    use versionx_release::{ReleasePlan, plan as plan_mod, propose as propose_mod};
    let root = resolve_cwd(cwd)?;
    let plans_dir = plan_mod::plans_dir(&root);

    match sub {
        ReleaseCommand::Propose { strategy, pr_title } => {
            let last = versionx_core::commands::workspace::load_last_hashes(&root);
            let input = propose_mod::ProposeInput {
                strategy,
                commit_messages: collect_commit_messages(&root),
                pr_title,
                groups: Vec::new(),
                ttl: None,
            };
            let plan = match propose_mod::propose(&root, &last, &input) {
                Ok(p) => p,
                Err(err) => return bail_with(output, "release propose", &err.to_string()),
            };

            if let Ok(set) = versionx_policy::load_and_verify(&root, &[]) {
                let ctx =
                    build_policy_context(&root, Some(versionx_policy::Trigger::ReleasePropose))?;
                if let Ok(report) = versionx_policy::evaluate(&set, &ctx)
                    && report.has_blocking()
                {
                    emit_policy_report(&report, output)?;
                    return bail_with(
                        output,
                        "release propose",
                        "blocked by policy findings — see report above",
                    );
                }
            }

            let saved_to = plan.save(&plans_dir).context("writing plan")?;
            emit_plan(&plan, Some(&saved_to), output)
        }
        ReleaseCommand::Show { plan_id } => {
            let plan = ReleasePlan::load_by_id(&plans_dir, &plan_id)
                .with_context(|| format!("loading plan {plan_id}"))?;
            emit_plan(&plan, None, output)
        }
        ReleaseCommand::List => {
            let plans = plan_mod::list_plans(&plans_dir).context("listing plans")?;
            emit_plan_list(&plans, output)
        }
        ReleaseCommand::Approve { plan_id } => {
            let mut plan = ReleasePlan::load_by_id(&plans_dir, &plan_id)
                .with_context(|| format!("loading plan {plan_id}"))?;
            plan.approve();
            plan.save(&plans_dir).context("writing approved plan")?;
            emit_msg(
                output,
                &format!("approved {}", plan.plan_id),
                serde_json::json!({"approved": plan.plan_id}),
            )?;
            Ok(ExitCode::from(0))
        }
        ReleaseCommand::Apply { plan_id, allow_dirty } => {
            let plan = ReleasePlan::load_by_id(&plans_dir, &plan_id)
                .with_context(|| format!("loading plan {plan_id}"))?;

            if let Ok(set) = versionx_policy::load_and_verify(&root, &[]) {
                let ctx =
                    build_policy_context(&root, Some(versionx_policy::Trigger::ReleaseApply))?;
                if let Ok(report) = versionx_policy::evaluate(&set, &ctx)
                    && report.has_blocking()
                {
                    emit_policy_report(&report, output)?;
                    return bail_with(
                        output,
                        "release apply",
                        "blocked by policy findings — see report above",
                    );
                }
            }

            let input = versionx_release::ApplyInput {
                commit_messages: collect_commit_messages(&root),
                enforce_clean_tree: !allow_dirty,
                ..versionx_release::ApplyInput::new(root.clone())
            };
            let outcome = match versionx_release::apply(&plan, &input) {
                Ok(o) => o,
                Err(err) => return bail_with(output, "release apply", &err.to_string()),
            };
            emit_apply_outcome(&outcome, output)
        }
        ReleaseCommand::Snapshot { prefix } => {
            let summary = versionx_git::read::summarize(&root)
                .map_err(|e| anyhow::anyhow!("git read: {e}"))?;
            let short: String = summary.head_sha.chars().take(7).collect();
            let date = chrono::Utc::now().format("%Y%m%d");
            let tag_name = format!("{prefix}-{date}-{short}");

            let repo = versionx_git::open(&root).map_err(|e| anyhow::anyhow!("open repo: {e}"))?;
            let message = format!("Versionx snapshot {tag_name}");
            if let Err(e) = versionx_git::tag(&repo, &tag_name, &message) {
                return bail_with(output, "release snapshot", &e.to_string());
            }
            emit_msg(
                output,
                &format!("created snapshot tag {tag_name}"),
                serde_json::json!({
                    "tag": tag_name,
                    "head_sha": summary.head_sha,
                }),
            )?;
            Ok(ExitCode::from(0))
        }
        ReleaseCommand::Rollback { plan_id } => {
            let events = versionx_git::history::list(&root, 1000)
                .map_err(|e| anyhow::anyhow!("history list: {e}"))?;
            let apply_event = events
                .iter()
                .find(|e| {
                    e.kind == "release.apply"
                        && e.details.get("plan_id").and_then(|v| v.as_str())
                            == Some(plan_id.as_str())
                })
                .ok_or_else(|| anyhow::anyhow!("no release.apply event found for plan {plan_id}"))?;
            let commit = apply_event
                .details
                .get("commit")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("apply event missing commit field"))?;

            let repo = versionx_git::open(&root).map_err(|e| anyhow::anyhow!("open repo: {e}"))?;
            let revert_sha = match versionx_git::revert_commit(&repo, commit) {
                Ok(s) => s,
                Err(e) => return bail_with(output, "release rollback", &e.to_string()),
            };
            if let Some(tags) = apply_event.details.get("tags").and_then(|v| v.as_array()) {
                for t in tags {
                    if let Some(tag_name) = t.as_str() {
                        let _ = versionx_git::delete_tag(&repo, tag_name);
                    }
                }
            }
            emit_msg(
                output,
                &format!("rolled back plan {plan_id} via commit {revert_sha}"),
                serde_json::json!({
                    "plan_id": plan_id,
                    "reverted_commit": commit,
                    "revert_commit": revert_sha,
                }),
            )?;
            Ok(ExitCode::from(0))
        }
        ReleaseCommand::Prerelease { plan_id, channel } => {
            let mut plan = ReleasePlan::load_by_id(&plans_dir, &plan_id)
                .with_context(|| format!("loading plan {plan_id}"))?;
            for b in &mut plan.bumps {
                if !b.to.contains('-') {
                    b.to = format!("{}-{channel}.0", b.to);
                }
            }
            let prerel_id = format!("{plan_id}-{channel}");
            plan.plan_id = prerel_id;
            plan.save(&plans_dir).context("writing prerelease plan")?;

            let input = versionx_release::ApplyInput {
                commit_messages: collect_commit_messages(&root),
                enforce_clean_tree: false,
                ..versionx_release::ApplyInput::new(root.clone())
            };
            let outcome = match versionx_release::apply(&plan, &input) {
                Ok(o) => o,
                Err(err) => return bail_with(output, "release prerelease", &err.to_string()),
            };
            emit_apply_outcome(&outcome, output)
        }
    }
}

pub(crate) fn run_plan(
    sub: PlanCommand,
    cwd: Option<&camino::Utf8Path>,
    output: OutputFormat,
) -> Result<ExitCode> {
    use versionx_release::{ReleasePlan, plan as plan_mod};
    let root = resolve_cwd(cwd)?;
    let plans_dir = plan_mod::plans_dir(&root);

    match sub {
        PlanCommand::List => {
            let plans = plan_mod::list_plans(&plans_dir).context("listing plans")?;
            emit_plan_list(&plans, output)
        }
        PlanCommand::Show { plan_id } => {
            let plan = ReleasePlan::load_by_id(&plans_dir, &plan_id)
                .with_context(|| format!("loading plan {plan_id}"))?;
            emit_plan(&plan, None, output)
        }
        PlanCommand::Expire => {
            let removed =
                plan_mod::expire_plans(&plans_dir, chrono::Utc::now()).context("expiring plans")?;
            emit_msg(
                output,
                &format!("expired {} plans", removed.len()),
                serde_json::json!({"expired": removed}),
            )?;
            Ok(ExitCode::from(0))
        }
        PlanCommand::Apply { plan_id, allow_dirty } => {
            run_release(ReleaseCommand::Apply { plan_id, allow_dirty }, cwd, output)
        }
    }
}

fn emit_plan(
    plan: &versionx_release::ReleasePlan,
    saved_to: Option<&camino::Utf8Path>,
    output: OutputFormat,
) -> Result<ExitCode> {
    match output {
        OutputFormat::Json | OutputFormat::Ndjson => {
            let payload = serde_json::json!({
                "plan_id": plan.plan_id,
                "workspace_root": plan.workspace_root,
                "pre_requisite_hash": plan.pre_requisite_hash,
                "created_at": plan.created_at,
                "expires_at": plan.expires_at,
                "approved": plan.approved,
                "strategy": plan.strategy,
                "bumps": plan.bumps,
                "saved_to": saved_to.map(ToString::to_string),
            });
            let mut stdout = io::stdout().lock();
            serde_json::to_writer(&mut stdout, &payload)?;
            stdout.write_all(b"\n")?;
        }
        OutputFormat::Human => {
            println!("plan_id: {}", plan.plan_id);
            println!("strategy: {}", plan.strategy);
            println!("approved: {}", plan.approved);
            println!("expires_at: {}", plan.expires_at);
            if let Some(p) = saved_to {
                println!("saved_to: {p}");
            }
            if plan.bumps.is_empty() {
                println!("no bumps proposed — workspace is clean.");
            } else {
                println!("proposed bumps ({}):", plan.bumps.len());
                for b in &plan.bumps {
                    let from = b.from.as_deref().unwrap_or("—");
                    println!("  {:<32} {from} -> {:<10} [{}]", b.id, b.to, b.level);
                }
            }
        }
    }
    Ok(ExitCode::from(0))
}

fn emit_plan_list(
    plans: &[versionx_release::ReleasePlan],
    output: OutputFormat,
) -> Result<ExitCode> {
    match output {
        OutputFormat::Json | OutputFormat::Ndjson => {
            let summary: Vec<_> = plans
                .iter()
                .map(|p| {
                    serde_json::json!({
                        "plan_id": p.plan_id,
                        "created_at": p.created_at,
                        "expires_at": p.expires_at,
                        "approved": p.approved,
                        "strategy": p.strategy,
                        "bump_count": p.bumps.len(),
                    })
                })
                .collect();
            let mut stdout = io::stdout().lock();
            serde_json::to_writer(&mut stdout, &summary)?;
            stdout.write_all(b"\n")?;
        }
        OutputFormat::Human => {
            if plans.is_empty() {
                println!("no plans found.");
            } else {
                for p in plans {
                    let flag = if p.approved { "✓" } else { "·" };
                    println!(
                        "  {flag} {} ({} bumps, expires {})",
                        p.plan_id,
                        p.bumps.len(),
                        p.expires_at
                    );
                }
            }
        }
    }
    Ok(ExitCode::from(0))
}

fn emit_apply_outcome(
    outcome: &versionx_release::ApplyOutcome,
    output: OutputFormat,
) -> Result<ExitCode> {
    match output {
        OutputFormat::Json | OutputFormat::Ndjson => {
            let mut stdout = io::stdout().lock();
            serde_json::to_writer(&mut stdout, outcome)?;
            stdout.write_all(b"\n")?;
        }
        OutputFormat::Human => {
            println!("applied plan {}", outcome.plan_id);
            println!("commit: {}", outcome.commit);
            println!("tags: {}", outcome.tags.join(", "));
            for b in &outcome.bumped {
                let from = b.from.as_deref().unwrap_or("—");
                println!("  {}: {from} -> {} ({})", b.id, b.to, b.tag);
            }
        }
    }
    Ok(ExitCode::from(0))
}

pub(crate) fn collect_commit_messages(root: &camino::Utf8Path) -> Vec<String> {
    let output = std::process::Command::new("git")
        .args(["-C", root.as_str(), "log", "-n", "100", "--pretty=%B%x00"])
        .output();
    let Ok(out) = output else { return Vec::new() };
    if !out.status.success() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&out.stdout);
    text.split('\0').map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect()
}
