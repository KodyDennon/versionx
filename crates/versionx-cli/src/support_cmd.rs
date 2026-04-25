use super::*;

pub(crate) fn run_doctor(
    cwd: Option<&camino::Utf8Path>,
    output: OutputFormat,
) -> Result<ExitCode> {
    let root = resolve_cwd(cwd).unwrap_or_else(|_| camino::Utf8PathBuf::from("."));
    let mut checks: Vec<(String, bool, String)> = Vec::new();

    let home = versionx_core::paths::VersionxHome::detect();
    checks.push(match &home {
        Ok(h) => ("VERSIONX_HOME".into(), true, h.data.to_string()),
        Err(e) => ("VERSIONX_HOME".into(), false, e.to_string()),
    });

    let in_git = versionx_git::read::summarize(&root).is_ok();
    checks.push(("git repo".into(), in_git, root.to_string()));

    let has_config = root.join("versionx.toml").is_file();
    checks.push((
        "versionx.toml present".into(),
        has_config,
        root.join("versionx.toml").to_string(),
    ));

    let has_lock = root.join("versionx.lock").is_file();
    checks.push(("versionx.lock present".into(), has_lock, root.join("versionx.lock").to_string()));

    let daemon_paths = versionx_daemon::DaemonPaths::from_env();
    let daemon_running = daemon_paths.as_ref().is_some_and(|p| {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .ok()
            .is_some_and(|rt| rt.block_on(versionx_daemon::is_running(p)))
    });
    checks.push((
        "daemon".into(),
        daemon_running,
        daemon_paths.as_ref().map_or_else(|| "no DaemonPaths".into(), |p| p.socket.to_string()),
    ));

    let shim_bin = versionx_core::commands::shim_install::shim_binary_path();
    checks.push(shim_bin.as_ref().map_or_else(
        || ("shim binary".into(), false, "not found".into()),
        |p| ("shim binary".into(), true, p.to_string()),
    ));

    let any_failed = checks.iter().any(|(_, ok, _)| !ok);

    match output {
        OutputFormat::Json | OutputFormat::Ndjson => {
            let payload: Vec<_> = checks
                .iter()
                .map(|(n, ok, d)| serde_json::json!({"check": n, "ok": ok, "detail": d}))
                .collect();
            let mut stdout = io::stdout().lock();
            serde_json::to_writer(&mut stdout, &payload)?;
            stdout.write_all(b"\n")?;
        }
        OutputFormat::Human => {
            for (name, ok, detail) in &checks {
                let mark = if *ok { "✓" } else { "✗" };
                println!("  {mark} {name:<28} {detail}");
            }
            if any_failed {
                println!();
                if !in_git {
                    println!("  → initialize a git repo or run Versionx inside one.");
                }
                if !has_config {
                    println!("  → run `versionx init` in the repo root to generate versionx.toml.");
                }
                if has_config && !has_lock {
                    println!("  → run `versionx sync` to create versionx.lock.");
                }
                if shim_bin.is_none() || !daemon_running {
                    println!(
                        "  → run `versionx install-shell-hook` and restart your shell for shims + daemon."
                    );
                }
                println!("  → rerun `versionx doctor` after setup changes.");
            }
        }
    }
    Ok(ExitCode::from(u8::from(any_failed)))
}

pub(crate) async fn run_exec(
    tool: String,
    args: Vec<String>,
    cwd: Option<&camino::Utf8Path>,
    output: OutputFormat,
) -> Result<ExitCode> {
    let root = resolve_cwd(cwd)?;
    let (_bus, ctx) = core_ctx()?;
    let opts = CoreWhichOpts { tool: tool.clone(), cwd: root };
    let resolved = match core_cmds::which(&ctx, &opts).await {
        Ok(o) => o,
        Err(err) => return Ok(render_core_error(&err, output, "exec")),
    };
    let Some(bin) = resolved.binary else {
        return bail_with(
            output,
            "exec",
            &format!("no resolved binary for `{tool}`: {}", resolved.reason),
        );
    };
    let status = std::process::Command::new(bin.as_str())
        .args(&args)
        .status()
        .with_context(|| format!("spawning {bin}"))?;
    Ok(ExitCode::from(status.code().unwrap_or(1) as u8))
}

pub(crate) fn run_self_check(
    cwd: Option<&camino::Utf8Path>,
    output: OutputFormat,
) -> Result<ExitCode> {
    let mut all_ok = true;

    let _ = run_doctor(cwd, output)?;
    if let Ok(root) = resolve_cwd(cwd)
        && versionx_git::read::summarize(&root).is_err()
    {
        all_ok = false;
    }

    let _ = run_verify(cwd, output)?;
    if let Ok(root) = resolve_cwd(cwd)
        && !root.join("versionx.lock").is_file()
        && root.join("versionx.toml").is_file()
    {
        all_ok = false;
    }

    if all_ok {
        emit_msg(output, "self-check passed", serde_json::json!({"ok": true}))?;
        Ok(ExitCode::from(0))
    } else {
        bail_with(output, "self-check", "one or more checks failed (see prior output)")
    }
}

pub(crate) fn run_changeset(
    sub: ChangesetCommand,
    cwd: Option<&camino::Utf8Path>,
    output: OutputFormat,
) -> Result<ExitCode> {
    let root = resolve_cwd(cwd)?;
    let dir = root.join(".changeset");
    std::fs::create_dir_all(dir.as_std_path()).context("creating .changeset/ dir")?;

    match sub {
        ChangesetCommand::Add { component, level, summary } => {
            let id = format!(
                "{}-{}",
                chrono::Utc::now().format("%Y%m%d-%H%M%S"),
                component.replace('/', "_")
            );
            let path = dir.join(format!("{id}.md"));
            let body = format!(
                "---\ncomponent: {component}\nlevel: {level}\n---\n\n{}\n",
                summary.unwrap_or_default()
            );
            std::fs::write(path.as_std_path(), body).context("writing changeset")?;
            emit_msg(
                output,
                &format!("wrote {path}"),
                serde_json::json!({"path": path.to_string()}),
            )?;
            Ok(ExitCode::from(0))
        }
        ChangesetCommand::List => {
            let mut entries: Vec<_> = std::fs::read_dir(dir.as_std_path())
                .context("reading .changeset/")?
                .filter_map(Result::ok)
                .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("md"))
                .collect();
            entries.sort_by_key(std::fs::DirEntry::file_name);
            match output {
                OutputFormat::Json | OutputFormat::Ndjson => {
                    let names: Vec<_> =
                        entries.iter().filter_map(|e| e.file_name().into_string().ok()).collect();
                    let mut stdout = io::stdout().lock();
                    serde_json::to_writer(&mut stdout, &names)?;
                    stdout.write_all(b"\n")?;
                }
                OutputFormat::Human => {
                    for e in &entries {
                        if let Some(name) = e.file_name().to_str() {
                            println!("  · {name}");
                        }
                    }
                }
            }
            Ok(ExitCode::from(0))
        }
        ChangesetCommand::Check => {
            let mut errors = Vec::new();
            let entries = std::fs::read_dir(dir.as_std_path()).context("reading .changeset/")?;
            for entry in entries.filter_map(Result::ok) {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) != Some("md") {
                    continue;
                }
                let raw = std::fs::read_to_string(&path).unwrap_or_default();
                if !raw.starts_with("---") {
                    errors.push(format!("{}: missing frontmatter", path.display()));
                    continue;
                }
                if !raw.contains("component:") || !raw.contains("level:") {
                    errors.push(format!(
                        "{}: missing required fields (component / level)",
                        path.display()
                    ));
                }
            }
            if errors.is_empty() {
                emit_msg(output, "all changesets valid", serde_json::json!({"ok": true}))?;
                Ok(ExitCode::from(0))
            } else {
                bail_with(output, "changeset check", &errors.join("; "))
            }
        }
    }
}
