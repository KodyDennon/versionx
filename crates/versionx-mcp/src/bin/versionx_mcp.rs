//! `versionx-mcp` binary entry point.
//!
//! Launched by `versionx mcp serve`. For 0.6 we ship the stdio
//! transport as the primary; HTTP support arrives when a use case that
//! genuinely needs it shows up (the spec's loopback HTTP flavor is an
//! opt-in path that most MCP clients don't use).

#![deny(unsafe_code)]

use anyhow::Context;
use camino::Utf8PathBuf;
use clap::Parser;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};
use versionx_mcp::{McpContext, VersionxServer, serve_stdio};

#[derive(Parser, Debug)]
#[command(name = "versionx-mcp", version, about = "Versionx MCP server.")]
struct Cli {
    /// Workspace root. Defaults to the current directory.
    #[arg(long)]
    cwd: Option<Utf8PathBuf>,

    /// Force stdio transport. Today this is the only supported
    /// transport; the flag is kept for forward compatibility with the
    /// eventual HTTP mode.
    #[arg(long, default_value_t = true)]
    stdio: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Logs go to stderr so they don't collide with stdio transport
    // traffic on stdout.
    let env_filter = EnvFilter::try_from_env("VERSIONX_LOG")
        .unwrap_or_else(|_| EnvFilter::new("versionx_mcp=info,warn"));
    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt::layer().with_writer(std::io::stderr).with_target(false))
        .try_init()
        .ok();

    let cli = Cli::parse();
    let cwd = match cli.cwd {
        Some(c) => c,
        None => Utf8PathBuf::from_path_buf(std::env::current_dir().context("current_dir")?)
            .map_err(|p| anyhow::anyhow!("cwd is not UTF-8: {}", p.to_string_lossy()))?,
    };
    let ctx = McpContext::new(cwd).context("McpContext")?;
    tracing::info!(cwd = %ctx.workspace_root, "starting versionx-mcp");
    let server = VersionxServer::new(ctx);
    if cli.stdio {
        serve_stdio(server).await?;
    }
    Ok(())
}
