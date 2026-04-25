use super::*;

#[derive(clap::Args, Debug)]
pub(crate) struct UpdateArgs {
    /// Compute the update plan and print it without executing anything.
    #[arg(long)]
    pub(crate) plan: bool,

    /// Restrict the update to a single ecosystem (`node`, `python`, `rust`).
    #[arg(long)]
    pub(crate) ecosystem: Option<String>,

    /// Optional package/dependency selector forwarded to the ecosystem adapter.
    pub(crate) spec: Option<String>,
}

pub(crate) async fn run_update(
    args: UpdateArgs,
    cwd: Option<&camino::Utf8Path>,
    output: OutputFormat,
) -> Result<ExitCode> {
    let root = resolve_cwd(cwd)?;
    let (_bus, ctx) = core_ctx()?;
    let opts = versionx_core::commands::UpdateOptions {
        root,
        dry_run: args.plan,
        spec: args.spec.clone(),
        ecosystem: args.ecosystem.clone(),
    };

    let outcome = match core_cmds::update(&ctx, &opts).await {
        Ok(o) => o,
        Err(err) => return Ok(render_core_error(&err, output, "update")),
    };

    match output {
        OutputFormat::Json | OutputFormat::Ndjson => {
            let mut stdout = io::stdout().lock();
            serde_json::to_writer(&mut stdout, &outcome)?;
            stdout.write_all(b"\n")?;
        }
        OutputFormat::Human => {
            if outcome.dry_run {
                println!("Planned dependency updates:");
            } else {
                println!("Updated dependencies — lockfile at {}", outcome.lockfile_path);
            }
            if let Some(spec) = &outcome.targeted_spec {
                println!("  target: {spec}");
            }
            if outcome.ecosystems.is_empty() {
                println!("  no configured ecosystems matched");
            }
            for eco in &outcome.ecosystems {
                if let Some(reason) = &eco.skipped_reason {
                    println!("  {} skipped: {}", eco.ecosystem, reason);
                    continue;
                }
                let pm = eco.package_manager.as_deref().unwrap_or("unknown-pm");
                let preview = eco.step_preview.as_deref().unwrap_or("<no plan>");
                println!("  {} ({pm}) @ {}", eco.ecosystem, eco.root);
                println!("    {preview}");
                for warning in &eco.warnings {
                    println!("    warning: {warning}");
                }
            }
            if outcome.dry_run {
                println!("  rerun without `--plan` to execute the update and refresh versionx.lock.");
            }
        }
    }

    Ok(ExitCode::from(0))
}
