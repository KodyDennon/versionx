//! Internal cargo-xtask helper. Run via `cargo xtask <subcommand>`.
//!
//! Used for packaging, release automation, and repo chores that
//! shouldn't be in the public `versionx` CLI.

#![deny(unsafe_code)]

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "xtask", about = "Internal repo tasks for Versionx")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Run `cargo check` + `cargo clippy` + `cargo test` with the usual flags.
    Ci,
    /// Print the crate list in workspace member order.
    Crates,
}

fn main() -> Result<()> {
    let args = Cli::parse();
    match args.cmd {
        Cmd::Ci => {
            duct::cmd!("cargo", "fmt", "--all", "--check").run()?;
            duct::cmd!("cargo", "clippy", "--workspace", "--all-targets", "--", "-D", "warnings")
                .run()?;
            duct::cmd!("cargo", "test", "--workspace").run()?;
            Ok(())
        }
        Cmd::Crates => {
            for crate_dir in std::fs::read_dir("crates")? {
                let entry = crate_dir?;
                println!("{}", entry.file_name().to_string_lossy());
            }
            Ok(())
        }
    }
}
