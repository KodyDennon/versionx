//! End-to-end tests for `versionx init`.
//!
//! Exercises the real binary through `assert_cmd`. Each test creates a
//! tempdir, seeds it with an ecosystem signal, runs the binary, and checks
//! both the output and the written file.

#![deny(unsafe_code)]

use std::fs;

use assert_cmd::Command;
use tempfile::TempDir;

fn versionx() -> Command {
    Command::cargo_bin("versionx").expect("versionx binary is built")
}

#[test]
fn init_detects_node_with_packagemanager_field() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("package.json"),
        r#"{"name":"demo","packageManager":"pnpm@8.15.0","engines":{"node":"22.12.0"}}"#,
    )
    .unwrap();

    versionx().current_dir(dir.path()).arg("init").assert().success();

    let toml = fs::read_to_string(dir.path().join("versionx.toml")).unwrap();
    assert!(toml.contains("schema_version = \"1\""));
    assert!(toml.contains("package_manager = \"pnpm\""));
    assert!(toml.contains("pnpm = \"8.15.0\""));
    assert!(toml.contains("node = \"22.12.0\""));
}

#[test]
fn init_json_output_is_parseable() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("package.json"), r#"{"name":"demo","packageManager":"pnpm@8.15.0"}"#)
        .unwrap();

    let out = versionx()
        .current_dir(dir.path())
        .args(["init", "--output", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let parsed: serde_json::Value = serde_json::from_slice(&out).expect("valid JSON on stdout");
    assert_eq!(parsed["created"], true);
    assert_eq!(parsed["ecosystems"][0], "node");
}

#[test]
fn init_refuses_overwrite_without_force() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("versionx.toml"), "# existing\n").unwrap();
    fs::write(dir.path().join("package.json"), r#"{"name":"demo"}"#).unwrap();

    let out =
        versionx().current_dir(dir.path()).arg("init").assert().failure().get_output().clone();

    // Exit code 1 = user error per the CLI's error mapping.
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("already exists"), "stderr: {stderr}");

    // Original file must be untouched.
    assert_eq!(fs::read_to_string(dir.path().join("versionx.toml")).unwrap(), "# existing\n");
}

#[test]
fn init_force_overwrites() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("versionx.toml"), "# old\n").unwrap();
    fs::write(dir.path().join("package.json"), r#"{"name":"demo","packageManager":"pnpm@8.15.0"}"#)
        .unwrap();

    versionx().current_dir(dir.path()).args(["init", "--force"]).assert().success();

    let toml = fs::read_to_string(dir.path().join("versionx.toml")).unwrap();
    assert!(!toml.contains("# old"));
    assert!(toml.contains("schema_version = \"1\""));
}

#[test]
fn init_errors_on_empty_directory_with_json_output() {
    let dir = TempDir::new().unwrap();
    let out = versionx()
        .current_dir(dir.path())
        .args(["init", "--output", "json"])
        .assert()
        .failure()
        .get_output()
        .clone();

    // Exit code 3 = NoEcosystemsDetected per the CLI's error mapping.
    assert_eq!(out.status.code(), Some(3));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("no_ecosystems_detected"), "stderr: {stderr}");
}

#[test]
fn init_detects_rust_workspace() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("Cargo.toml"), "[workspace]\nmembers = [\"crates/*\"]\n").unwrap();

    let out = versionx()
        .current_dir(dir.path())
        .args(["init", "--output", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let parsed: serde_json::Value = serde_json::from_slice(&out).unwrap();
    let ecosystems = parsed["ecosystems"].as_array().unwrap();
    assert!(ecosystems.iter().any(|e| e == "rust"));
}

#[test]
fn help_json_flag_emits_command_list() {
    let out = versionx().arg("--help-json").assert().success().get_output().stdout.clone();

    let parsed: serde_json::Value = serde_json::from_slice(&out).unwrap();
    let subs = parsed["command"]["subcommands"].as_array().expect("subcommands array");
    assert!(subs.iter().any(|s| s["name"].as_str() == Some("init")), "expected `init` in {subs:?}");
}

#[test]
fn version_flag_works() {
    versionx().arg("--version").assert().success().stdout(predicates::str::contains("versionx"));
}
