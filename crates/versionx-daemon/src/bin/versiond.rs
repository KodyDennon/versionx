//! `versiond` — the long-running daemon binary.
//!
//! Default: run in the foreground, log to stderr.
//! `--detach`: fork into the background + log to the rolling log file.
//! `--foreground` is a no-op kept for launchd/systemd compat.

#![deny(unsafe_code)]

use std::fs::OpenOptions;
use std::io::Write;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::Parser;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};
use versionx_daemon::{DaemonPaths, ServerConfig, run};

#[derive(Parser, Debug)]
#[command(name = "versiond", version, about = "The versionx long-running daemon.")]
struct Cli {
    /// Run in the background. On Unix this does a double-fork; on Windows
    /// we spawn ourselves detached and exit.
    #[arg(long)]
    detach: bool,

    /// Explicitly run in the foreground. No-op today — present for
    /// launchd/systemd compatibility.
    #[arg(long, conflicts_with = "detach")]
    foreground: bool,

    /// Override the idle timeout (seconds). 0 disables the watchdog.
    #[arg(long, value_name = "SECS")]
    idle_timeout: Option<u64>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let paths = DaemonPaths::from_env().context("resolving VERSIONX_HOME")?;
    paths.ensure_dirs().context("creating run/log dirs")?;

    if cli.detach {
        return detach_and_exit(&paths);
    }

    // Foreground mode: stderr logs by default, plus the log file so
    // `versionx daemon logs` has something to tail.
    init_logging(&paths)?;

    let mut config = ServerConfig::new(paths);
    if let Some(secs) = cli.idle_timeout {
        config.idle_timeout = if secs == 0 { None } else { Some(Duration::from_secs(secs)) };
    }

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(num_cpus_ish())
        .thread_name("versiond-worker")
        .build()
        .context("building tokio runtime")?;
    runtime.block_on(async move {
        // Install SIGINT/SIGTERM handlers so we get a graceful shutdown on
        // launchd/systemd stop requests + ctrl-C in dev.
        tokio::select! {
            res = run(config) => res,
            () = install_signal_handlers() => {
                tracing::info!("signal received, exiting");
                // The server's accept loop observes shutdown via its own
                // Notify; we can't easily reach it from here. For 0.3 we
                // exit the runtime, which is fine — in-flight connections
                // are dropped cleanly.
                Ok(())
            }
        }
    })
}

fn init_logging(paths: &DaemonPaths) -> Result<()> {
    let log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(paths.log_file.as_std_path())
        .with_context(|| format!("opening log file {}", paths.log_file))?;

    let env_filter = EnvFilter::try_from_env("VERSIONX_LOG")
        .unwrap_or_else(|_| EnvFilter::new("versionx_daemon=info,info"));
    let stderr_layer = fmt::layer().with_target(false).with_writer(std::io::stderr);
    let file_layer = fmt::layer().with_target(false).with_writer(log_file).json();

    tracing_subscriber::registry()
        .with(env_filter)
        .with(stderr_layer)
        .with(file_layer)
        .try_init()
        .ok();
    Ok(())
}

fn detach_and_exit(paths: &DaemonPaths) -> Result<()> {
    // Re-exec ourselves in detached mode. We do this by spawning a child
    // process with stdin/stdout/stderr redirected to /dev/null (or the
    // log file) and letting the parent exit. This works uniformly on
    // Unix and Windows without any libc fork dance.
    let exe = std::env::current_exe().context("locating versiond binary")?;
    let log = OpenOptions::new().create(true).append(true).open(paths.log_file.as_std_path())?;

    let mut cmd = std::process::Command::new(exe);
    cmd.arg("--foreground");
    cmd.stdin(std::process::Stdio::null());
    cmd.stdout(std::process::Stdio::from(
        log.try_clone().context("cloning log handle for stdout")?,
    ));
    cmd.stderr(std::process::Stdio::from(log));

    #[cfg(unix)]
    {
        // Become session leader so the parent shell can't send signals
        // to the detached child. `pre_exec` runs in the child after fork
        // but before exec — setsid() there is the standard detach
        // idiom.
        #[allow(unsafe_code)]
        {
            use std::os::unix::process::CommandExt;
            unsafe {
                cmd.pre_exec(|| {
                    unsafe extern "C" {
                        fn setsid() -> i32;
                    }
                    let _ = setsid();
                    Ok(())
                });
            }
        }
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP
        cmd.creation_flags(0x0000_0008 | 0x0000_0200);
    }

    let child = cmd.spawn().context("spawning detached versiond")?;

    // Wait briefly for the child to bind the socket, so the user's
    // next `versionx daemon status` sees a live daemon.
    let paths = paths.clone();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("building probe runtime")?;
    let bound = runtime.block_on(async move {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while tokio::time::Instant::now() < deadline {
            if versionx_daemon::is_running(&paths).await {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        false
    });

    if bound {
        writeln!(std::io::stdout(), "versiond started (pid {})", child.id())
            .context("writing startup status")?;
        Ok(())
    } else {
        anyhow::bail!(
            "versiond started (pid {}) but did not bind the socket within 5s; check the log file",
            child.id(),
        )
    }
}

async fn install_signal_handlers() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut sigint = signal(SignalKind::interrupt()).expect("sigint handler");
        let mut sigterm = signal(SignalKind::terminate()).expect("sigterm handler");
        tokio::select! {
            _ = sigint.recv() => {}
            _ = sigterm.recv() => {}
        }
    }
    #[cfg(windows)]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

fn num_cpus_ish() -> usize {
    // We don't want to pull `num_cpus` into this binary just for one
    // call — std::thread::available_parallelism is good enough.
    std::thread::available_parallelism().map_or(2, |n| n.get().min(4))
}
