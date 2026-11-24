use std::fs;
use std::process::Command;

use chrono::Utc;
use rudder_agent::identity::AgentMeta;
use rudder_agent::status::{AgentSettings, AgentStatus};
use serde_json::Value;
use tempfile::TempDir;

#[test]
fn status_cli_json_and_text_exit_codes_follow_enrollment_and_connection() {
    let dir = TempDir::new().unwrap();
    let invoke = |json| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_rudder-agent"));
        command.args(["status", "--data-dir"]).arg(dir.path());
        if json {
            command.arg("--json");
        }
        command.output().unwrap()
    };
    let missing = invoke(true);
    assert_eq!(missing.status.code(), Some(1));
    let value: Value = serde_json::from_slice(&missing.stdout).unwrap();
    assert_eq!(value["enrolled"], false);
    assert!(String::from_utf8(invoke(false).stdout)
        .unwrap()
        .contains("not enrolled"));
    let meta = AgentMeta {
        server_id: "test-server".into(),
        panel_url: "https://panel.example".into(),
        grpc_url: "https://panel.example:7443".into(),
        tls_name: "rudder-panel".into(),
    };
    fs::write(
        dir.path().join("agent.json"),
        serde_json::to_vec(&meta).unwrap(),
    )
    .unwrap();
    for file in ["cert.pem", "key.pem", "ca.pem"] {
        fs::write(dir.path().join(file), "test identity").unwrap();
    }
    fs::write(
        dir.path().join("config.json"),
        serde_json::to_vec(&AgentSettings { builder: true }).unwrap(),
    )
    .unwrap();
    assert_eq!(invoke(true).status.code(), Some(1));
    let mut status = AgentStatus {
        panel_url: Some(meta.panel_url),
        server_id: Some(meta.server_id),
        enrolled: true,
        connected: true,
        last_connected_at: Some(Utc::now()),
        last_error: None,
        builder: true,
        buildx: true,
        nixpacks: false,
        arch: std::env::consts::ARCH.into(),
        running_jobs: vec!["job-one".into()],
        cache_bytes: 1024,
        cache_entries: 1,
        updated_at: Utc::now(),
    };
    let write = |status: &AgentStatus| {
        fs::write(
            dir.path().join("status.json"),
            serde_json::to_vec(status).unwrap(),
        )
        .unwrap()
    };
    write(&status);
    let connected = invoke(true);
    assert!(connected.status.success());
    let value: Value = serde_json::from_slice(&connected.stdout).unwrap();
    assert_eq!(value["running_jobs"], serde_json::json!(["job-one"]));
    assert_eq!(value["cache_bytes"], 1024);
    let text = invoke(false);
    assert!(text.status.success());
    assert!(String::from_utf8(text.stdout)
        .unwrap()
        .contains("Connection: connected"));
    status.connected = false;
    status.last_error = Some("connection refused".into());
    write(&status);
    let disconnected = invoke(true);
    assert_eq!(disconnected.status.code(), Some(1));
    let value: Value = serde_json::from_slice(&disconnected.stdout).unwrap();
    assert_eq!(value["last_error"], "connection refused");
    status.connected = true;
    status.updated_at -= chrono::Duration::seconds(31);
    write(&status);
    assert_eq!(invoke(true).status.code(), Some(1));
    status.updated_at = Utc::now();
    write(&status);
    fs::remove_file(dir.path().join("key.pem")).unwrap();
    let incomplete = invoke(true);
    assert_eq!(incomplete.status.code(), Some(1));
    let value: Value = serde_json::from_slice(&incomplete.stdout).unwrap();
    assert_eq!(value["enrolled"], false);
}
