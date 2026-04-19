//! Versionx shim — a minimal trampoline dispatched by filename.
//!
//! Linux/macOS: this binary is symlinked as `node`, `npm`, `python`, etc.
//! Windows: copied (or hardlinked) per tool as `node.exe`, `npm.exe`, ...
//! — but the Windows path is still a stub in 0.1.0 per the roadmap.
//!
//! On invocation:
//! 1. Read our own `argv[0]` basename — that's the tool name.
//! 2. Walk up from the current directory looking for `versionx.toml`.
//! 3. Load the pinned version for `<tool>` from `[runtimes]`.
//! 4. Resolve the actual binary path under
//!    `$XDG_DATA_HOME/versionx/runtimes/<tool>/<version>/`.
//! 5. `exec()` it with the original argv (Unix) / spawn + wait (Windows).
//!
//! Hot-path target: <5 ms cold. No tokio, no async, no logging allocations.
//! Everything here is straight std + a tiny amount of TOML parsing.

#![deny(unsafe_code)]

use std::env;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(err) => {
            eprintln!("versionx-shim: {err}");
            ExitCode::from(127)
        }
    }
}

fn run() -> Result<u8, String> {
    let argv0 = env::args_os().next().ok_or_else(|| "empty argv[0]".to_string())?;
    let invoked_name = Path::new(&argv0)
        .file_stem()
        .ok_or_else(|| "no file stem in argv[0]".to_string())?
        .to_string_lossy()
        .into_owned();

    // Some tools live under several names (`python`/`python3`, `pip`/`pip3`,
    // `npm`/`npx`). We normalise to their owning runtime tool so we find the
    // right `[runtimes]` entry.
    let (tool_id, binary_basename) = map_invocation(&invoked_name);

    let cwd = env::current_dir().map_err(|e| format!("getting cwd: {e}"))?;
    let version = resolve_version_for(tool_id, &cwd)?;

    let runtimes_dir = runtimes_dir()?;
    let install = runtimes_dir.join(tool_id).join(&version);

    let bin = pick_binary(&install, tool_id, binary_basename)?;

    // Pass the rest of argv straight through.
    let args: Vec<_> = env::args_os().skip(1).collect();

    exec_or_spawn(&bin, &args)
}

/// Map an invoked shim name to `(tool_id_in_versionx_toml, basename_to_exec)`.
const fn map_invocation(name: &str) -> (&str, &str) {
    match name.as_bytes() {
        b"node" => ("node", "node"),
        b"npm" => ("node", "npm"),
        b"npx" => ("node", "npx"),
        b"python" | b"python3" => ("python", "python3"),
        b"pip" | b"pip3" => ("python", "pip3"),
        b"cargo" => ("rust", "cargo"),
        b"rustc" => ("rust", "rustc"),
        b"rustfmt" => ("rust", "rustfmt"),
        b"clippy-driver" => ("rust", "clippy-driver"),
        b"rust-analyzer" => ("rust", "rust-analyzer"),
        b"rustdoc" => ("rust", "rustdoc"),
        // Tool-manager runtimes (pnpm/uv/poetry) are their own runtime records.
        b"pnpm" => ("pnpm", "pnpm"),
        b"yarn" => ("yarn", "yarn"),
        b"uv" => ("uv", "uv"),
        b"poetry" => ("poetry", "poetry"),
        _ => ("", ""),
    }
}

fn resolve_version_for(tool_id: &str, cwd: &Path) -> Result<String, String> {
    if tool_id.is_empty() {
        return Err("unknown tool — shim invoked as a name without a mapping".into());
    }

    // First check a per-shell override (`VERSIONX_<TOOL>_VERSION=`).
    let env_key = format!("VERSIONX_{}_VERSION", tool_id.to_uppercase());
    if let Ok(v) = env::var(&env_key) {
        return Ok(v);
    }

    // Walk up for a versionx.toml.
    let mut cursor: Option<&Path> = Some(cwd);
    while let Some(dir) = cursor {
        let candidate = dir.join("versionx.toml");
        if candidate.is_file()
            && let Some(version) = read_tool_version(&candidate, tool_id)?
        {
            return Ok(version);
        }
        cursor = dir.parent();
    }

    // Fall back to the user-level global default.
    if let Some(v) = read_user_global(tool_id)? {
        return Ok(v);
    }

    Err(format!(
        "no version pinned for `{tool_id}` in versionx.toml or user global (set one with `versionx global set {tool_id} <version>`)"
    ))
}

/// Load `versionx.toml` and return the pinned version for `tool_id`.
/// We stay in the `toml` crate here — it's the shim's only non-std dep —
/// to keep hot-path cost bounded.
fn read_tool_version(config_path: &Path, tool_id: &str) -> Result<Option<String>, String> {
    let raw = std::fs::read_to_string(config_path)
        .map_err(|e| format!("reading {}: {e}", config_path.display()))?;
    let value: toml::Value =
        toml::from_str(&raw).map_err(|e| format!("parsing {}: {e}", config_path.display()))?;
    let Some(toml::Value::Table(runtimes)) = value.get("runtimes") else {
        return Ok(None);
    };
    let Some(entry) = runtimes.get(tool_id) else {
        return Ok(None);
    };
    Ok(match entry {
        toml::Value::String(s) => Some(s.clone()),
        toml::Value::Table(t) => t.get("version").and_then(|v| v.as_str()).map(str::to_string),
        _ => None,
    })
}

fn read_user_global(tool_id: &str) -> Result<Option<String>, String> {
    let Some(path) = user_global_path() else {
        return Ok(None);
    };
    if !path.is_file() {
        return Ok(None);
    }
    read_tool_version(&path, tool_id)
}

fn runtimes_dir() -> Result<PathBuf, String> {
    if let Ok(explicit) = env::var("VERSIONX_HOME") {
        return Ok(PathBuf::from(explicit).join("runtimes"));
    }
    let dirs = directories::BaseDirs::new().ok_or_else(|| "no home directory".to_string())?;
    let data = dirs.data_dir();
    Ok(data.join("versionx").join("runtimes"))
}

fn user_global_path() -> Option<PathBuf> {
    if let Ok(explicit) = env::var("VERSIONX_HOME") {
        return Some(PathBuf::from(explicit).join("global.toml"));
    }
    let dirs = directories::BaseDirs::new()?;
    Some(dirs.config_dir().join("versionx").join("global.toml"))
}

/// Locate the real binary inside `install`. Different tools organise their
/// install differently (Node ships `bin/node`; Python's `bin/python3`;
/// rust's toolchain at `bin/cargo`).
fn pick_binary(install: &Path, tool_id: &str, basename: &str) -> Result<PathBuf, String> {
    // Windows-specific layouts (stubbed — this shim on Windows is a v0.1 stub
    // per the roadmap, but we return something plausible for local testing).
    if cfg!(target_os = "windows") {
        let exe = format!("{basename}.exe");
        let candidates = [install.join(&exe), install.join("bin").join(&exe)];
        for c in &candidates {
            if c.is_file() {
                return Ok(c.clone());
            }
        }
        return Err(format!("can't find `{exe}` in {}", install.display()));
    }

    let primary = install.join("bin").join(basename);
    if primary.is_file() {
        return Ok(primary);
    }
    // Fallback: sometimes tools land at the install root without bin/.
    let fallback = install.join(basename);
    if fallback.is_file() {
        return Ok(fallback);
    }
    Err(format!("can't find `{basename}` for `{tool_id}` in {}", install.display()))
}

#[cfg(unix)]
fn exec_or_spawn(bin: &Path, args: &[std::ffi::OsString]) -> Result<u8, String> {
    use std::os::unix::process::CommandExt;

    // `exec` replaces this process — it only returns on failure.
    let err = std::process::Command::new(bin).args(args).exec();
    Err(format!("exec {}: {err}", bin.display()))
}

#[cfg(windows)]
fn exec_or_spawn(bin: &Path, args: &[std::ffi::OsString]) -> Result<u8, String> {
    // Windows has no exec(); spawn + wait + propagate exit code.
    let status = std::process::Command::new(bin)
        .args(args)
        .status()
        .map_err(|e| format!("spawning {}: {e}", bin.display()))?;
    Ok(u8::try_from(status.code().unwrap_or(1).max(0).min(255)).unwrap_or(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_common_invocations() {
        assert_eq!(map_invocation("node"), ("node", "node"));
        assert_eq!(map_invocation("npx"), ("node", "npx"));
        assert_eq!(map_invocation("python3"), ("python", "python3"));
        assert_eq!(map_invocation("pip"), ("python", "pip3"));
        assert_eq!(map_invocation("cargo"), ("rust", "cargo"));
        assert_eq!(map_invocation("unknown"), ("", ""));
    }

    #[test]
    fn reads_plain_string_version() {
        use std::io::Write;
        let tmp = tempfile::NamedTempFile::new().unwrap();
        writeln!(&tmp, "[runtimes]\nnode = \"22.12.0\"").unwrap();
        let v = read_tool_version(tmp.path(), "node").unwrap().unwrap();
        assert_eq!(v, "22.12.0");
    }

    #[test]
    fn reads_detailed_table_version() {
        use std::io::Write;
        let tmp = tempfile::NamedTempFile::new().unwrap();
        writeln!(&tmp, "[runtimes]\njvm = {{ version = \"21\", distribution = \"temurin\" }}")
            .unwrap();
        let v = read_tool_version(tmp.path(), "jvm").unwrap().unwrap();
        assert_eq!(v, "21");
    }

    #[test]
    fn missing_runtime_returns_none() {
        use std::io::Write;
        let tmp = tempfile::NamedTempFile::new().unwrap();
        writeln!(&tmp, "[runtimes]\nnode = \"20\"").unwrap();
        assert!(read_tool_version(tmp.path(), "python").unwrap().is_none());
    }
}
