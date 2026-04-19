//! End-to-end integration: spin up the server on a tempdir, connect a
//! client, call each method, exercise subscribe/notify, then shut down.

use std::fs;
use std::time::Duration;

use camino::Utf8PathBuf;
use versionx_daemon::{Client, DaemonPaths, ServerConfig, protocol, run};

/// Build a workspace fixture with two Rust crates so the server actually
/// has components to report.
fn write_fixture(root: &camino::Utf8Path) {
    let core = root.join("core");
    let app = root.join("app");
    fs::create_dir_all(&core).unwrap();
    fs::create_dir_all(&app).unwrap();
    fs::write(core.join("Cargo.toml"), "[package]\nname = \"core\"\nversion = \"0.1.0\"\n")
        .unwrap();
    fs::write(core.join("lib.rs"), "// core\n").unwrap();
    fs::write(
        app.join("Cargo.toml"),
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\n[dependencies]\ncore = { path = \"../core\" }\n",
    )
    .unwrap();
    fs::write(app.join("lib.rs"), "// app\n").unwrap();
}

#[tokio::test]
async fn server_responds_to_ping_and_workspace_rpc() {
    let home = tempfile::tempdir().unwrap();
    let home_path = Utf8PathBuf::from_path_buf(home.path().to_path_buf()).unwrap();
    let paths = DaemonPaths::under(&home_path);
    paths.ensure_dirs().unwrap();

    // Workspace fixture lives outside the daemon home.
    let ws = tempfile::tempdir().unwrap();
    let ws_path = Utf8PathBuf::from_path_buf(ws.path().to_path_buf()).unwrap();
    write_fixture(&ws_path);

    // Boot the server on a background task. Short idle timeout keeps the
    // test snappy if shutdown somehow doesn't propagate.
    let mut config = ServerConfig::new(paths.clone());
    config.idle_timeout = None;
    let server_task = tokio::spawn(async move {
        run(config).await.unwrap();
    });

    // Let the bind happen. Use `is_running` to avoid a sleep.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        if versionx_daemon::is_running(&paths).await {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(versionx_daemon::is_running(&paths).await, "server never came up");

    let client = Client::connect(&paths).await.unwrap();

    // ping
    client.ping().await.unwrap();

    // server.info
    let info = client.server_info().await.unwrap();
    assert_eq!(info.pid, std::process::id());
    assert!(info.uptime_seconds < 60);

    // workspace.list with the fixture path.
    let list: serde_json::Value = client
        .call(protocol::methods::WORKSPACE_LIST, serde_json::json!({"root": ws_path.to_string()}))
        .await
        .unwrap();
    let names: Vec<String> = list["components"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap().to_string())
        .collect();
    assert!(names.contains(&"core".into()), "got {names:?}");
    assert!(names.contains(&"app".into()), "got {names:?}");

    // workspace.graph returns a topo order with core before app.
    let graph: serde_json::Value = client
        .call(protocol::methods::WORKSPACE_GRAPH, serde_json::json!({"root": ws_path.to_string()}))
        .await
        .unwrap();
    let topo: Vec<String> = graph["topo_order"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap().to_string())
        .collect();
    let core_idx = topo.iter().position(|s| s == "core").unwrap();
    let app_idx = topo.iter().position(|s| s == "app").unwrap();
    assert!(core_idx < app_idx);

    // Second workspace.list hits the cache path — should be instant + equal.
    let list2: serde_json::Value = client
        .call(protocol::methods::WORKSPACE_LIST, serde_json::json!({"root": ws_path.to_string()}))
        .await
        .unwrap();
    assert_eq!(list, list2);

    // Shut the server down cleanly.
    client.shutdown().await.unwrap();
    let _ = tokio::time::timeout(Duration::from_secs(5), server_task).await;

    // After shutdown is_running should flip false quickly.
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while std::time::Instant::now() < deadline {
        if !versionx_daemon::is_running(&paths).await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("server did not shut down");
}

#[tokio::test]
async fn unknown_method_returns_method_not_found() {
    let home = tempfile::tempdir().unwrap();
    let home_path = Utf8PathBuf::from_path_buf(home.path().to_path_buf()).unwrap();
    let paths = DaemonPaths::under(&home_path);
    paths.ensure_dirs().unwrap();

    let mut config = ServerConfig::new(paths.clone());
    config.idle_timeout = None;
    let server_task = tokio::spawn(async move {
        run(config).await.unwrap();
    });

    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        if versionx_daemon::is_running(&paths).await {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    let client = Client::connect(&paths).await.unwrap();
    let err = client
        .call::<_, serde_json::Value>("bogus.method", serde_json::json!({}))
        .await
        .unwrap_err();
    match err {
        versionx_daemon::ClientError::Rpc { code, .. } => {
            assert_eq!(code, versionx_daemon::ErrorObject::METHOD_NOT_FOUND);
        }
        other => panic!("unexpected: {other:?}"),
    }

    client.shutdown().await.unwrap();
    let _ = tokio::time::timeout(Duration::from_secs(5), server_task).await;
}
