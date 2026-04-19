//! The `versionx` CLI entry point.
//!
//! This is the main binary users invoke. Every subcommand routes through
//! `versionx-core` — the CLI never calls git, adapters, or ecosystem tools
//! directly (see `docs/spec/01-architecture-overview.md`).
//!
//! Status: 0.1.0 scaffold. Commands return informative `NotImplemented` errors
//! until each subsystem lands per `docs/spec/11-version-roadmap.md`.

#![deny(unsafe_code)]

use std::io::{self, Write};
use std::process::ExitCode;

use anyhow::{Context, Result};
use camino::Utf8PathBuf;
use clap::{Parser, Subcommand, ValueEnum};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};
use versionx_core::EventBus;
use versionx_core::commands::{
    self as core_cmds, ActivateOptions as CoreActivate, CoreContext,
    InstallOptions as CoreInstallOpts, Shell as CoreShell, SyncOptions as CoreSyncOpts,
    WhichOptions as CoreWhichOpts, init as core_init,
};

/// Command-line interface for Versionx, the polyglot version manager +
/// release orchestrator.
///
/// See `docs/spec/00-README.md` for the design principles. Most flags on the
/// root command (like `--output`) apply to every subcommand.
#[derive(Parser, Debug)]
#[command(
    name = "versionx",
    version,
    about = "Cross-platform, cross-language, cross-package-manager version manager.",
    long_about = None,
    disable_help_subcommand = true,
)]
struct Cli {
    /// Output format. `human` is the colored pretty default; `json` emits
    /// a single JSON object; `ndjson` streams newline-delimited events
    /// suitable for AI agents and shell pipelines.
    #[arg(long, value_enum, default_value_t = OutputFormat::Human, global = true)]
    output: OutputFormat,

    /// Suppress all output except errors.
    #[arg(long, short, global = true)]
    quiet: bool,

    /// Increase output verbosity (`-v`, `-vv`, `-vvv`).
    #[arg(long, short, action = clap::ArgAction::Count, global = true)]
    verbose: u8,

    /// Working directory for the command. Defaults to the current directory.
    #[arg(long, global = true)]
    cwd: Option<Utf8PathBuf>,

    /// Bypass the daemon and run every operation in-process (slower).
    #[arg(long, global = true)]
    no_daemon: bool,

    /// Emit the full command tree as JSON and exit. Used by the MCP server
    /// and by agents that want to introspect Versionx's capabilities.
    #[arg(long, hide = true)]
    help_json: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
enum OutputFormat {
    Human,
    Json,
    Ndjson,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Synthesize a `versionx.toml` from detected ecosystems and write it
    /// to the repo root. Safe to re-run.
    Init(InitArgs),

    /// Install everything declared in `versionx.toml` + `versionx.lock`:
    /// toolchains, package managers, and dependencies.
    Sync(SyncArgs),

    /// Verify that the lockfile matches the current manifest state.
    /// Designed for CI as a fast fail-closed integrity check.
    Verify,

    /// Print a summary of the current workspace: detected ecosystems,
    /// pinned runtimes, outstanding changes, and policy findings.
    Status,

    /// Install a toolchain globally (outside a repo context). Mirrors
    /// `mise install <tool> <version>` for users migrating from mise/asdf.
    Install(InstallArgs),

    /// Show which binary the shim resolves to in the current context
    /// and why. Debugging aid for `PATH` / shim behavior.
    Which {
        /// The tool name to resolve (e.g. `node`, `python`, `cargo`).
        tool: String,
    },

    /// Emit a shell integration hook. Put `eval "$(versionx activate bash)"`
    /// in your shell rc file to start the daemon on login and prepend the
    /// shims dir to `PATH`.
    Activate(ActivateArgs),

    /// Runtime installation and management subcommands.
    #[command(subcommand)]
    Runtime(RuntimeCommand),

    /// User-level default pins (falls back when no versionx.toml pins a tool).
    #[command(subcommand)]
    Global(GlobalCommand),

    /// Inspect your workspace — components, change state, dependency graph.
    #[command(subcommand)]
    Workspace(WorkspaceCommand),

    /// Propose version bumps for every dirty component (+ cascade).
    /// This is a preview of the 0.4 release orchestration: the plan is
    /// printed but nothing is written.
    Bump,

    /// Start or manage the `versiond` background daemon.
    #[command(subcommand)]
    Daemon(DaemonCommand),

    /// Launch the interactive terminal dashboard.
    Tui,

    /// Release orchestration subcommands (plan/apply, publish, snapshots).
    #[command(subcommand)]
    Release(ReleaseCommand),

    /// Manage stored release plans under `.versionx/plans/`.
    #[command(subcommand)]
    Plan(PlanCommand),

    /// Policy authoring and evaluation.
    #[command(subcommand)]
    Policy(PolicyCommand),

    /// Waiver management — time-boxed policy exceptions.
    #[command(subcommand)]
    Waiver(WaiverCommand),

    /// MCP server for AI-agent integration.
    #[command(subcommand)]
    Mcp(McpCommand),
}

#[derive(clap::Args, Debug)]
struct InitArgs {
    /// Overwrite an existing `versionx.toml` without prompting.
    #[arg(long)]
    force: bool,
}

#[derive(clap::Args, Debug)]
struct SyncArgs {
    /// Compute the plan and emit it without executing anything.
    #[arg(long)]
    plan_only: bool,

    /// Apply a previously-computed plan from `<FILE>`. Refused if the plan's
    /// `pre_requisite_hash` does not match the current lockfile's hash.
    #[arg(long, value_name = "FILE")]
    apply: Option<Utf8PathBuf>,

    /// Override concurrency (default: `num_cpus::get().min(8)`).
    #[arg(long)]
    jobs: Option<usize>,
}

#[derive(clap::Args, Debug)]
struct InstallArgs {
    /// Tool to install (e.g. `node`, `python`, `rust`, `pnpm`, `uv`).
    tool: String,
    /// Version spec: exact, semver range, `lts`, `stable`, or a channel.
    version: String,
}

#[derive(clap::Args, Debug)]
struct ActivateArgs {
    /// Which shell's hook to emit.
    #[arg(value_enum)]
    shell: Shell,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum Shell {
    Bash,
    Zsh,
    Fish,
    Pwsh,
}

#[derive(Subcommand, Debug)]
enum RuntimeCommand {
    /// List installed toolchains.
    List,
    /// Remove toolchains unused for the given age cutoff.
    Prune {
        /// Minimum age in days before a toolchain is eligible for prune.
        #[arg(long, default_value_t = 90)]
        older_than_days: u32,
        /// Preview the prune without touching disk.
        #[arg(long)]
        dry_run: bool,
        /// Always keep the newest install per tool.
        #[arg(long, default_value_t = true)]
        keep_latest: bool,
    },
}

#[derive(Subcommand, Debug)]
enum WorkspaceCommand {
    /// List every component discovered in this workspace.
    List,
    /// Report per-component change state (dirty / clean) vs. last release.
    Status,
    /// Print the dep DAG (nodes + edges + topological order).
    Graph,
}

#[derive(Subcommand, Debug)]
enum GlobalCommand {
    /// Set a user-wide default version for `<tool>`.
    Set { tool: String, version: String },
    /// Read the user-wide default for `<tool>` (empty if none).
    Get { tool: String },
    /// Remove any user-wide default for `<tool>`.
    Unset { tool: String },
    /// List every pinned tool in the user's global config.
    List,
}

#[derive(Subcommand, Debug)]
enum DaemonCommand {
    /// Start the daemon in the foreground (for debugging) or detached.
    Start {
        /// Run in the foreground instead of detaching.
        #[arg(long)]
        foreground: bool,
    },
    /// Stop a running daemon.
    Stop,
    /// Report daemon status.
    Status,
    /// Tail the daemon's structured log file.
    Logs {
        /// Number of lines to dump before streaming further writes.
        #[arg(long, default_value_t = 200)]
        tail: usize,
        /// Only dump existing lines and exit (don't keep watching the file).
        #[arg(long)]
        no_follow: bool,
    },
}

#[derive(Subcommand, Debug)]
enum ReleaseCommand {
    /// Compute a release plan without executing it.
    Propose {
        /// Strategy: `conventional` (default), `pr-title`, `manual`.
        #[arg(long, default_value = "conventional")]
        strategy: String,
        /// Override PR title (otherwise harvested from git).
        #[arg(long)]
        pr_title: Option<String>,
    },
    /// Show a single plan by id.
    Show { plan_id: String },
    /// List every plan under .versionx/plans/.
    List,
    /// Approve a previously-proposed plan by id.
    Approve { plan_id: String },
    /// Apply an approved release plan (local-only — push is a future flag).
    Apply {
        /// Plan id to apply.
        plan_id: String,
        /// Allow applying even when the working tree has unrelated changes.
        #[arg(long)]
        allow_dirty: bool,
    },
}

#[derive(Subcommand, Debug)]
enum PlanCommand {
    /// List every plan under `.versionx/plans/`.
    List,
    /// Show a plan body by id.
    Show { plan_id: String },
    /// Delete all expired plans.
    Expire,
}

#[derive(Subcommand, Debug)]
enum PolicyCommand {
    /// Scaffold a starter policy file under `.versionx/policies/`.
    Init,
    /// Evaluate all policies against the current workspace.
    Check,
    /// Explain why a specific policy fired (or didn't) by name.
    Explain { name: String },
    /// List loaded policies + their kinds.
    List,
    /// Show aggregate counts per kind / severity.
    Stats,
    /// Refresh the policy lockfile (versionx.policy.lock) with current
    /// content hashes for every loaded policy source.
    Update,
    /// Audit waivers for expired / expiring-soon entries.
    Audit,
}

#[derive(Subcommand, Debug)]
enum WaiverCommand {
    /// List every waiver with days-until-expiry.
    List,
    /// Audit waivers: split into live / expiring-soon / expired.
    Audit,
    /// Delete every already-expired waiver from the policies file at
    /// `<path>` (default: .versionx/policies/local.toml).
    Expire {
        #[arg(long, value_name = "FILE")]
        path: Option<Utf8PathBuf>,
    },
}

#[derive(Subcommand, Debug)]
enum McpCommand {
    /// Serve the MCP protocol over stdio (the default for agent integrations).
    Serve {
        /// Listen on a loopback HTTP port instead of stdio.
        #[arg(long)]
        http: Option<u16>,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    // Default filter: warn only. `-v` promotes to info, `-vv` to debug,
    // `-vvv` to trace. Always overridable via `VERSIONX_LOG`.
    let base = match cli.verbose {
        0 => "warn",
        1 => "versionx=info,warn",
        2 => "versionx=debug,warn",
        _ => "trace",
    };
    let env_filter =
        EnvFilter::try_from_env("VERSIONX_LOG").unwrap_or_else(|_| EnvFilter::new(base));

    // Log format depends on output mode: `ndjson` outputs events as JSON lines
    // to stderr; everything else gets pretty text. Always to stderr so it
    // never mixes with structured stdout output.
    let fmt_layer = fmt::layer().with_target(false).without_time().with_writer(std::io::stderr);

    let _ = tracing_subscriber::registry().with(env_filter).with(fmt_layer).try_init();

    if cli.help_json {
        return emit_help_json();
    }

    match dispatch(cli) {
        Ok(code) => code,
        Err(err) => {
            eprintln!("versionx: {err:#}");
            ExitCode::from(1)
        }
    }
}

fn dispatch(cli: Cli) -> Result<ExitCode> {
    let Some(command) = cli.command else {
        // `versionx` with no args prints a short status-ish message.
        println!("versionx {} — scaffold.", env!("CARGO_PKG_VERSION"));
        println!("See `versionx --help` for commands, or `docs/spec/` for the design.");
        return Ok(ExitCode::from(0));
    };

    match command {
        Command::Init(args) => run_init(&args, cli.cwd.as_deref(), cli.output),
        Command::Sync(args) => block_on(run_sync(args, cli.cwd.as_deref(), cli.output)),
        Command::Verify => run_verify(cli.cwd.as_deref(), cli.output),
        Command::Status => status_stub(cli.output),
        Command::Install(args) => block_on(run_install(args, cli.output)),
        Command::Which { tool } => block_on(run_which(tool, cli.cwd.as_deref(), cli.output)),
        Command::Activate(args) => run_activate(args.shell, cli.output),
        Command::Runtime(sub) => run_runtime(sub, cli.output),
        Command::Global(sub) => run_global(sub, cli.output),
        Command::Workspace(sub) => run_workspace(sub, cli.cwd.as_deref(), cli.output),
        Command::Bump => run_bump(cli.cwd.as_deref(), cli.output),
        Command::Daemon(sub) => block_on(run_daemon(sub, cli.output)),
        Command::Tui => run_tui(cli.cwd.as_deref()),
        Command::Release(sub) => run_release(sub, cli.cwd.as_deref(), cli.output),
        Command::Plan(sub) => run_plan(sub, cli.cwd.as_deref(), cli.output),
        Command::Policy(sub) => run_policy(sub, cli.cwd.as_deref(), cli.output),
        Command::Waiver(sub) => run_waiver(sub, cli.cwd.as_deref(), cli.output),
        Command::Mcp(_) => not_yet("mcp", "0.6.0"),
    }
}

/// Build a short-lived tokio runtime for commands that need async.
fn block_on<F: std::future::Future<Output = Result<ExitCode>>>(fut: F) -> Result<ExitCode> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("building tokio runtime")?;
    rt.block_on(fut)
}

/// Set up a bus + context. Returns both so the caller can keep the bus alive.
fn core_ctx() -> Result<(EventBus, CoreContext)> {
    let bus = EventBus::new();
    let ctx = CoreContext::detect(bus.sender())?;
    Ok((bus, ctx))
}

async fn run_sync(
    args: SyncArgs,
    cwd: Option<&camino::Utf8Path>,
    output: OutputFormat,
) -> Result<ExitCode> {
    let root = resolve_cwd(cwd)?;
    let (_bus, ctx) = core_ctx()?;
    let opts = CoreSyncOpts { root, dry_run: args.plan_only };

    let outcome = match core_cmds::sync(&ctx, &opts).await {
        Ok(o) => o,
        Err(err) => return Ok(render_core_error(&err, output, "sync")),
    };

    match output {
        OutputFormat::Json | OutputFormat::Ndjson => {
            let mut stdout = io::stdout().lock();
            serde_json::to_writer(&mut stdout, &outcome)?;
            stdout.write_all(b"\n")?;
        }
        OutputFormat::Human => {
            println!("Synced — lockfile at {}", outcome.lockfile_path);
            for rt in &outcome.installed {
                let state = if rt.already_installed { "cached" } else { "installed" };
                println!("  {state} {} {} ({})", rt.tool, rt.version, rt.source);
            }
            if !outcome.shims.is_empty() {
                println!("  Shims: {}", outcome.shims.join(", "));
            }
            for skip in &outcome.skipped {
                eprintln!("  skipped {skip}");
            }
        }
    }
    Ok(ExitCode::from(0))
}

async fn run_install(args: InstallArgs, output: OutputFormat) -> Result<ExitCode> {
    let (_bus, ctx) = core_ctx()?;
    let opts = CoreInstallOpts {
        tool: args.tool.clone(),
        version: args.version.clone(),
        skip_shims: false,
    };

    let outcome = match core_cmds::install(&ctx, &opts).await {
        Ok(o) => o,
        Err(err) => return Ok(render_core_error(&err, output, "install")),
    };

    match output {
        OutputFormat::Json | OutputFormat::Ndjson => {
            let mut stdout = io::stdout().lock();
            serde_json::to_writer(&mut stdout, &outcome)?;
            stdout.write_all(b"\n")?;
        }
        OutputFormat::Human => {
            let state = if outcome.already_installed { "already installed" } else { "installed" };
            println!("{state} {} {} ({})", outcome.tool, outcome.resolved_version, outcome.source);
            println!("  path: {}", outcome.install_path);
            if let Some(sha) = &outcome.sha256 {
                println!("  sha256: {sha}");
            }
            if !outcome.shims.is_empty() {
                println!("  shims: {}", outcome.shims.join(", "));
            }
        }
    }
    Ok(ExitCode::from(0))
}

async fn run_which(
    tool: String,
    cwd: Option<&camino::Utf8Path>,
    output: OutputFormat,
) -> Result<ExitCode> {
    let root = resolve_cwd(cwd)?;
    let (_bus, ctx) = core_ctx()?;
    let opts = CoreWhichOpts { tool, cwd: root };

    let outcome = match core_cmds::which(&ctx, &opts).await {
        Ok(o) => o,
        Err(err) => return Ok(render_core_error(&err, output, "which")),
    };

    match output {
        OutputFormat::Json | OutputFormat::Ndjson => {
            let mut stdout = io::stdout().lock();
            serde_json::to_writer(&mut stdout, &outcome)?;
            stdout.write_all(b"\n")?;
        }
        OutputFormat::Human => {
            if let Some(bin) = &outcome.binary {
                println!("{bin}");
                println!("  version: {}", outcome.resolved_version.clone().unwrap_or_default());
                println!("  reason: {}", outcome.reason);
            } else if let Some(v) = outcome.resolved_version {
                println!("version: {v}");
                println!("reason: {}", outcome.reason);
            } else {
                println!("unresolved: {}", outcome.reason);
                return Ok(ExitCode::from(1));
            }
        }
    }
    Ok(ExitCode::from(0))
}

fn run_verify(cwd: Option<&camino::Utf8Path>, output: OutputFormat) -> Result<ExitCode> {
    use versionx_core::commands::verify as verify_mod;

    let root = resolve_cwd(cwd)?;
    let (_bus, ctx) = core_ctx()?;
    let opts = verify_mod::VerifyOptions { root, deep: false };

    let outcome = match verify_mod::verify(&ctx, &opts) {
        Ok(o) => o,
        Err(err) => return Ok(render_core_error(&err, output, "verify")),
    };

    let ok = outcome.config_hash_ok && outcome.problems.is_empty();

    match output {
        OutputFormat::Json | OutputFormat::Ndjson => {
            let mut stdout = io::stdout().lock();
            serde_json::to_writer(&mut stdout, &outcome)?;
            stdout.write_all(b"\n")?;
        }
        OutputFormat::Human => {
            if ok {
                println!("✓ lockfile matches config + every runtime is installed");
                for rt in &outcome.checked {
                    let path_str = rt
                        .install_path
                        .as_ref()
                        .map_or_else(|| "<unknown>".to_string(), ToString::to_string);
                    println!("  {} {} at {}", rt.tool, rt.version, path_str);
                }
            } else {
                eprintln!("✗ verify found {} problem(s):", outcome.problems.len());
                for p in &outcome.problems {
                    eprintln!("  - {p:?}");
                }
            }
        }
    }
    Ok(ExitCode::from(u8::from(!ok)))
}

fn run_runtime(sub: RuntimeCommand, output: OutputFormat) -> Result<ExitCode> {
    use versionx_core::commands::runtime as rt;

    let (_bus, ctx) = core_ctx()?;
    match sub {
        RuntimeCommand::List => {
            let all = match rt::list(&ctx) {
                Ok(v) => v,
                Err(err) => return Ok(render_core_error(&err, output, "runtime list")),
            };
            match output {
                OutputFormat::Json | OutputFormat::Ndjson => {
                    let mut stdout = io::stdout().lock();
                    serde_json::to_writer(&mut stdout, &all)?;
                    stdout.write_all(b"\n")?;
                }
                OutputFormat::Human => {
                    if all.is_empty() {
                        println!("no runtimes installed (try `versionx install node 20`)");
                    } else {
                        for rt in &all {
                            let size_mb = rt.size_bytes / (1024 * 1024);
                            let status = if rt.on_disk { "ok " } else { "gone" };
                            println!(
                                "  [{status}] {:<7} {:<14} {} MiB   {}",
                                rt.tool, rt.version, size_mb, rt.install_path,
                            );
                        }
                    }
                }
            }
        }
        RuntimeCommand::Prune { older_than_days, dry_run, keep_latest } => {
            let opts = rt::PruneOptions { older_than_days, dry_run, keep_latest };
            let out = match rt::prune(&ctx, &opts) {
                Ok(v) => v,
                Err(err) => return Ok(render_core_error(&err, output, "runtime prune")),
            };
            match output {
                OutputFormat::Json | OutputFormat::Ndjson => {
                    let mut stdout = io::stdout().lock();
                    serde_json::to_writer(&mut stdout, &out)?;
                    stdout.write_all(b"\n")?;
                }
                OutputFormat::Human => {
                    let verb = if out.dry_run { "would remove" } else { "removed" };
                    println!(
                        "{verb} {} runtime(s), freed {} MiB",
                        out.removed.len(),
                        out.freed_bytes / (1024 * 1024)
                    );
                    for rt in &out.removed {
                        println!("  - {} {} ({})", rt.tool, rt.version, rt.install_path);
                    }
                    if !out.kept.is_empty() {
                        println!("kept {}:", out.kept.len());
                        for rt in &out.kept {
                            println!("  · {} {}", rt.tool, rt.version);
                        }
                    }
                }
            }
        }
    }
    Ok(ExitCode::from(0))
}

fn run_global(sub: GlobalCommand, output: OutputFormat) -> Result<ExitCode> {
    use versionx_core::commands::global as g;

    let (_bus, ctx) = core_ctx()?;
    match sub {
        GlobalCommand::Set { tool, version } => {
            let out = match g::set(&ctx, &tool, &version) {
                Ok(o) => o,
                Err(err) => return Ok(render_core_error(&err, output, "global set")),
            };
            match output {
                OutputFormat::Json | OutputFormat::Ndjson => {
                    let mut stdout = io::stdout().lock();
                    serde_json::to_writer(&mut stdout, &out)?;
                    stdout.write_all(b"\n")?;
                }
                OutputFormat::Human => {
                    if let Some(prev) = &out.previous {
                        println!("{} {} -> {} ({})", out.tool, prev, out.version, out.path);
                    } else {
                        println!("{} = {} ({})", out.tool, out.version, out.path);
                    }
                }
            }
        }
        GlobalCommand::Get { tool } => {
            let out = match g::get(&ctx, &tool) {
                Ok(o) => o,
                Err(err) => return Ok(render_core_error(&err, output, "global get")),
            };
            match output {
                OutputFormat::Json | OutputFormat::Ndjson => {
                    let mut stdout = io::stdout().lock();
                    serde_json::to_writer(&mut stdout, &out)?;
                    stdout.write_all(b"\n")?;
                }
                OutputFormat::Human => {
                    if let Some(v) = &out.version {
                        println!("{v}");
                    } else {
                        eprintln!("no global default for `{}`", out.tool);
                        return Ok(ExitCode::from(1));
                    }
                }
            }
        }
        GlobalCommand::Unset { tool } => {
            let out = match g::unset(&ctx, &tool) {
                Ok(o) => o,
                Err(err) => return Ok(render_core_error(&err, output, "global unset")),
            };
            match output {
                OutputFormat::Json | OutputFormat::Ndjson => {
                    let mut stdout = io::stdout().lock();
                    serde_json::to_writer(&mut stdout, &out)?;
                    stdout.write_all(b"\n")?;
                }
                OutputFormat::Human => {
                    if out.removed {
                        println!(
                            "unset {} (was {})",
                            out.tool,
                            out.previous.as_deref().unwrap_or("?")
                        );
                    } else {
                        println!("{} was not pinned globally", out.tool);
                    }
                }
            }
        }
        GlobalCommand::List => {
            let path = ctx.home.global_config();
            let Ok(raw) = std::fs::read_to_string(&path) else {
                match output {
                    OutputFormat::Json | OutputFormat::Ndjson => {
                        println!("{{\"path\":\"{path}\",\"runtimes\":{{}}}}");
                    }
                    OutputFormat::Human => println!("(no global config yet at {path})"),
                }
                return Ok(ExitCode::from(0));
            };
            println!("{raw}");
        }
    }
    Ok(ExitCode::from(0))
}

#[allow(clippy::too_many_lines)] // three flat subcommand arms; splitting just pushes complexity around.
fn run_workspace(
    sub: WorkspaceCommand,
    cwd: Option<&camino::Utf8Path>,
    output: OutputFormat,
) -> Result<ExitCode> {
    use versionx_core::commands::workspace as ws;

    let root = resolve_cwd(cwd)?;
    let (_bus, ctx) = core_ctx()?;

    match sub {
        WorkspaceCommand::List => {
            let outcome = match ws::list(&ctx, &ws::ListOptions { root }) {
                Ok(o) => o,
                Err(err) => return Ok(render_core_error(&err, output, "workspace list")),
            };
            match output {
                OutputFormat::Json | OutputFormat::Ndjson => {
                    let mut stdout = io::stdout().lock();
                    serde_json::to_writer(&mut stdout, &outcome)?;
                    stdout.write_all(b"\n")?;
                }
                OutputFormat::Human => {
                    if outcome.components.is_empty() {
                        println!("no components discovered at {}", outcome.workspace_root);
                    } else {
                        println!("workspace root: {}", outcome.workspace_root);
                        for c in &outcome.components {
                            let v = c.version.as_deref().unwrap_or("-");
                            println!("  {:<8} {:<30} {:<10} {}", c.kind, c.id, v, c.root);
                            if !c.depends_on.is_empty() {
                                println!("      depends_on: {}", c.depends_on.join(", "));
                            }
                        }
                    }
                }
            }
        }
        WorkspaceCommand::Status => {
            let outcome = match ws::status(&ctx, &ws::StatusOptions { root }) {
                Ok(o) => o,
                Err(err) => return Ok(render_core_error(&err, output, "workspace status")),
            };
            match output {
                OutputFormat::Json | OutputFormat::Ndjson => {
                    let mut stdout = io::stdout().lock();
                    serde_json::to_writer(&mut stdout, &outcome)?;
                    stdout.write_all(b"\n")?;
                }
                OutputFormat::Human => {
                    println!("workspace root: {}", outcome.workspace_root);
                    for c in &outcome.components {
                        let marker = if c.dirty { "M" } else { " " };
                        let v = c.version.as_deref().unwrap_or("-");
                        println!(
                            "  [{marker}] {:<8} {:<30} {:<10} {}",
                            c.kind,
                            c.id,
                            v,
                            &c.current_hash[..16.min(c.current_hash.len())]
                        );
                        if c.dirty && !c.cascade.is_empty() {
                            println!("      cascade: {}", c.cascade.join(", "));
                        }
                    }
                    if outcome.any_dirty {
                        println!(
                            "\n{} component(s) modified \u{2014} run `versionx release propose` (0.4) to bump.",
                            outcome.components.iter().filter(|c| c.dirty).count()
                        );
                    } else {
                        println!("\nclean \u{2014} all components match last-released hashes.");
                    }
                }
            }
        }
        WorkspaceCommand::Graph => {
            let outcome = match ws::graph(&ctx, &ws::GraphOptions { root }) {
                Ok(o) => o,
                Err(err) => return Ok(render_core_error(&err, output, "workspace graph")),
            };
            match output {
                OutputFormat::Json | OutputFormat::Ndjson => {
                    let mut stdout = io::stdout().lock();
                    serde_json::to_writer(&mut stdout, &outcome)?;
                    stdout.write_all(b"\n")?;
                }
                OutputFormat::Human => {
                    println!("workspace root: {}", outcome.workspace_root);
                    println!("nodes ({}):", outcome.nodes.len());
                    for n in &outcome.nodes {
                        println!("  - {n}");
                    }
                    if outcome.edges.is_empty() {
                        println!("\nno dependency edges.");
                    } else {
                        println!("\nedges ({}):", outcome.edges.len());
                        for e in &outcome.edges {
                            println!("  {} -> {}", e.from, e.to);
                        }
                    }
                    println!("\ntopological order (leaves first):");
                    for (i, id) in outcome.topo_order.iter().enumerate() {
                        println!("  {i:>2}. {id}");
                    }
                }
            }
        }
    }
    Ok(ExitCode::from(0))
}

fn run_bump(cwd: Option<&camino::Utf8Path>, output: OutputFormat) -> Result<ExitCode> {
    use versionx_core::commands::bump;

    let root = resolve_cwd(cwd)?;
    let last_hashes = versionx_core::commands::workspace::load_last_hashes(&root);
    let opts = bump::BumpOptions { root, last_hashes, groups: Vec::new() };
    let outcome = match bump::propose(&opts) {
        Ok(o) => o,
        Err(err) => return Ok(render_core_error(&err, output, "bump")),
    };

    match output {
        OutputFormat::Json | OutputFormat::Ndjson => {
            let mut stdout = io::stdout().lock();
            serde_json::to_writer(&mut stdout, &outcome)?;
            stdout.write_all(b"\n")?;
        }
        OutputFormat::Human => {
            println!("workspace root: {}", outcome.workspace_root);
            if outcome.clean {
                println!("no changes detected — nothing to bump.");
            } else {
                println!("proposed bumps ({}):", outcome.plan.len());
                for p in &outcome.plan {
                    let from = p.from.as_deref().unwrap_or("—");
                    let reason = match &p.reason {
                        bump::BumpReason::DirectChange => "direct change".to_string(),
                        bump::BumpReason::Cascaded { from: srcs } => {
                            format!("cascaded from {}", srcs.join(", "))
                        }
                        bump::BumpReason::GroupLockstep { group, via } => {
                            format!("lockstep group {group} via {via}")
                        }
                    };
                    println!("  {:<32} {from} -> {:<10} [{:?}] ({reason})", p.id, p.to, p.level);
                }
            }
        }
    }
    Ok(ExitCode::from(0))
}

async fn run_daemon(sub: DaemonCommand, output: OutputFormat) -> Result<ExitCode> {
    use versionx_daemon::{Client, DaemonPaths, client::is_running};
    let paths = DaemonPaths::from_env().context("resolving VERSIONX_HOME")?;

    match sub {
        DaemonCommand::Start { foreground } => {
            if is_running(&paths).await {
                emit_msg(output, "daemon already running", serde_json::json!({"running": true}))?;
                return Ok(ExitCode::from(0));
            }
            // Locate the versiond binary the same way we locate
            // versionx-tui — prefer a sibling of the current exe.
            let versiond = sibling_binary("versiond")?;
            let mut cmd = std::process::Command::new(versiond);
            if !foreground {
                cmd.arg("--detach");
            }
            let status = cmd.status().context("spawning versiond")?;
            Ok(ExitCode::from(status.code().unwrap_or(1) as u8))
        }
        DaemonCommand::Stop => {
            if let Ok(client) = Client::connect(&paths).await {
                client.shutdown().await.context("sending shutdown")?;
                emit_msg(output, "daemon shutting down", serde_json::json!({"shutdown": true}))?;
            } else {
                emit_msg(output, "daemon not running", serde_json::json!({"running": false}))?;
            }
            Ok(ExitCode::from(0))
        }
        DaemonCommand::Status => {
            if !is_running(&paths).await {
                emit_msg(output, "daemon not running", serde_json::json!({"running": false}))?;
                return Ok(ExitCode::from(1));
            }
            let client = Client::connect(&paths).await.context("connecting to daemon")?;
            let info = client.server_info().await.context("fetching server info")?;
            match output {
                OutputFormat::Json | OutputFormat::Ndjson => {
                    let payload = serde_json::json!({
                        "running": true,
                        "pid": info.pid,
                        "uptime_seconds": info.uptime_seconds,
                        "version": info.version,
                        "socket": paths.socket.to_string(),
                    });
                    let mut stdout = io::stdout().lock();
                    serde_json::to_writer(&mut stdout, &payload)?;
                    stdout.write_all(b"\n")?;
                }
                OutputFormat::Human => {
                    println!("daemon running");
                    println!("  pid:     {}", info.pid);
                    println!("  uptime:  {}s", info.uptime_seconds);
                    println!("  version: {}", info.version);
                    println!("  socket:  {}", paths.socket);
                }
            }
            Ok(ExitCode::from(0))
        }
        DaemonCommand::Logs { tail, no_follow } => {
            tail_log(&paths.log_file, tail, !no_follow).await?;
            Ok(ExitCode::from(0))
        }
    }
}

/// Find a sibling binary next to the current executable. Falls back to
/// PATH lookup if not colocated (e.g. in `/usr/local/bin`).
fn sibling_binary(name: &str) -> Result<std::path::PathBuf> {
    let exe = std::env::current_exe().context("locating current exe")?;
    let sibling = exe
        .parent()
        .map(|p| p.join(if cfg!(windows) { format!("{name}.exe") } else { name.into() }));
    if let Some(p) = sibling
        && p.exists()
    {
        return Ok(p);
    }
    which::which(name).with_context(|| format!("cannot find `{name}` on PATH"))
}

fn emit_msg(output: OutputFormat, human: &str, json_payload: serde_json::Value) -> Result<()> {
    match output {
        OutputFormat::Json | OutputFormat::Ndjson => {
            let mut stdout = io::stdout().lock();
            serde_json::to_writer(&mut stdout, &json_payload)?;
            stdout.write_all(b"\n")?;
        }
        OutputFormat::Human => {
            println!("{human}");
        }
    }
    Ok(())
}

async fn tail_log(path: &camino::Utf8Path, tail: usize, follow: bool) -> Result<()> {
    use tokio::io::{AsyncBufReadExt, AsyncSeekExt, BufReader};

    if !path.exists() {
        eprintln!("no log file at {path} — has the daemon ever run?");
        return Ok(());
    }

    // Read the last `tail` lines by slurping the whole file — it's capped
    // at a few MiB for typical daemon runs, and we avoid reverse-seek
    // complexity.
    let contents = tokio::fs::read_to_string(path.as_std_path()).await?;
    let lines: Vec<&str> = contents.lines().collect();
    let start = lines.len().saturating_sub(tail);
    for line in &lines[start..] {
        println!("{line}");
    }

    if !follow {
        return Ok(());
    }

    // Simple poll-tail: re-open the file periodically from our current
    // offset and dump any new bytes. Good enough for 0.3 — users who need
    // fancier tailing can `tail -f` directly.
    let mut offset = tokio::fs::metadata(path.as_std_path()).await?.len();
    let mut interval = tokio::time::interval(std::time::Duration::from_millis(250));
    loop {
        interval.tick().await;
        let meta = tokio::fs::metadata(path.as_std_path()).await?;
        let len = meta.len();
        if len < offset {
            // File got truncated/rotated — start over from the top.
            offset = 0;
        }
        if len == offset {
            continue;
        }
        let mut file = tokio::fs::File::open(path.as_std_path()).await?;
        file.seek(std::io::SeekFrom::Start(offset)).await?;
        let reader = BufReader::new(file);
        let mut lines = reader.lines();
        while let Some(line) = lines.next_line().await? {
            println!("{line}");
        }
        offset = len;
    }
}

fn run_release(
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
    }
}

fn run_plan(
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

fn bail_with(output: OutputFormat, op: &str, message: &str) -> Result<ExitCode> {
    match output {
        OutputFormat::Json | OutputFormat::Ndjson => {
            let payload = serde_json::json!({"op": op, "error": message});
            let mut stdout = io::stdout().lock();
            serde_json::to_writer(&mut stdout, &payload)?;
            stdout.write_all(b"\n")?;
        }
        OutputFormat::Human => {
            eprintln!("versionx {op}: {message}");
        }
    }
    Ok(ExitCode::from(1))
}

/// Collect commit messages since the most recent release tag (if any).
/// Falls back to the last N commits for first releases / orphan refs.
fn collect_commit_messages(root: &camino::Utf8Path) -> Vec<String> {
    // git2 wouldn't give us cleaner output than shelling out here, and
    // shelling out keeps the CLI binary smaller. We format with
    // `--pretty=%B` to get full commit bodies, separated by a NUL byte.
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

#[allow(clippy::too_many_lines)] // dispatcher over 7 subcommands; splitting relocates size
fn run_policy(
    sub: PolicyCommand,
    cwd: Option<&camino::Utf8Path>,
    output: OutputFormat,
) -> Result<ExitCode> {
    use versionx_policy as pol;
    let root = resolve_cwd(cwd)?;
    let dir = pol::default_policies_dir(&root);

    match sub {
        PolicyCommand::Init => {
            std::fs::create_dir_all(dir.as_std_path()).context("creating policies dir")?;
            let target = dir.join("main.toml");
            if target.is_file() {
                emit_msg(
                    output,
                    "policy file already exists",
                    serde_json::json!({"path": target.to_string()}),
                )?;
                return Ok(ExitCode::from(0));
            }
            let starter = r#"# Versionx policies — edit to fit your org's rules.
# See docs/spec/07-policy-engine.md for the full catalog.

[[policy]]
name = "conventional-commits"
kind = "commit_format"
severity = "warn"
style = "conventional"
"#;
            std::fs::write(target.as_std_path(), starter).context("writing starter policy")?;
            emit_msg(
                output,
                &format!("wrote {target}"),
                serde_json::json!({"created": target.to_string()}),
            )?;
            Ok(ExitCode::from(0))
        }
        PolicyCommand::Check => {
            let docs = pol::load_dir(&dir, &[]).context("loading policies")?;
            let set = pol::PolicySet::from_documents(&docs).context("merging policies")?;
            let ctx = build_policy_context(&root)?;
            let report = pol::evaluate(&set, &ctx).context("evaluating policies")?;
            emit_policy_report(&report, output)
        }
        PolicyCommand::Explain { name } => {
            let docs = pol::load_dir(&dir, &[]).context("loading policies")?;
            for d in &docs {
                for p in &d.document.policies {
                    if p.name == name {
                        match output {
                            OutputFormat::Json | OutputFormat::Ndjson => {
                                let mut stdout = io::stdout().lock();
                                serde_json::to_writer(&mut stdout, p)?;
                                stdout.write_all(b"\n")?;
                            }
                            OutputFormat::Human => {
                                println!("policy: {}", p.name);
                                println!("kind:   {:?}", p.kind);
                                println!("severity: {:?}", p.severity);
                                println!("sealed: {}", p.sealed);
                                println!("source: {}", d.path);
                                if let Some(msg) = &p.message {
                                    println!("message: {msg}");
                                }
                            }
                        }
                        return Ok(ExitCode::from(0));
                    }
                }
            }
            bail_with(output, "policy explain", &format!("policy `{name}` not found"))
        }
        PolicyCommand::List => {
            let docs = pol::load_dir(&dir, &[]).context("loading policies")?;
            match output {
                OutputFormat::Json | OutputFormat::Ndjson => {
                    let payload: Vec<_> = docs
                        .iter()
                        .flat_map(|d| {
                            d.document.policies.iter().map(move |p| {
                                serde_json::json!({
                                    "name": p.name,
                                    "kind": p.kind,
                                    "severity": p.severity,
                                    "sealed": p.sealed,
                                    "source": d.path.to_string(),
                                })
                            })
                        })
                        .collect();
                    let mut stdout = io::stdout().lock();
                    serde_json::to_writer(&mut stdout, &payload)?;
                    stdout.write_all(b"\n")?;
                }
                OutputFormat::Human => {
                    for d in &docs {
                        for p in &d.document.policies {
                            let seal = if p.sealed { "🔒" } else { "·" };
                            println!(
                                "  {seal} {:<32} {:<22} [{:?}]",
                                p.name,
                                format!("{:?}", p.kind),
                                p.severity
                            );
                        }
                    }
                }
            }
            Ok(ExitCode::from(0))
        }
        PolicyCommand::Stats => {
            let docs = pol::load_dir(&dir, &[]).context("loading policies")?;
            let set = pol::PolicySet::from_documents(&docs).context("merging policies")?;
            let ctx = build_policy_context(&root)?;
            let report = pol::evaluate(&set, &ctx).context("evaluating policies")?;
            let tally = report.tally();
            match output {
                OutputFormat::Json | OutputFormat::Ndjson => {
                    let mut stdout = io::stdout().lock();
                    serde_json::to_writer(&mut stdout, &tally)?;
                    stdout.write_all(b"\n")?;
                }
                OutputFormat::Human => {
                    println!("deny:   {}", tally.deny);
                    println!("warn:   {}", tally.warn);
                    println!("info:   {}", tally.info);
                    println!("waived: {}", tally.waived);
                }
            }
            Ok(ExitCode::from(0))
        }
        PolicyCommand::Update => {
            let docs = pol::load_dir(&dir, &[]).context("loading policies")?;
            let mut lf = pol::PolicyLockfile::new();
            for d in &docs {
                let hash = pol::hash_source(&d.path).context("hashing policy source")?;
                let sealed_names: Vec<String> = d
                    .document
                    .policies
                    .iter()
                    .filter(|p| p.sealed)
                    .map(|p| p.name.clone())
                    .collect();
                lf.sources.push(pol::LockedSource {
                    path: d.path.to_string(),
                    blake3: hash,
                    sealed: sealed_names,
                });
            }
            let out_path = root.join("versionx.policy.lock");
            lf.save(&out_path).context("writing policy lockfile")?;
            emit_msg(
                output,
                &format!("wrote {out_path}"),
                serde_json::json!({"lockfile": out_path.to_string(), "sources": lf.sources.len()}),
            )?;
            Ok(ExitCode::from(0))
        }
        PolicyCommand::Audit => {
            let docs = pol::load_dir(&dir, &[]).context("loading policies")?;
            let set = pol::PolicySet::from_documents(&docs).context("merging policies")?;
            emit_waiver_audit(&pol::audit_waivers(&set.waivers, chrono::Utc::now()), output)
        }
    }
}

fn run_waiver(
    sub: WaiverCommand,
    cwd: Option<&camino::Utf8Path>,
    output: OutputFormat,
) -> Result<ExitCode> {
    use versionx_policy as pol;
    let root = resolve_cwd(cwd)?;
    let dir = pol::default_policies_dir(&root);

    match sub {
        WaiverCommand::List => {
            let docs = pol::load_dir(&dir, &[]).context("loading policies")?;
            let now = chrono::Utc::now();
            match output {
                OutputFormat::Json | OutputFormat::Ndjson => {
                    let payload: Vec<_> = docs
                        .iter()
                        .flat_map(|d| d.document.waivers.iter())
                        .map(|w| {
                            serde_json::json!({
                                "policy": w.policy,
                                "reason": w.reason,
                                "expires_at": w.expires_at,
                                "owner": w.owner,
                                "days_until_expiry": w.days_until_expiry(now),
                            })
                        })
                        .collect();
                    let mut stdout = io::stdout().lock();
                    serde_json::to_writer(&mut stdout, &payload)?;
                    stdout.write_all(b"\n")?;
                }
                OutputFormat::Human => {
                    for d in &docs {
                        for w in &d.document.waivers {
                            let days = w.days_until_expiry(now);
                            let marker = if days < 0 {
                                "✗"
                            } else if days <= 7 {
                                "!"
                            } else {
                                "·"
                            };
                            println!(
                                "  {marker} {:<24} {:>4}d remaining — {}",
                                w.policy, days, w.reason
                            );
                        }
                    }
                }
            }
            Ok(ExitCode::from(0))
        }
        WaiverCommand::Audit => {
            let docs = pol::load_dir(&dir, &[]).context("loading policies")?;
            let set = pol::PolicySet::from_documents(&docs).context("merging policies")?;
            emit_waiver_audit(&pol::audit_waivers(&set.waivers, chrono::Utc::now()), output)
        }
        WaiverCommand::Expire { path } => {
            let file = path.unwrap_or_else(|| dir.join("local.toml"));
            if !file.is_file() {
                return bail_with(output, "waiver expire", &format!("{file} not found"));
            }
            let raw = std::fs::read_to_string(file.as_std_path()).context("reading waivers")?;
            let mut doc = versionx_policy::parse_policy_toml(&raw).context("parsing waivers")?;
            let before = doc.waivers.len();
            let now = chrono::Utc::now();
            doc.waivers.retain(|w| w.is_live(now));
            let removed = before - doc.waivers.len();
            let rendered =
                versionx_policy::render_policy_toml(&doc).context("rendering waivers")?;
            std::fs::write(file.as_std_path(), rendered).context("writing waivers")?;
            emit_msg(
                output,
                &format!("expired {removed} waivers"),
                serde_json::json!({"removed": removed, "path": file.to_string()}),
            )?;
            Ok(ExitCode::from(0))
        }
    }
}

/// Build a [`PolicyContext`] from workspace discovery + lockfile hash
/// state. Populates components + runtimes; leaves link/advisory
/// channels empty (they're filled by higher-level subsystems — 0.5
/// just needs the plumbing).
fn build_policy_context(root: &camino::Utf8Path) -> Result<versionx_policy::PolicyContext> {
    use versionx_policy::{ContextComponent, ContextRuntime, PolicyContext};
    use versionx_workspace::discovery;

    let ws = discovery::discover(root).context("discovering workspace")?;
    let mut ctx = PolicyContext::new(root.to_path_buf());
    for c in ws.components.values() {
        let mut deps = std::collections::BTreeMap::new();
        for d in &c.depends_on {
            deps.insert(d.to_string(), "workspace:*".into());
        }
        ctx.components.insert(
            c.id.to_string(),
            ContextComponent {
                id: c.id.to_string(),
                kind: c.kind.as_str().to_string(),
                root: c.root.clone(),
                version: c.version.as_ref().map(ToString::to_string),
                dependencies: deps,
                tags: Vec::new(),
            },
        );
    }

    // Runtimes — read pins straight from `versionx.toml` so
    // `runtime_version` rules can fire without depending on the full
    // core loader.
    if let Some(pins) = read_runtime_pins(root) {
        for (name, version) in pins {
            ctx.runtimes.insert(name.clone(), ContextRuntime { name, version });
        }
    }
    Ok(ctx)
}

/// Pull `[runtimes]` pins out of `versionx.toml`. Returns `None` if the
/// file is missing / malformed.
fn read_runtime_pins(root: &camino::Utf8Path) -> Option<Vec<(String, String)>> {
    let cfg_path = root.join("versionx.toml");
    let raw = std::fs::read_to_string(cfg_path.as_std_path()).ok()?;
    let doc: toml::Value = toml::from_str(&raw).ok()?;
    let tbl = doc.get("runtimes")?.as_table()?;
    let mut out = Vec::with_capacity(tbl.len());
    for (name, val) in tbl {
        let version = match val {
            toml::Value::String(s) => s.clone(),
            toml::Value::Table(t) => {
                t.get("version").and_then(|v| v.as_str()).unwrap_or("").to_string()
            }
            _ => continue,
        };
        if !version.is_empty() {
            out.push((name.clone(), version));
        }
    }
    Some(out)
}

fn emit_policy_report(
    report: &versionx_policy::PolicyReport,
    output: OutputFormat,
) -> Result<ExitCode> {
    match output {
        OutputFormat::Json | OutputFormat::Ndjson => {
            let mut stdout = io::stdout().lock();
            serde_json::to_writer(&mut stdout, report)?;
            stdout.write_all(b"\n")?;
        }
        OutputFormat::Human => {
            if report.findings.is_empty() {
                println!("all policies pass.");
            } else {
                for f in &report.findings {
                    let waived = if f.waiver.is_some() { " (waived)" } else { "" };
                    println!(
                        "  [{:?}] {:<24} {:<40}{waived}",
                        f.finding.severity, f.finding.policy, f.finding.message
                    );
                }
                let t = report.tally();
                println!(
                    "\ndeny: {} warn: {} info: {} waived: {}",
                    t.deny, t.warn, t.info, t.waived
                );
            }
        }
    }
    if report.has_blocking() { Ok(ExitCode::from(1)) } else { Ok(ExitCode::from(0)) }
}

fn emit_waiver_audit(
    audit: &versionx_policy::WaiverAudit,
    output: OutputFormat,
) -> Result<ExitCode> {
    match output {
        OutputFormat::Json | OutputFormat::Ndjson => {
            let payload = serde_json::json!({
                "live": audit.live,
                "expiring_soon": audit.expiring_soon,
                "expired": audit.expired,
            });
            let mut stdout = io::stdout().lock();
            serde_json::to_writer(&mut stdout, &payload)?;
            stdout.write_all(b"\n")?;
        }
        OutputFormat::Human => {
            if !audit.expired.is_empty() {
                println!("expired:");
                for e in &audit.expired {
                    println!("  {e}");
                }
            }
            if !audit.expiring_soon.is_empty() {
                println!("expiring soon:");
                for e in &audit.expiring_soon {
                    println!("  {e}");
                }
            }
            if !audit.live.is_empty() {
                println!("live waivers: {}", audit.live.len());
            }
        }
    }
    Ok(ExitCode::from(0))
}

fn run_tui(cwd: Option<&camino::Utf8Path>) -> Result<ExitCode> {
    // The TUI ships as a sibling binary (`versionx-tui`). We launch it in
    // the same working directory so it can discover the workspace.
    // Locating the sibling: first try PATH (normal install), then fall
    // back to the sibling next to `versionx` (cargo dev builds).
    let target = std::env::current_exe().context("locating current exe")?;
    let sibling = target
        .parent()
        .map(|p| p.join(if cfg!(windows) { "versionx-tui.exe" } else { "versionx-tui" }));
    let mut cmd = match &sibling {
        Some(p) if p.exists() => std::process::Command::new(p),
        _ => std::process::Command::new("versionx-tui"),
    };
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    let status = cmd.status().context("spawning versionx-tui")?;
    Ok(status.code().map_or_else(|| ExitCode::from(1), |c| ExitCode::from(c as u8)))
}

fn run_activate(shell: Shell, output: OutputFormat) -> Result<ExitCode> {
    let (_bus, ctx) = core_ctx()?;
    let core_shell = match shell {
        Shell::Bash => CoreShell::Bash,
        Shell::Zsh => CoreShell::Zsh,
        Shell::Fish => CoreShell::Fish,
        Shell::Pwsh => CoreShell::Pwsh,
    };
    let snippet = core_cmds::activate(&ctx, &CoreActivate { shell: core_shell })?;

    match output {
        OutputFormat::Json | OutputFormat::Ndjson => {
            let payload = serde_json::json!({ "shell": format!("{shell:?}"), "snippet": snippet });
            let mut stdout = io::stdout().lock();
            serde_json::to_writer(&mut stdout, &payload)?;
            stdout.write_all(b"\n")?;
        }
        OutputFormat::Human => {
            print!("{snippet}");
        }
    }
    Ok(ExitCode::from(0))
}

fn render_core_error(err: &versionx_core::CoreError, output: OutputFormat, cmd: &str) -> ExitCode {
    use versionx_core::CoreError as E;
    let (code, kind): (u8, &str) = match err {
        E::ConfigAlreadyExists { .. } | E::NoConfig { .. } => (1, "user_error"),
        E::Config(_) => (2, "config"),
        E::NoEcosystemsDetected { .. } => (3, "no_ecosystems_detected"),
        E::Io { .. } | E::Serialize(_) | E::State(_) | E::Lockfile(_) | E::Paths(_) => (4, "io"),
        E::UnknownRuntime(_) | E::RuntimeNotPinned { .. } => (5, "runtime"),
        E::Installer(_) => (6, "installer"),
        E::Adapter(_) => (7, "adapter"),
    };
    match output {
        OutputFormat::Json | OutputFormat::Ndjson => {
            let payload =
                serde_json::json!({"error": format!("{err}"), "kind": kind, "exit_code": code});
            eprintln!("{payload}");
        }
        OutputFormat::Human => {
            eprintln!("versionx {cmd}: {err}");
        }
    }
    ExitCode::from(code)
}

/// Resolve the working directory for a command. Uses `--cwd` if given,
/// falling back to the process cwd. Errors if the path is not UTF-8
/// (matches our `camino`-everywhere policy).
fn resolve_cwd(flag: Option<&camino::Utf8Path>) -> Result<Utf8PathBuf> {
    if let Some(p) = flag {
        return Ok(p.to_path_buf());
    }
    let current = std::env::current_dir().context("determining current directory")?;
    Utf8PathBuf::from_path_buf(current)
        .map_err(|p| anyhow::anyhow!("current directory is not valid UTF-8: {}", p.display()))
}

fn run_init(
    args: &InitArgs,
    cwd: Option<&camino::Utf8Path>,
    output: OutputFormat,
) -> Result<ExitCode> {
    let root = resolve_cwd(cwd)?;

    // Build a temporary event bus + subscribe to stream lines to stderr
    // in ndjson mode. For 0.1.0 all subsystems run in-process (no daemon),
    // so the bus lives only for this command.
    let bus = EventBus::new();

    let opts = core_init::InitOptions { root, force: args.force, dry_run: false };

    let outcome = match core_init::init(&opts, &bus.sender()) {
        Ok(o) => o,
        Err(err) => {
            // Typed renderer so we can return sensible exit codes.
            return Ok(render_init_error(&err, output));
        }
    };

    match output {
        OutputFormat::Json | OutputFormat::Ndjson => {
            let mut stdout = io::stdout().lock();
            serde_json::to_writer(&mut stdout, &outcome)?;
            stdout.write_all(b"\n")?;
        }
        OutputFormat::Human => {
            if outcome.created {
                println!("Created {}", outcome.path);
            } else if outcome.overwrote {
                println!("Overwrote {}", outcome.path);
            }
            if outcome.ecosystems.is_empty() {
                println!("  (no ecosystems detected — configure manually)");
            } else {
                println!("  Ecosystems: {}", outcome.ecosystems.join(", "));
            }
            if !outcome.runtimes.is_empty() {
                println!("  Runtimes:");
                for (tool, version) in &outcome.runtimes {
                    println!("    {tool} = {version}");
                }
            }
            if !outcome.signals.is_empty() {
                println!("  Detected from:");
                for sig in &outcome.signals {
                    println!("    - {sig}");
                }
            }
        }
    }

    Ok(ExitCode::from(0))
}

fn render_init_error(err: &versionx_core::CoreError, output: OutputFormat) -> ExitCode {
    use versionx_core::CoreError as E;

    // Exit code map: 1 = user error, 2 = config error, 3 = no ecosystems, 4 = i/o.
    let code: u8 = match err {
        E::ConfigAlreadyExists { .. } | E::NoConfig { .. } => 1,
        E::Config(_) => 2,
        E::NoEcosystemsDetected { .. } => 3,
        E::Io { .. } | E::Serialize(_) | E::State(_) | E::Lockfile(_) | E::Paths(_) => 4,
        E::UnknownRuntime(_) | E::RuntimeNotPinned { .. } => 5,
        E::Installer(_) => 6,
        E::Adapter(_) => 7,
    };

    match output {
        OutputFormat::Json | OutputFormat::Ndjson => {
            let payload = serde_json::json!({
                "error": format!("{err}"),
                "kind": error_kind(err),
                "exit_code": code,
            });
            eprintln!("{payload}");
        }
        OutputFormat::Human => {
            eprintln!("versionx init: {err}");
        }
    }
    ExitCode::from(code)
}

const fn error_kind(err: &versionx_core::CoreError) -> &'static str {
    use versionx_core::CoreError as E;
    match err {
        E::NoConfig { .. } => "no_config",
        E::ConfigAlreadyExists { .. } => "config_already_exists",
        E::NoEcosystemsDetected { .. } => "no_ecosystems_detected",
        E::UnknownRuntime(_) => "unknown_runtime",
        E::RuntimeNotPinned { .. } => "runtime_not_pinned",
        E::Config(_) => "config",
        E::Installer(_) => "installer",
        E::Adapter(_) => "adapter",
        E::State(_) => "state",
        E::Lockfile(_) => "lockfile",
        E::Paths(_) => "paths",
        E::Io { .. } => "io",
        E::Serialize(_) => "serialize",
    }
}

fn status_stub(output: OutputFormat) -> Result<ExitCode> {
    let payload = serde_json::json!({
        "schema_version": "0",
        "versionx_version": env!("CARGO_PKG_VERSION"),
        "state": "scaffold",
        "message": "Workspace detection not yet implemented. See docs/spec/11-version-roadmap.md for 0.1.0 scope."
    });

    match output {
        OutputFormat::Json | OutputFormat::Ndjson => {
            let mut stdout = io::stdout().lock();
            serde_json::to_writer(&mut stdout, &payload)?;
            stdout.write_all(b"\n")?;
        }
        OutputFormat::Human => {
            println!("Versionx {} — scaffold", env!("CARGO_PKG_VERSION"));
            println!("Workspace detection not yet implemented.");
            println!("Tracking toward 0.1.0 per docs/spec/11-version-roadmap.md.");
        }
    }
    Ok(ExitCode::from(0))
}

// Returns Result for symmetry with sibling dispatch paths that actually fail.
#[allow(clippy::unnecessary_wraps)]
fn not_yet(cmd: &str, target: &str) -> Result<ExitCode> {
    eprintln!(
        "versionx: `{cmd}` is not yet implemented. Target release: {target}.\n\
         This binary is the 0.1.0 scaffold — see docs/spec/11-version-roadmap.md."
    );
    // Exit code 64 = EX_USAGE-ish; distinguishes "feature not present" from real errors.
    Ok(ExitCode::from(64))
}

fn emit_help_json() -> ExitCode {
    // Placeholder — a full implementation walks the clap Command tree.
    let schema = serde_json::json!({
        "schema_version": "0",
        "versionx_version": env!("CARGO_PKG_VERSION"),
        "commands": [
            "init", "sync", "verify", "status", "install", "which",
            "activate", "runtime", "daemon", "tui", "release", "policy", "mcp"
        ],
        "note": "Structured --help-json output lands in 0.1.0 per docs/spec/09-programmatic-and-ai-api.md."
    });
    println!("{}", serde_json::to_string_pretty(&schema).unwrap());
    ExitCode::from(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_parses_no_args() {
        assert!(Cli::try_parse_from(["versionx"]).is_ok());
    }

    #[test]
    fn cli_parses_all_top_level_commands() {
        let cmds = [
            vec!["versionx", "init"],
            vec!["versionx", "sync"],
            vec!["versionx", "verify"],
            vec!["versionx", "status"],
            vec!["versionx", "install", "node", "20"],
            vec!["versionx", "which", "node"],
            vec!["versionx", "activate", "bash"],
            vec!["versionx", "runtime", "list"],
            vec!["versionx", "daemon", "start"],
            vec!["versionx", "tui"],
            vec!["versionx", "release", "propose"],
            vec!["versionx", "policy", "init"],
            vec!["versionx", "mcp", "serve"],
        ];
        for argv in cmds {
            assert!(Cli::try_parse_from(&argv).is_ok(), "failed to parse: {argv:?}");
        }
    }

    #[test]
    fn cli_command_tree_is_valid() {
        // `debug_assert` in clap catches misconfigured subcommands.
        Cli::command().debug_assert();
    }
}
