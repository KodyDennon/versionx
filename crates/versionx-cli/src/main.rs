//! The `versionx` CLI entry point.
//!
//! This is the main binary users invoke. Every subcommand routes through
//! `versionx-core` — the CLI never calls git, adapters, or ecosystem tools
//! directly (see `docs/spec/01-architecture-overview.md`).
//!
//! Status: 0.1.x alpha. Commands route to implemented subsystems when shipped
//! and surface targeted gaps while the remaining roadmap lands.

#![deny(unsafe_code)]

mod import_cmd;
mod release_cmd;
mod support_cmd;
mod update_cmd;

use std::io::{self, Write};
use std::process::ExitCode;

use anyhow::{Context, Result};
use camino::Utf8PathBuf;
use clap::{Parser, Subcommand, ValueEnum};
use import_cmd::{ImportSource, run_import};
use release_cmd::{collect_commit_messages, run_plan, run_release};
use support_cmd::{run_changeset, run_doctor, run_exec, run_self_check};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};
use update_cmd::{UpdateArgs, run_update};
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

    /// Update ecosystem dependencies and refresh the recorded lock metadata.
    Update(UpdateArgs),

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

    /// Auto-install the activation hook into the user's shell rc file
    /// (~/.zshrc, ~/.bashrc, etc.). Idempotent: re-running detects the
    /// existing marker + skips.
    InstallShellHook {
        /// Force a specific shell. Defaults to `$SHELL`.
        #[arg(long, value_enum)]
        shell: Option<Shell>,
    },

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

    /// BYO-API-key LLM configuration + smoke test.
    #[command(subcommand)]
    Ai(AiCommand),

    /// Changelog drafting (voice-aware, via BYO-API-key).
    #[command(subcommand)]
    Changelog(ChangelogCommand),

    /// Fleet-level orchestration across member repositories.
    #[command(subcommand)]
    Fleet(FleetCommand),

    /// Manage external links (submodule / subtree / virtual / ref).
    #[command(subcommand)]
    Links(LinksCommand),

    /// State backup / restore / repair via refs/versionx/history.
    #[command(subcommand)]
    State(StateCommand),

    /// Diagnose the local Versionx install + workspace health.
    /// Prints a structured pass/fail per check.
    Doctor,

    /// Run a command using the workspace's pinned tool resolution.
    /// `versionx exec node app.js` invokes the pinned Node.
    Exec {
        /// Tool to launch (e.g. `node`, `python`).
        tool: String,
        /// Arguments to pass to the tool.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// Import an existing repo's tool config (mise/asdf/nvm/poetry)
    /// into a fresh `versionx.toml`.
    #[command(alias = "migrate")]
    Import {
        /// Source format. Auto-detects when omitted.
        #[arg(long, value_enum)]
        from: Option<ImportSource>,
    },

    /// One-shot self-test: doctor + verify + a no-op sync plan.
    SelfCheck,

    /// Changeset workflow (release-please-style metadata files).
    #[command(subcommand)]
    Changeset(ChangesetCommand),
}

#[derive(Subcommand, Debug)]
enum ChangesetCommand {
    /// Create a new changeset file under `.changeset/`.
    Add {
        /// Component id this change targets.
        component: String,
        /// Bump kind: major | minor | patch.
        #[arg(long, default_value = "patch")]
        level: String,
        /// One-line summary.
        #[arg(long)]
        summary: Option<String>,
    },
    /// Validate every changeset file is parseable + targets a real component.
    Check,
    /// List pending changesets in chronological order.
    List,
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
    #[command(alias = "plan")]
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
    /// Cut a snapshot release — uses a CalVer-style version derived
    /// from the current timestamp. Useful for nightly/canary cuts.
    Snapshot {
        /// Override the snapshot tag prefix (default: `snapshot`).
        #[arg(long, default_value = "snapshot")]
        prefix: String,
    },
    /// Roll back a previously-applied release by reverting its commit
    /// + deleting its tag(s). Conservative: never force-pushes.
    Rollback {
        /// Plan id whose apply you want to undo.
        plan_id: String,
    },
    /// Cut a prerelease (`-rc.N`, `-alpha.N`, `-beta.N`).
    Prerelease {
        /// Plan id (must be approved).
        plan_id: String,
        /// Prerelease channel: `rc`, `alpha`, `beta`.
        #[arg(long, default_value = "rc")]
        channel: String,
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
    /// Apply a plan — alias of `versionx release apply`.
    Apply {
        plan_id: String,
        #[arg(long)]
        allow_dirty: bool,
    },
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
    /// Verify policy sources against the policy lockfile (hash drift
    /// + sealed-name removal).
    Verify,
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
    /// Add a new waiver to the local policy file.
    Add {
        /// Policy name to waive.
        policy: String,
        /// Required: human-readable reason. Silent waivers hide rot.
        #[arg(long)]
        reason: String,
        /// ISO date for waiver expiry. Defaults to 30 days from now.
        #[arg(long)]
        expires_at: Option<String>,
        /// Optional owner / contact.
        #[arg(long)]
        owner: Option<String>,
        /// Path to waivers file (defaults to `.versionx/policies/local.toml`).
        #[arg(long, value_name = "FILE")]
        path: Option<Utf8PathBuf>,
    },
}

#[derive(Subcommand, Debug)]
enum McpCommand {
    /// Serve the MCP protocol over stdio (the default for agent integrations).
    Serve {
        /// Listen on a loopback HTTP port instead of stdio. The
        /// server binds 127.0.0.1:`PORT` and uses rmcp's
        /// streamable-http transport with default DNS-rebind
        /// protection (loopback only).
        #[arg(long, value_name = "PORT")]
        http: Option<u16>,
    },
    /// Print the list of tools, prompts, and resources the server advertises.
    Describe,
}

#[derive(Subcommand, Debug)]
enum AiCommand {
    /// Print the resolved BYO-API-key provider config.
    Configure,
    /// Roundtrip a trivial prompt through the configured provider to verify it.
    Ping,
}

#[derive(Subcommand, Debug)]
enum ChangelogCommand {
    /// Generate a voice-aware draft changelog section using the configured
    /// BYO-API-key provider.
    Draft {
        /// Next version to draft (e.g. "1.2.4").
        #[arg(long)]
        version: String,
        /// Optional comma-separated commit messages. If omitted we harvest
        /// from `git log -n 100`.
        #[arg(long)]
        commits: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum FleetCommand {
    /// Print fleet config path + a summary of members/sets.
    Status,
    /// List every declared member.
    Members,
    /// Query members by tag.
    Query {
        /// Tag to filter on.
        #[arg(long)]
        tag: String,
    },
    /// Initialize a bare-bones `versionx-fleet.toml` in the current directory.
    Init,
    /// Sync every member: shallow git fetch + report drift vs. the
    /// recorded remote.
    Sync {
        /// Comma-separated member name filter; empty = all members.
        #[arg(long)]
        only: Option<String>,
    },
    /// Release orchestration over a set.
    #[command(subcommand)]
    Release(FleetReleaseCommand),
}

#[derive(Subcommand, Debug)]
enum FleetReleaseCommand {
    /// Dry-run a release against every member of `--set` without touching disk.
    Propose {
        #[arg(long)]
        set: String,
    },
    /// Apply an approved release across the set (runs the saga).
    Apply {
        #[arg(long)]
        set: String,
        /// Rollback strategy on failure: `manual` | `auto-revert` | `yank`.
        #[arg(long, default_value = "manual")]
        rollback: String,
    },
    /// Show the fleet's release history for a set.
    Show {
        #[arg(long)]
        set: String,
    },
}

#[derive(Subcommand, Debug)]
enum LinksCommand {
    /// Sync every declared link (check out submodules / verify virtuals / …).
    Sync,
    /// Report whether each link is ahead of or behind its upstream.
    CheckUpdates,
    /// Update each link to the latest upstream tip on its track ref.
    Update,
    /// Pull where the link kind supports it (subtree / virtual).
    Pull,
    /// Push where the link kind supports it (subtree).
    Push,
}

#[derive(Subcommand, Debug)]
enum StateCommand {
    /// Record a state backup event in refs/versionx/history.
    Backup {
        /// Optional human-readable label.
        #[arg(long)]
        label: Option<String>,
    },
    /// Recover the most recent backup manifest.
    Restore,
    /// Walk the history ref and print every event (newest first).
    Repair {
        /// Max events to walk.
        #[arg(long, default_value_t = 200)]
        max: usize,
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
        return run_bare(cli.cwd.as_deref(), cli.output);
    };

    match command {
        Command::Init(args) => run_init(&args, cli.cwd.as_deref(), cli.output),
        Command::Sync(args) => block_on(run_sync(args, cli.cwd.as_deref(), cli.output)),
        Command::Update(args) => block_on(run_update(args, cli.cwd.as_deref(), cli.output)),
        Command::Verify => run_verify(cli.cwd.as_deref(), cli.output),
        Command::Status => run_status(cli.cwd.as_deref(), cli.output),
        Command::Install(args) => block_on(run_install(args, cli.output)),
        Command::Which { tool } => block_on(run_which(tool, cli.cwd.as_deref(), cli.output)),
        Command::Activate(args) => run_activate(args.shell, cli.output),
        Command::InstallShellHook { shell } => run_install_shell_hook(shell, cli.output),
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
        Command::Mcp(sub) => block_on(run_mcp(sub, cli.cwd.as_deref(), cli.output)),
        Command::Ai(sub) => block_on(run_ai(sub, cli.cwd.as_deref(), cli.output)),
        Command::Changelog(sub) => block_on(run_changelog(sub, cli.cwd.as_deref(), cli.output)),
        Command::Fleet(sub) => run_fleet(sub, cli.cwd.as_deref(), cli.output),
        Command::Links(sub) => run_links(sub, cli.cwd.as_deref(), cli.output),
        Command::State(sub) => run_state(sub, cli.cwd.as_deref(), cli.output),
        Command::Doctor => run_doctor(cli.cwd.as_deref(), cli.output),
        Command::Exec { tool, args } => {
            block_on(run_exec(tool, args, cli.cwd.as_deref(), cli.output))
        }
        Command::Import { from } => run_import(from, cli.cwd.as_deref(), cli.output),
        Command::SelfCheck => run_self_check(cli.cwd.as_deref(), cli.output),
        Command::Changeset(sub) => run_changeset(sub, cli.cwd.as_deref(), cli.output),
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
    let opts = CoreSyncOpts { root: root.clone(), dry_run: args.plan_only };

    let outcome = match core_cmds::sync(&ctx, &opts).await {
        Ok(o) => o,
        Err(err) => return Ok(render_core_error(&err, output, "sync")),
    };

    // Run policies with `Trigger::Sync` after sync completes (the
    // resolved versions are what `runtime_version` rules expect to
    // see). A blocking finding flips the exit code but doesn't roll
    // back the install — sync is idempotent, the user can fix the
    // policy or re-pin and re-run.
    let mut policy_blocked = false;
    if let Ok(set) = versionx_policy::load_and_verify(&root, &[]) {
        let pctx = build_policy_context(&root, Some(versionx_policy::Trigger::Sync))?;
        if let Ok(report) = versionx_policy::evaluate(&set, &pctx)
            && report.has_blocking()
        {
            emit_policy_report(&report, output)?;
            policy_blocked = true;
        }
    }

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
    if policy_blocked {
        return Ok(ExitCode::from(1));
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
            let set = pol::load_and_verify(&root, &[])
                .context("loading + verifying policies against lockfile")?;
            let ctx = build_policy_context(&root, Some(pol::Trigger::Check))?;
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
            let set = pol::load_and_verify(&root, &[]).context("loading policies")?;
            let ctx = build_policy_context(&root, Some(pol::Trigger::Check))?;
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
                // Store paths relative to the workspace root so the
                // lockfile is portable across machines / checkouts.
                let rel = d
                    .path
                    .strip_prefix(&root)
                    .map_or_else(|_| d.path.clone(), camino::Utf8Path::to_path_buf);
                lf.sources.push(pol::LockedSource {
                    path: rel.to_string(),
                    blake3: hash,
                    sealed: sealed_names,
                });
            }
            let out_path = pol::default_lockfile_path(&root);
            lf.save(&out_path).context("writing policy lockfile")?;
            emit_msg(
                output,
                &format!("wrote {out_path}"),
                serde_json::json!({"lockfile": out_path.to_string(), "sources": lf.sources.len()}),
            )?;
            Ok(ExitCode::from(0))
        }
        PolicyCommand::Verify => {
            // Load + run lockfile verification; surface mismatches.
            match pol::load_and_verify(&root, &[]) {
                Ok(set) => {
                    emit_msg(
                        output,
                        "policies + lockfile in sync",
                        serde_json::json!({
                            "policies": set.policies.len(),
                            "waivers": set.waivers.len(),
                        }),
                    )?;
                    Ok(ExitCode::from(0))
                }
                Err(e) => bail_with(output, "policy verify", &format!("{e}")),
            }
        }
        PolicyCommand::Audit => {
            let docs = pol::load_dir(&dir, &[]).context("loading policies")?;
            let set = pol::PolicySet::from_documents(&docs).context("merging policies")?;
            emit_waiver_audit(&pol::audit_waivers(&set.waivers, chrono::Utc::now()), output)
        }
    }
}

#[allow(clippy::too_many_lines)] // dispatcher over 4 waiver subcommands; splitting scatters logic
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
        WaiverCommand::Add { policy, reason, expires_at, owner, path } => {
            let file = path.unwrap_or_else(|| dir.join("local.toml"));
            std::fs::create_dir_all(dir.as_std_path()).context("creating policies dir")?;

            let expiry = match expires_at {
                Some(s) => chrono::DateTime::parse_from_rfc3339(&s)
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .or_else(|_| {
                        chrono::NaiveDate::parse_from_str(&s, "%Y-%m-%d").map(|d| {
                            d.and_hms_opt(0, 0, 0).unwrap().and_local_timezone(chrono::Utc).unwrap()
                        })
                    })
                    .map_err(|e| anyhow::anyhow!("invalid expires_at `{s}`: {e}"))?,
                None => chrono::Utc::now() + chrono::Duration::days(30),
            };

            // Load-or-create the local doc, append waiver, write back.
            let mut doc = if file.is_file() {
                let raw =
                    std::fs::read_to_string(file.as_std_path()).context("reading waivers file")?;
                versionx_policy::parse_policy_toml(&raw).context("parsing waivers file")?
            } else {
                versionx_policy::PolicyDocument::default()
            };
            doc.waivers.push(versionx_policy::Waiver {
                policy: policy.clone(),
                reason,
                expires_at: expiry,
                owner,
                scope: None,
            });
            let rendered =
                versionx_policy::render_policy_toml(&doc).context("rendering updated waivers")?;
            std::fs::write(file.as_std_path(), rendered).context("writing waivers file")?;
            emit_msg(
                output,
                &format!("added waiver for `{policy}` in {file}"),
                serde_json::json!({
                    "policy": policy,
                    "expires_at": expiry,
                    "path": file.to_string(),
                }),
            )?;
            Ok(ExitCode::from(0))
        }
    }
}

/// Build a [`PolicyContext`] from workspace discovery + lockfile hash
/// state. Populates components + runtimes; leaves link/advisory
/// channels empty (they're filled by higher-level subsystems — 0.5
/// just needs the plumbing).
/// Build the full policy evaluation context for a workspace.
///
/// Populates every field [`versionx_policy::PolicyContext`] exposes so
/// rules don't see partial state:
///   - `trigger`: which command spawned the evaluation (caller passes).
///   - `components`: discovered components with their declared deps.
///   - `runtimes`: pinned versions from `[runtimes]`.
///   - `commits`: messages since the last tag (or last 100 if first
///     release).
///   - `lockfile_integrity_ok`: result of `versionx verify`.
///   - `advisories`: from the lockfile (if it records them; empty
///     otherwise — populated once the resolver lands).
///   - `links`: from `[links]` in `versionx.toml`.
///   - `provenance`: empty until sigstore wiring lands.
fn build_policy_context(
    root: &camino::Utf8Path,
    trigger: Option<versionx_policy::Trigger>,
) -> Result<versionx_policy::PolicyContext> {
    use versionx_policy::{ContextComponent, ContextLink, ContextRuntime, PolicyContext};
    use versionx_workspace::discovery;

    let ws = discovery::discover(root).context("discovering workspace")?;
    let mut ctx = PolicyContext::new(root.to_path_buf());
    ctx.trigger = trigger;

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

    if let Some(pins) = read_runtime_pins(root) {
        for (name, version) in pins {
            ctx.runtimes.insert(name.clone(), ContextRuntime { name, version });
        }
    }

    // Commits since the most recent tag (or last 100 if no tags yet).
    // We reuse `collect_commit_messages` for simplicity; release-gate
    // policies typically want the last batch anyway.
    for msg in collect_commit_messages(root) {
        // Tease the SHA off the front if we ever switch to `--pretty=%H %B`.
        ctx.commits.push(versionx_policy::ContextCommit { sha: String::new(), message: msg });
    }

    // Lockfile integrity — best-effort, swallow errors so
    // `policy check` works even when no lock exists yet.
    let opts =
        versionx_core::commands::verify::VerifyOptions { root: root.to_path_buf(), deep: false };
    if let Ok((_bus, ctx_core)) = core_ctx()
        && let Ok(report) = versionx_core::commands::verify::verify(&ctx_core, &opts)
    {
        ctx.lockfile_integrity_ok = Some(report.config_hash_ok && report.problems.is_empty());
    }

    // Links — pull `[links]` out of versionx.toml so `link_freshness`
    // can look at last-update timestamps.
    if let Some(links) = read_links(root) {
        for (name, age) in links {
            ctx.links.insert(name.clone(), ContextLink { name, age_days: age });
        }
    }

    Ok(ctx)
}

/// Read `[links]` from versionx.toml. Each entry may be a string
/// (treated as a remote spec, age unknown) or a table with an
/// `age_days` field. Returns `None` on any parse failure.
fn read_links(root: &camino::Utf8Path) -> Option<Vec<(String, Option<i64>)>> {
    let raw = std::fs::read_to_string(root.join("versionx.toml").as_std_path()).ok()?;
    let doc: toml::Value = toml::from_str(&raw).ok()?;
    let tbl = doc.get("links")?.as_table()?;
    let mut out = Vec::with_capacity(tbl.len());
    for (name, val) in tbl {
        let age = match val {
            toml::Value::Table(t) => t.get("age_days").and_then(toml::Value::as_integer),
            _ => None,
        };
        out.push((name.clone(), age));
    }
    Some(out)
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

async fn run_mcp(
    sub: McpCommand,
    cwd: Option<&camino::Utf8Path>,
    output: OutputFormat,
) -> Result<ExitCode> {
    let root = resolve_cwd(cwd)?;
    match sub {
        McpCommand::Serve { http } => {
            let ctx = versionx_mcp::McpContext::new(root).context("building mcp context")?;
            let server = versionx_mcp::VersionxServer::new(ctx);
            if let Some(port) = http {
                versionx_mcp::serve_http(server, port).await.context("mcp serve_http")?;
            } else {
                let _ = output;
                versionx_mcp::serve_stdio(server).await.context("mcp serve_stdio")?;
            }
            Ok(ExitCode::from(0))
        }
        McpCommand::Describe => {
            let payload = serde_json::json!({
                "tools": versionx_mcp::tools::descriptors()
                    .iter()
                    .map(|d| serde_json::json!({
                        "name": d.name,
                        "title": d.title,
                        "description": d.description,
                        "mutating": d.mutating,
                    }))
                    .collect::<Vec<_>>(),
                "prompts": versionx_mcp::prompts::descriptors()
                    .iter()
                    .map(|p| serde_json::json!({
                        "name": p.name,
                        "description": p.description,
                    }))
                    .collect::<Vec<_>>(),
                "resources": versionx_mcp::resources::descriptors()
                    .iter()
                    .map(|r| serde_json::json!({
                        "uri": r.uri,
                        "name": r.name,
                        "description": r.description,
                        "mime_type": r.mime_type,
                    }))
                    .collect::<Vec<_>>(),
            });
            match output {
                OutputFormat::Json | OutputFormat::Ndjson => {
                    let mut stdout = io::stdout().lock();
                    serde_json::to_writer(&mut stdout, &payload)?;
                    stdout.write_all(b"\n")?;
                }
                OutputFormat::Human => {
                    println!("tools:");
                    for d in versionx_mcp::tools::descriptors() {
                        let flag = if d.mutating { "✎" } else { "·" };
                        println!("  {flag} {:<22} {}", d.name, d.description);
                    }
                    println!("\nprompts:");
                    for p in versionx_mcp::prompts::descriptors() {
                        println!("  · {:<26} {}", p.name, p.description);
                    }
                    println!("\nresources:");
                    for r in versionx_mcp::resources::descriptors() {
                        println!("  · {:<30} {}", r.uri, r.description);
                    }
                }
            }
            Ok(ExitCode::from(0))
        }
    }
}

async fn run_ai(
    sub: AiCommand,
    cwd: Option<&camino::Utf8Path>,
    output: OutputFormat,
) -> Result<ExitCode> {
    let root = resolve_cwd(cwd)?;
    match sub {
        AiCommand::Configure => {
            let cfg = versionx_mcp::ProviderConfig::from_params(&serde_json::Value::Null, &root)
                .context("reading [release.ai.byo]")?;
            let payload = serde_json::json!({
                "provider": cfg.provider_name(),
                "model": cfg.model,
                "endpoint": cfg.endpoint(),
                "api_key_env": cfg.api_key_env,
                "api_key_set": cfg.api_key().is_some(),
            });
            match output {
                OutputFormat::Json | OutputFormat::Ndjson => {
                    let mut stdout = io::stdout().lock();
                    serde_json::to_writer(&mut stdout, &payload)?;
                    stdout.write_all(b"\n")?;
                }
                OutputFormat::Human => {
                    println!("provider: {}", cfg.provider_name());
                    println!("model:    {}", cfg.model);
                    println!("endpoint: {}", cfg.endpoint());
                    println!(
                        "api key:  {}",
                        if cfg.api_key().is_some() { "set" } else { "NOT SET" }
                    );
                }
            }
            Ok(ExitCode::from(0))
        }
        AiCommand::Ping => {
            let cfg = versionx_mcp::ProviderConfig::from_params(&serde_json::Value::Null, &root)
                .context("reading [release.ai.byo]")?;
            let prompt = versionx_mcp::ai::Prompt::new(
                "You are a smoke test. Respond with a single word: pong.",
                "ping?",
            );
            match versionx_mcp::drive_provider(&cfg, &prompt).await {
                Ok(reply) => {
                    emit_msg(
                        output,
                        &format!("ok: {}", reply.trim()),
                        serde_json::json!({"ok": true, "reply": reply}),
                    )?;
                    Ok(ExitCode::from(0))
                }
                Err(e) => bail_with(output, "ai ping", &e.to_string()),
            }
        }
    }
}

async fn run_changelog(
    sub: ChangelogCommand,
    cwd: Option<&camino::Utf8Path>,
    output: OutputFormat,
) -> Result<ExitCode> {
    let root = resolve_cwd(cwd)?;
    match sub {
        ChangelogCommand::Draft { version, commits } => {
            let cfg = versionx_mcp::ProviderConfig::from_params(&serde_json::Value::Null, &root)
                .context("reading [release.ai.byo]")?;
            let commit_messages: Vec<String> = commits.map_or_else(
                || collect_commit_messages(&root),
                |s| {
                    s.split(',')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_string)
                        .collect()
                },
            );
            let draft =
                versionx_mcp::changelog::draft_section(&root, &version, &commit_messages, &cfg)
                    .await
                    .context("changelog draft")?;
            match output {
                OutputFormat::Json | OutputFormat::Ndjson => {
                    let mut stdout = io::stdout().lock();
                    serde_json::to_writer(
                        &mut stdout,
                        &serde_json::json!({"version": version, "draft": draft}),
                    )?;
                    stdout.write_all(b"\n")?;
                }
                OutputFormat::Human => {
                    println!("{draft}");
                }
            }
            Ok(ExitCode::from(0))
        }
    }
}

/// Bare `versionx` invocation — auto-detect workspace, report status,
/// suggest next steps. Designed to be the first thing a new user ever
/// runs.
#[allow(clippy::too_many_lines)]
fn run_bare(cwd: Option<&camino::Utf8Path>, output: OutputFormat) -> Result<ExitCode> {
    let root = resolve_cwd(cwd).unwrap_or_else(|_| camino::Utf8PathBuf::from("."));

    // First-run bootstrap: make sure $VERSIONX_HOME exists so subsequent
    // commands don't scream.
    let home_ok =
        versionx_core::paths::VersionxHome::detect().is_ok_and(|h| h.ensure_dirs().is_ok());

    // Gather facts without failing loud on any single missing piece.
    let has_config = root.join("versionx.toml").is_file();
    let has_lock = root.join("versionx.lock").is_file();
    let in_git = versionx_git::read::summarize(&root).is_ok();
    let workspace = versionx_workspace::discovery::discover(&root).ok();
    let component_count = workspace.as_ref().map_or(0, |w| w.components.len());

    let daemon_paths = versionx_daemon::DaemonPaths::from_env();
    // Tiny blocking probe so we can report in sync code.
    let daemon_running = daemon_paths.as_ref().is_some_and(|p| {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .ok()
            .is_some_and(|rt| rt.block_on(versionx_daemon::is_running(p)))
    });

    match output {
        OutputFormat::Json | OutputFormat::Ndjson => {
            let payload = serde_json::json!({
                "version": env!("CARGO_PKG_VERSION"),
                "workspace_root": root,
                "in_git": in_git,
                "has_config": has_config,
                "has_lockfile": has_lock,
                "components": component_count,
                "daemon_running": daemon_running,
                "home_ok": home_ok,
            });
            let mut stdout = io::stdout().lock();
            serde_json::to_writer(&mut stdout, &payload)?;
            stdout.write_all(b"\n")?;
            return Ok(ExitCode::from(0));
        }
        OutputFormat::Human => {}
    }

    println!("versionx {} · {}", env!("CARGO_PKG_VERSION"), root);

    // One-line status banner.
    let mut flags = Vec::new();
    flags.push(if in_git { "git✓" } else { "git✗" });
    flags.push(if has_config { "config✓" } else { "config✗" });
    flags.push(if has_lock { "lock✓" } else { "lock✗" });
    flags.push(if daemon_running { "daemon✓" } else { "daemon✗" });
    if component_count > 0 {
        println!("  {} · {} components discovered", flags.join(" · "), component_count);
    } else {
        println!("  {}", flags.join(" · "));
    }

    // Suggestion stack — top item first, user follows arrows.
    println!();
    let mut suggested = false;
    if !in_git {
        println!("  → not inside a git repo. versionx works best inside one.");
        suggested = true;
    } else if !has_config && component_count == 0 {
        println!("  → no manifests detected. `versionx init` to scaffold one anyway.");
        suggested = true;
    } else if !has_config {
        println!("  → run `versionx init` to synthesize a versionx.toml for this workspace.");
        suggested = true;
    }
    if has_config && !has_lock {
        println!("  → run `versionx sync` to resolve + record versions into versionx.lock.");
        suggested = true;
    }
    if has_config && has_lock && component_count > 0 {
        println!("  → run `versionx workspace status` to see what changed since last release.");
        println!("  → run `versionx bump` to preview proposed bumps.");
        suggested = true;
    }
    if !daemon_running {
        println!(
            "  → run `versionx daemon start` (or `versionx install-shell-hook`) for warm caching."
        );
        suggested = true;
    }
    if !suggested {
        println!("  everything looks good. try `versionx workspace status` or `versionx --help`.");
    }
    Ok(ExitCode::from(0))
}

#[allow(clippy::too_many_lines)] // dispatcher over 6 fleet subcommands; splitting scatters logic
fn run_fleet(
    sub: FleetCommand,
    cwd: Option<&camino::Utf8Path>,
    output: OutputFormat,
) -> Result<ExitCode> {
    use versionx_multirepo::FleetConfig;

    let root = resolve_cwd(cwd)?;
    match sub {
        FleetCommand::Init => {
            let path = root.join(versionx_multirepo::fleet::DEFAULT_FLEET_FILENAME);
            if path.is_file() {
                return bail_with(output, "fleet init", &format!("{path} already exists"));
            }
            let starter = "# versionx-fleet.toml — managed by the ops repo.\n\
                           schema_version = \"1\"\n\n\
                           # [[member]]\n\
                           # name = \"frontend\"\n\
                           # path = \"./repos/frontend\"\n\
                           # remote = \"git@github.com:acme/frontend.git\"\n\n\
                           # [[set]]\n\
                           # name = \"customer-portal\"\n\
                           # members = [\"frontend\", \"api\"]\n\
                           # release_mode = \"coordinated\"\n";
            std::fs::write(path.as_std_path(), starter).context("writing fleet starter")?;
            emit_msg(
                output,
                &format!("wrote {path}"),
                serde_json::json!({"created": path.to_string()}),
            )?;
            Ok(ExitCode::from(0))
        }
        FleetCommand::Status => {
            let path = FleetConfig::discover(&root).map_err(|e| anyhow::anyhow!("{e}"))?;
            let cfg = FleetConfig::load(&path).map_err(|e| anyhow::anyhow!("{e}"))?;
            let payload = serde_json::json!({
                "path": path.to_string(),
                "members": cfg.members.len(),
                "sets": cfg.sets.len(),
            });
            match output {
                OutputFormat::Json | OutputFormat::Ndjson => {
                    let mut stdout = io::stdout().lock();
                    serde_json::to_writer(&mut stdout, &payload)?;
                    stdout.write_all(b"\n")?;
                }
                OutputFormat::Human => {
                    println!("fleet: {path}");
                    println!("members: {}", cfg.members.len());
                    for m in &cfg.members {
                        println!("  · {:<20} {}", m.name, m.path);
                    }
                    println!("\nsets: {}", cfg.sets.len());
                    for s in &cfg.sets {
                        println!(
                            "  · {:<20} [{}] -> {}",
                            s.name,
                            s.release_mode,
                            s.members.join(", ")
                        );
                    }
                }
            }
            Ok(ExitCode::from(0))
        }
        FleetCommand::Members => {
            let path = FleetConfig::discover(&root).map_err(|e| anyhow::anyhow!("{e}"))?;
            let cfg = FleetConfig::load(&path).map_err(|e| anyhow::anyhow!("{e}"))?;
            match output {
                OutputFormat::Json | OutputFormat::Ndjson => {
                    let mut stdout = io::stdout().lock();
                    serde_json::to_writer(&mut stdout, &cfg.members)?;
                    stdout.write_all(b"\n")?;
                }
                OutputFormat::Human => {
                    for m in &cfg.members {
                        let remote = m.remote.as_deref().unwrap_or("(local)");
                        println!("  · {:<20} {:<40} {remote}", m.name, m.path);
                    }
                }
            }
            Ok(ExitCode::from(0))
        }
        FleetCommand::Query { tag } => {
            let path = FleetConfig::discover(&root).map_err(|e| anyhow::anyhow!("{e}"))?;
            let cfg = FleetConfig::load(&path).map_err(|e| anyhow::anyhow!("{e}"))?;
            let matches: Vec<_> =
                cfg.members.iter().filter(|m| m.tags.iter().any(|t| t == &tag)).collect();
            match output {
                OutputFormat::Json | OutputFormat::Ndjson => {
                    let mut stdout = io::stdout().lock();
                    serde_json::to_writer(&mut stdout, &matches)?;
                    stdout.write_all(b"\n")?;
                }
                OutputFormat::Human => {
                    for m in &matches {
                        println!("  · {:<20} {}", m.name, m.path);
                    }
                }
            }
            Ok(ExitCode::from(0))
        }
        FleetCommand::Sync { only } => {
            let path = FleetConfig::discover(&root).map_err(|e| anyhow::anyhow!("{e}"))?;
            let fleet_root = path.parent().unwrap_or(&root).to_path_buf();
            let cfg = FleetConfig::load(&path).map_err(|e| anyhow::anyhow!("{e}"))?;

            let filter: Vec<String> = only
                .as_deref()
                .map(|s| s.split(',').map(str::trim).map(str::to_string).collect())
                .unwrap_or_default();

            let mut report = Vec::new();
            for m in &cfg.members {
                if !filter.is_empty() && !filter.contains(&m.name) {
                    continue;
                }
                let member_root = fleet_root.join(&m.path);
                let summary = versionx_git::read::summarize(&member_root).map_or_else(
                    |e| {
                        serde_json::json!({
                            "name": m.name,
                            "path": member_root.to_string(),
                            "error": e.to_string(),
                        })
                    },
                    |s| {
                        serde_json::json!({
                            "name": m.name,
                            "path": member_root.to_string(),
                            "head_sha": s.head_sha,
                            "head_ref": s.head_ref,
                            "dirty": s.dirty,
                        })
                    },
                );
                report.push(summary);
            }

            match output {
                OutputFormat::Json | OutputFormat::Ndjson => {
                    let mut stdout = io::stdout().lock();
                    serde_json::to_writer(&mut stdout, &report)?;
                    stdout.write_all(b"\n")?;
                }
                OutputFormat::Human => {
                    for r in &report {
                        let name = r.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                        if let Some(err) = r.get("error").and_then(|v| v.as_str()) {
                            println!("  ✗ {name}: {err}");
                        } else {
                            let head = r.get("head_sha").and_then(|v| v.as_str()).unwrap_or("?");
                            let dirty = r
                                .get("dirty")
                                .and_then(serde_json::Value::as_bool)
                                .unwrap_or(false);
                            let mark = if dirty { "·" } else { "✓" };
                            println!("  {mark} {name:<24} {head:.8}");
                        }
                    }
                }
            }
            Ok(ExitCode::from(0))
        }
        FleetCommand::Release(sub) => run_fleet_release(sub, &root, output),
    }
}

#[allow(clippy::too_many_lines)] // saga dispatch plus inline MemberStep; splitting relocates noise
fn run_fleet_release(
    sub: FleetReleaseCommand,
    root: &camino::Utf8Path,
    output: OutputFormat,
) -> Result<ExitCode> {
    use versionx_multirepo::{FleetConfig, RollbackStrategy};

    let path = FleetConfig::discover(root).map_err(|e| anyhow::anyhow!("{e}"))?;
    let fleet_root = path.parent().unwrap_or(root).to_path_buf();
    let cfg = FleetConfig::load(&path).map_err(|e| anyhow::anyhow!("{e}"))?;

    match sub {
        FleetReleaseCommand::Propose { set } => {
            let set_cfg = cfg.set(&set).ok_or_else(|| anyhow::anyhow!("unknown set: {set}"))?;
            match output {
                OutputFormat::Json | OutputFormat::Ndjson => {
                    let mut stdout = io::stdout().lock();
                    serde_json::to_writer(&mut stdout, set_cfg)?;
                    stdout.write_all(b"\n")?;
                }
                OutputFormat::Human => {
                    println!("set: {}", set_cfg.name);
                    println!("mode: {}", set_cfg.release_mode);
                    println!("members:");
                    for m in &set_cfg.members {
                        println!("  · {m}");
                    }
                }
            }
            Ok(ExitCode::from(0))
        }
        FleetReleaseCommand::Apply { set, rollback } => {
            use versionx_multirepo::{
                MemberStep, SagaResult, TagInfo, fleet::FleetConfig as _FC, run_saga,
            };
            let _ = rollback;
            let strategy = match rollback.as_str() {
                "auto-revert" => RollbackStrategy::AutoRevert,
                "yank" => RollbackStrategy::Yank,
                _ => RollbackStrategy::ManualRescue,
            };

            // Each member step shells out to `versionx release …`
            // inside the member's checkout. The struct keeps a
            // per-member map of (member → plan_id) so `tag` knows
            // which plan to apply after `dry_run` proposed one. The
            // map is wrapped in a Mutex so the trait methods can be
            // `&self` (the saga drives steps sequentially per member,
            // but the executor type signatures still need shared
            // mutability for cross-call state).
            //
            // The `remotes` map tells `publish` which git remote to
            // push the tag to. Empty entry = local-only (no push).
            struct CliStep {
                exe: std::path::PathBuf,
                proposed_plans: std::sync::Mutex<std::collections::HashMap<String, String>>,
                remotes: std::collections::HashMap<String, Option<String>>,
            }
            impl CliStep {
                fn run(
                    &self,
                    member_root: &camino::Utf8Path,
                    args: &[&str],
                ) -> Result<serde_json::Value, versionx_multirepo::SagaError> {
                    let mut full = vec!["--cwd", member_root.as_str(), "--output", "json"];
                    full.extend_from_slice(args);
                    let out = std::process::Command::new(&self.exe).args(&full).output().map_err(
                        |e| versionx_multirepo::SagaError::StepFailed {
                            member: member_root.to_string(),
                            message: format!("spawn versionx: {e}"),
                        },
                    )?;
                    if !out.status.success() {
                        return Err(versionx_multirepo::SagaError::StepFailed {
                            member: member_root.to_string(),
                            message: format!(
                                "exit={} stderr={}",
                                out.status,
                                String::from_utf8_lossy(&out.stderr)
                            ),
                        });
                    }
                    serde_json::from_slice(&out.stdout).map_err(|e| {
                        versionx_multirepo::SagaError::StepFailed {
                            member: member_root.to_string(),
                            message: format!(
                                "non-JSON output from versionx {args:?}: {e} — raw {}",
                                String::from_utf8_lossy(&out.stdout)
                            ),
                        }
                    })
                }
            }
            impl MemberStep for CliStep {
                fn dry_run(&self, name: &str, member_root: &camino::Utf8Path) -> SagaResult<()> {
                    // Propose a release plan locally — does not commit
                    // or tag, just writes a plan file under
                    // `.versionx/plans/`. Save the plan_id so
                    // `tag` can apply it.
                    let value = self.run(member_root, &["release", "propose"])?;
                    let plan_id = value
                        .get("plan_id")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| versionx_multirepo::SagaError::StepFailed {
                            member: member_root.to_string(),
                            message: "release propose returned no plan_id".into(),
                        })?
                        .to_string();
                    self.proposed_plans
                        .lock()
                        .map_err(|e| versionx_multirepo::SagaError::StepFailed {
                            member: name.to_string(),
                            message: format!("plan map poisoned: {e}"),
                        })?
                        .insert(name.to_string(), plan_id);
                    Ok(())
                }
                fn tag(&self, name: &str, member_root: &camino::Utf8Path) -> SagaResult<TagInfo> {
                    let plan_id = self
                        .proposed_plans
                        .lock()
                        .map_err(|e| versionx_multirepo::SagaError::StepFailed {
                            member: name.to_string(),
                            message: format!("plan map poisoned: {e}"),
                        })?
                        .get(name)
                        .cloned()
                        .ok_or_else(|| versionx_multirepo::SagaError::StepFailed {
                            member: member_root.to_string(),
                            message: "no plan from dry_run; cannot tag".into(),
                        })?;
                    // Approve before apply (apply refuses unapproved plans).
                    let _ = self.run(member_root, &["release", "approve", plan_id.as_str()]);
                    let outcome = self.run(member_root, &["release", "apply", plan_id.as_str()])?;
                    let commit = outcome
                        .get("commit")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string();
                    let tag_name = outcome
                        .get("tags")
                        .and_then(|v| v.as_array())
                        .and_then(|a| a.first())
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    Ok(TagInfo { tag_name, commit_sha: commit })
                }
                fn publish(&self, name: &str, member_root: &camino::Utf8Path) -> SagaResult<()> {
                    // Two-step publish: (1) push the git tag + branch
                    // to the configured remote; (2) attempt a registry
                    // publish (npm/crates.io). Both steps are best-
                    // effort with structured errors so the saga can
                    // attribute failures.

                    // 1. Git push.
                    if let Some(Some(remote)) = self.remotes.get(name).cloned() {
                        let repo = versionx_git::open(member_root).map_err(|e| {
                            versionx_multirepo::SagaError::StepFailed {
                                member: member_root.to_string(),
                                message: format!("open repo: {e}"),
                            }
                        })?;
                        let head_ref = repo
                            .head()
                            .ok()
                            .and_then(|h| h.shorthand().map(str::to_string))
                            .unwrap_or_else(|| "HEAD".to_string());
                        let refspecs = vec![
                            format!("refs/heads/{head_ref}:refs/heads/{head_ref}"),
                            "refs/tags/*:refs/tags/*".to_string(),
                        ];
                        versionx_git::push(&repo, &remote, &refspecs).map_err(|e| {
                            versionx_multirepo::SagaError::StepFailed {
                                member: member_root.to_string(),
                                message: format!("push to {remote}: {e}"),
                            }
                        })?;
                    }

                    // 2. Registry publish. Returns Ok(None) when the
                    //    member root has no recognized package
                    //    manifest (mixed-ecosystem fleets are fine).
                    versionx_release::publish_component(member_root).map_err(|e| {
                        versionx_multirepo::SagaError::StepFailed {
                            member: member_root.to_string(),
                            message: format!("registry publish: {e}"),
                        }
                    })?;

                    Ok(())
                }
                fn rollback(
                    &self,
                    _name: &str,
                    member_root: &camino::Utf8Path,
                    tag: &TagInfo,
                ) -> SagaResult<()> {
                    // Non-destructive: create a revert commit + delete
                    // the local tag. We deliberately do *not* hard-reset
                    // — published commits should stay in history. If
                    // publish already pushed, the operator pushes the
                    // revert manually (or runs `versionx fleet release
                    // apply` again with the fix).
                    let Ok(repo) = versionx_git::open(member_root) else {
                        return Ok(());
                    };
                    if !tag.commit_sha.is_empty() {
                        let _ = versionx_git::revert_commit(&repo, &tag.commit_sha);
                    }
                    if !tag.tag_name.is_empty() {
                        let _ = versionx_git::delete_tag(&repo, &tag.tag_name);
                    }
                    Ok(())
                }
            }

            let exe = std::env::current_exe().unwrap_or_else(|_| "versionx".into());
            // Map member-name → optional remote for `publish`. Honors
            // each member's configured `remote = "..."`.
            let remotes: std::collections::HashMap<String, Option<String>> = cfg
                .members
                .iter()
                .map(|m| {
                    // Default remote is "origin" if a URL is set; if
                    // none is configured, publish stays local-only.
                    let r = m.remote.as_ref().map(|_| "origin".to_string());
                    (m.name.clone(), r)
                })
                .collect();
            let proposed_plans = std::sync::Mutex::new(std::collections::HashMap::new());
            let cli_step = std::sync::Arc::new(CliStep { exe, proposed_plans, remotes });
            // The saga API takes Box<dyn MemberStep> per member; we
            // wrap a per-member adapter that delegates to the shared
            // CliStep. Cheap — just forwards calls.
            struct StepHandle {
                inner: std::sync::Arc<CliStep>,
            }
            impl MemberStep for StepHandle {
                fn dry_run(&self, name: &str, root: &camino::Utf8Path) -> SagaResult<()> {
                    self.inner.dry_run(name, root)
                }
                fn tag(&self, name: &str, root: &camino::Utf8Path) -> SagaResult<TagInfo> {
                    self.inner.tag(name, root)
                }
                fn publish(&self, name: &str, root: &camino::Utf8Path) -> SagaResult<()> {
                    self.inner.publish(name, root)
                }
                fn rollback(
                    &self,
                    name: &str,
                    root: &camino::Utf8Path,
                    tag: &TagInfo,
                ) -> SagaResult<()> {
                    self.inner.rollback(name, root, tag)
                }
            }
            let mut steps: std::collections::BTreeMap<String, Box<dyn MemberStep>> =
                std::collections::BTreeMap::new();
            for name in &cfg.set(&set).ok_or_else(|| anyhow::anyhow!("unknown set: {set}"))?.members
            {
                steps.insert(name.clone(), Box::new(StepHandle { inner: cli_step.clone() }));
            }
            let report = run_saga(&cfg, &fleet_root, &set, &steps, strategy)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            let _ = _FC::discover; // silence
            match output {
                OutputFormat::Json | OutputFormat::Ndjson => {
                    let mut stdout = io::stdout().lock();
                    serde_json::to_writer(&mut stdout, &report)?;
                    stdout.write_all(b"\n")?;
                }
                OutputFormat::Human => {
                    println!(
                        "set={} mode={} success={}",
                        report.set, report.mode, report.succeeded
                    );
                    for m in &report.members {
                        let err = m.error.as_deref().unwrap_or("");
                        println!(
                            "  {:<20} phase={:?} tag={} rolled_back={} err={err}",
                            m.name,
                            m.phase_reached,
                            m.tag.as_ref().map_or_else(|| "—".to_string(), |t| t.tag_name.clone()),
                            m.rolled_back
                        );
                    }
                }
            }
            Ok(if report.succeeded { ExitCode::from(0) } else { ExitCode::from(1) })
        }
        FleetReleaseCommand::Show { set } => {
            let set_cfg = cfg.set(&set).ok_or_else(|| anyhow::anyhow!("unknown set: {set}"))?;
            emit_msg(
                output,
                &format!(
                    "set {}: {} members, mode={}",
                    set_cfg.name,
                    set_cfg.members.len(),
                    set_cfg.release_mode
                ),
                serde_json::to_value(set_cfg).unwrap_or(serde_json::Value::Null),
            )?;
            Ok(ExitCode::from(0))
        }
    }
}

fn run_links(
    sub: LinksCommand,
    cwd: Option<&camino::Utf8Path>,
    output: OutputFormat,
) -> Result<ExitCode> {
    use versionx_multirepo::{LinkKind, LinkSpec, handler_for};

    let root = resolve_cwd(cwd)?;
    // Read `[links]` from versionx.toml into our simpler LinkSpec shape.
    let cfg_path = root.join("versionx.toml");
    let raw = std::fs::read_to_string(cfg_path.as_std_path()).context("reading versionx.toml")?;
    let doc: toml::Value = toml::from_str(&raw).context("parsing versionx.toml")?;
    let specs: Vec<LinkSpec> = doc
        .get("links")
        .and_then(|v| v.as_table())
        .map(|t| {
            t.iter()
                .filter_map(|(name, val)| {
                    let t = val.as_table()?;
                    let kind = match t.get("type").and_then(|v| v.as_str())? {
                        "submodule" => LinkKind::Submodule,
                        "subtree" => LinkKind::Subtree,
                        "virtual" => LinkKind::Virtual,
                        "ref" => LinkKind::Ref,
                        _ => return None,
                    };
                    Some(LinkSpec {
                        name: name.clone(),
                        kind,
                        url: t.get("url").and_then(|v| v.as_str())?.to_string(),
                        path: camino::Utf8PathBuf::from(
                            t.get("path").and_then(|v| v.as_str()).unwrap_or(name),
                        ),
                        track: t
                            .get("track")
                            .and_then(|v| v.as_str())
                            .unwrap_or("main")
                            .to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    let mut out_rows = Vec::with_capacity(specs.len());
    for spec in &specs {
        let handler = handler_for(&spec.kind);
        let status = match sub {
            LinksCommand::Sync => handler.sync(&root, spec),
            LinksCommand::CheckUpdates => handler.check_updates(&root, spec),
            LinksCommand::Update => handler.update(&root, spec),
            LinksCommand::Pull => handler.pull(&root, spec),
            LinksCommand::Push => handler.push(&root, spec),
        }
        .map_err(|e| anyhow::anyhow!("{e}"))?;
        out_rows.push(status);
    }

    match output {
        OutputFormat::Json | OutputFormat::Ndjson => {
            let mut stdout = io::stdout().lock();
            serde_json::to_writer(&mut stdout, &out_rows)?;
            stdout.write_all(b"\n")?;
        }
        OutputFormat::Human => {
            for s in &out_rows {
                println!("  {:<20} up_to_date={:<5} {}", s.name, s.up_to_date, s.message);
            }
        }
    }
    Ok(ExitCode::from(0))
}

fn run_state(
    sub: StateCommand,
    cwd: Option<&camino::Utf8Path>,
    output: OutputFormat,
) -> Result<ExitCode> {
    let root = resolve_cwd(cwd)?;
    match sub {
        StateCommand::Backup { label } => {
            let manifest = versionx_multirepo::BackupManifest {
                at: chrono::Utc::now(),
                workspace_root: root.to_string(),
                label,
                state_db_path: root.join(".versionx/state.db").to_string(),
            };
            let sha = versionx_multirepo::backup(&root, manifest.clone())
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            emit_msg(
                output,
                &format!("backup recorded: {sha}"),
                serde_json::json!({"commit": sha, "manifest": manifest}),
            )?;
            Ok(ExitCode::from(0))
        }
        StateCommand::Restore => match versionx_multirepo::restore(&root) {
            Ok(m) => {
                emit_msg(
                    output,
                    &format!("last backup: {}", m.at),
                    serde_json::to_value(&m).unwrap_or(serde_json::Value::Null),
                )?;
                Ok(ExitCode::from(0))
            }
            Err(e) => bail_with(output, "state restore", &e.to_string()),
        },
        StateCommand::Repair { max } => {
            let events =
                versionx_multirepo::repair(&root, max).map_err(|e| anyhow::anyhow!("{e}"))?;
            match output {
                OutputFormat::Json | OutputFormat::Ndjson => {
                    let mut stdout = io::stdout().lock();
                    serde_json::to_writer(&mut stdout, &events)?;
                    stdout.write_all(b"\n")?;
                }
                OutputFormat::Human => {
                    for e in &events {
                        println!("  {} {} — {}", e.at.format("%Y-%m-%d %H:%M"), e.kind, e.summary);
                    }
                    println!("({} events)", events.len());
                }
            }
            Ok(ExitCode::from(0))
        }
    }
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

/// Append `eval "$(versionx activate <shell>)"` to the user's shell rc
/// file. Idempotent via a sentinel line — re-running does nothing.
fn run_install_shell_hook(shell: Option<Shell>, output: OutputFormat) -> Result<ExitCode> {
    let shell = shell.unwrap_or_else(detect_shell);
    let rc = rc_file_for(shell).context("locating shell rc file")?;
    let marker =
        "# versionx: shell activation (do not edit — managed by `versionx install-shell-hook`)";
    let snippet = match shell {
        Shell::Bash | Shell::Zsh => format!(
            "{marker}\ncommand -v versionx >/dev/null 2>&1 && eval \"$(versionx activate {})\"\n",
            shell_name_lower(shell)
        ),
        Shell::Fish => {
            format!("{marker}\nif type -q versionx\n    versionx activate fish | source\nend\n")
        }
        Shell::Pwsh => format!(
            "{marker}\nif (Get-Command versionx -ErrorAction SilentlyContinue) {{ Invoke-Expression (versionx activate pwsh | Out-String) }}\n"
        ),
    };

    let existing = std::fs::read_to_string(&rc).unwrap_or_default();
    if existing.contains(marker) {
        emit_msg(
            output,
            &format!("shell hook already installed in {}", rc.display()),
            serde_json::json!({"already_installed": true, "rc": rc.display().to_string()}),
        )?;
        return Ok(ExitCode::from(0));
    }
    // Append, ensuring a leading newline so we don't glue onto the previous line.
    let body = if existing.is_empty() || existing.ends_with('\n') {
        format!("{existing}\n{snippet}")
    } else {
        format!("{existing}\n\n{snippet}")
    };
    if let Some(parent) = rc.parent() {
        std::fs::create_dir_all(parent).context("creating rc parent")?;
    }
    std::fs::write(&rc, body).with_context(|| format!("writing {}", rc.display()))?;
    emit_msg(
        output,
        &format!("installed shell hook into {}", rc.display()),
        serde_json::json!({"installed": true, "rc": rc.display().to_string()}),
    )?;
    Ok(ExitCode::from(0))
}

fn detect_shell() -> Shell {
    let sh = std::env::var("SHELL").unwrap_or_default();
    if sh.ends_with("zsh") {
        Shell::Zsh
    } else if sh.ends_with("fish") {
        Shell::Fish
    } else if sh.contains("pwsh") || sh.contains("powershell") {
        Shell::Pwsh
    } else {
        Shell::Bash
    }
}

fn rc_file_for(shell: Shell) -> Result<std::path::PathBuf> {
    let home = directories::BaseDirs::new().context("no home directory")?.home_dir().to_path_buf();
    Ok(match shell {
        Shell::Bash => home.join(".bashrc"),
        Shell::Zsh => home.join(".zshrc"),
        Shell::Fish => home.join(".config/fish/config.fish"),
        Shell::Pwsh => home.join(".config/powershell/profile.ps1"),
    })
}

const fn shell_name_lower(shell: Shell) -> &'static str {
    match shell {
        Shell::Bash => "bash",
        Shell::Zsh => "zsh",
        Shell::Fish => "fish",
        Shell::Pwsh => "pwsh",
    }
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

/// `versionx status` — structured workspace report.
///
/// Returns real facts: git presence, config + lockfile state, component
/// count, daemon status, and per-component pinned versions. In human
/// mode the output is a tight table; in JSON / NDJSON it's a single
/// document the MCP server / agents can ingest verbatim.
fn run_status(cwd: Option<&camino::Utf8Path>, output: OutputFormat) -> Result<ExitCode> {
    let root = resolve_cwd(cwd).unwrap_or_else(|_| camino::Utf8PathBuf::from("."));

    let has_config = root.join("versionx.toml").is_file();
    let has_lock = root.join("versionx.lock").is_file();
    let in_git = versionx_git::read::summarize(&root).is_ok();
    let head_sha = versionx_git::read::summarize(&root).ok().map(|s| s.head_sha);
    let workspace = versionx_workspace::discovery::discover(&root).ok();
    let component_count = workspace.as_ref().map_or(0, |w| w.components.len());

    let daemon_paths = versionx_daemon::DaemonPaths::from_env();
    let daemon_running = daemon_paths.as_ref().is_some_and(|p| {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .ok()
            .is_some_and(|rt| rt.block_on(versionx_daemon::is_running(p)))
    });

    let runtime_pins = read_runtime_pins(&root).unwrap_or_default();

    let components_json: Vec<_> = workspace
        .as_ref()
        .map(|w| {
            w.components
                .values()
                .map(|c| {
                    serde_json::json!({
                        "id": c.id.to_string(),
                        "kind": c.kind.as_str(),
                        "root": c.root.to_string(),
                        "version": c.version.as_ref().map(ToString::to_string),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    match output {
        OutputFormat::Json | OutputFormat::Ndjson => {
            let payload = serde_json::json!({
                "schema_version": "1",
                "versionx_version": env!("CARGO_PKG_VERSION"),
                "workspace_root": root,
                "in_git": in_git,
                "head_sha": head_sha,
                "has_config": has_config,
                "has_lockfile": has_lock,
                "daemon_running": daemon_running,
                "components": components_json,
                "runtime_pins": runtime_pins
                    .iter()
                    .map(|(n, v)| serde_json::json!({"name": n, "version": v}))
                    .collect::<Vec<_>>(),
            });
            let mut stdout = io::stdout().lock();
            serde_json::to_writer(&mut stdout, &payload)?;
            stdout.write_all(b"\n")?;
        }
        OutputFormat::Human => {
            println!("versionx {} · {}", env!("CARGO_PKG_VERSION"), root);
            let mut flags = Vec::new();
            flags.push(if in_git { "git✓" } else { "git✗" });
            flags.push(if has_config { "config✓" } else { "config✗" });
            flags.push(if has_lock { "lock✓" } else { "lock✗" });
            flags.push(if daemon_running { "daemon✓" } else { "daemon✗" });
            println!("  {}", flags.join(" · "));
            if let Some(sha) = head_sha {
                let short: String = sha.chars().take(8).collect();
                println!("  head: {short}");
            }
            if !runtime_pins.is_empty() {
                println!("  runtimes:");
                for (name, version) in &runtime_pins {
                    println!("    {name} {version}");
                }
            }
            if component_count == 0 {
                println!("  components: none discovered");
            } else {
                println!("  components ({component_count}):");
                if let Some(ws) = &workspace {
                    for c in ws.components.values() {
                        let v = c.version.as_ref().map_or_else(|| "-".into(), ToString::to_string);
                        println!("    {:<28} {} ({})", c.id.to_string(), v, c.kind.as_str());
                    }
                }
            }
        }
    }
    Ok(ExitCode::from(0))
}

// Returns Result for symmetry with sibling dispatch paths that actually fail.
#[allow(clippy::unnecessary_wraps, dead_code)]
fn not_yet(cmd: &str, target: &str) -> Result<ExitCode> {
    eprintln!(
        "versionx: `{cmd}` is not yet implemented. Target release: {target}.\n\
         This binary is the 0.1.0 scaffold — see docs/spec/11-version-roadmap.md."
    );
    // Exit code 64 = EX_USAGE-ish; distinguishes "feature not present" from real errors.
    Ok(ExitCode::from(64))
}

/// Walk the clap [`Cli`] command tree and emit a JSON schema MCP /
/// AI agents can consume. Includes every subcommand, every arg, and
/// the structured help text.
fn emit_help_json() -> ExitCode {
    use clap::CommandFactory;
    let cmd = Cli::command();
    let payload = serde_json::json!({
        "schema_version": "1",
        "versionx_version": env!("CARGO_PKG_VERSION"),
        "command": describe_command(&cmd),
    });
    println!("{}", serde_json::to_string_pretty(&payload).unwrap_or_default());
    ExitCode::from(0)
}

fn describe_command(cmd: &clap::Command) -> serde_json::Value {
    let about = cmd.get_about().map(ToString::to_string);
    let long_about = cmd.get_long_about().map(ToString::to_string);
    let args: Vec<_> = cmd.get_arguments().filter(|a| !a.is_hide_set()).map(describe_arg).collect();
    let subcommands: Vec<_> =
        cmd.get_subcommands().filter(|s| !s.is_hide_set()).map(describe_command).collect();
    serde_json::json!({
        "name": cmd.get_name(),
        "about": about,
        "long_about": long_about,
        "args": args,
        "subcommands": subcommands,
    })
}

fn describe_arg(arg: &clap::Arg) -> serde_json::Value {
    let possible_values: Vec<_> =
        arg.get_possible_values().iter().map(|v| v.get_name().to_string()).collect();
    serde_json::json!({
        "id": arg.get_id().as_str(),
        "long": arg.get_long().map(ToString::to_string),
        "short": arg.get_short().map(|c| c.to_string()),
        "help": arg.get_help().map(ToString::to_string),
        "value_name": arg.get_value_names().map(|names| {
            names.iter().map(ToString::to_string).collect::<Vec<_>>()
        }),
        "required": arg.is_required_set(),
        "global": arg.is_global_set(),
        "default_values": arg
            .get_default_values()
            .iter()
            .map(|v| v.to_string_lossy().into_owned())
            .collect::<Vec<_>>(),
        "possible_values": possible_values,
    })
}

// -------- doctor / exec / import / self-check / changeset --------------

/// `versionx doctor` — structured pass/fail per check.
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
            vec!["versionx", "update", "--plan"],
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
            vec!["versionx", "migrate", "--from", "changesets"],
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
