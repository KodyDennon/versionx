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

use anyhow::Result;
use camino::Utf8PathBuf;
use clap::{Parser, Subcommand, ValueEnum};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

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

    /// Start or manage the `versiond` background daemon.
    #[command(subcommand)]
    Daemon(DaemonCommand),

    /// Launch the interactive terminal dashboard.
    Tui,

    /// Release orchestration subcommands (plan/apply, publish, snapshots).
    #[command(subcommand)]
    Release(ReleaseCommand),

    /// Policy authoring and evaluation.
    #[command(subcommand)]
    Policy(PolicyCommand),

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
    /// Remove toolchains not pinned by any known repo, older than the cutoff.
    Prune {
        /// Minimum age in days before a toolchain is eligible for prune.
        #[arg(long, default_value_t = 90)]
        older_than_days: u32,
    },
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
}

#[derive(Subcommand, Debug)]
enum ReleaseCommand {
    /// Compute a release plan without executing it.
    Propose,
    /// Approve a previously-proposed plan by id.
    Approve { plan_id: String },
    /// Apply an approved release plan.
    Apply {
        /// Plan id to apply. If omitted, the latest approved plan is used.
        plan_id: Option<String>,
        /// Push tags and release commits after applying locally.
        /// In non-TTY contexts `--push` or `--no-push` is required.
        #[arg(long)]
        push: bool,
        /// Explicitly skip pushing (the inverse of `--push`).
        #[arg(long, conflicts_with = "push")]
        no_push: bool,
    },
}

#[derive(Subcommand, Debug)]
enum PolicyCommand {
    /// Scaffold a starter policy file under `.versionx/policies/`.
    Init,
    /// Evaluate all policies against the current workspace.
    Check,
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
    // Initialize tracing early so parse errors benefit from it too.
    let env_filter = EnvFilter::try_from_env("VERSIONX_LOG")
        .unwrap_or_else(|_| EnvFilter::new("versionx=info,warn"));
    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt::layer().with_target(false).without_time())
        .init();

    let cli = Cli::parse();

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
        Command::Init(_) => not_yet("init", "0.1.0 (Phase 1 of the roadmap)"),
        Command::Sync(_) => not_yet("sync", "0.1.0"),
        Command::Verify => not_yet("verify", "0.1.0"),
        Command::Status => status_stub(cli.output),
        Command::Install(_) => not_yet("install", "0.1.0"),
        Command::Which { .. } => not_yet("which", "0.1.0"),
        Command::Activate(_) => not_yet("activate", "0.3.0"),
        Command::Runtime(_) => not_yet("runtime", "0.1.0"),
        Command::Daemon(_) => not_yet("daemon", "0.3.0"),
        Command::Tui => not_yet("tui", "0.3.0"),
        Command::Release(_) => not_yet("release", "0.4.0"),
        Command::Policy(_) => not_yet("policy", "0.5.0"),
        Command::Mcp(_) => not_yet("mcp", "0.6.0"),
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
