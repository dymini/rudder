//! Remote servers end to end: enrollment over HTTP, the mTLS gRPC session, the
//! remote driver, and telemetry shipping, with a real `dymini-panel-agent` running in
//! process against real listeners.
//!
//! The agent side uses a driver without a Docker socket, so everything except
//! container operations runs in CI. The Docker test runs only with
//! `DYMINI_PANEL_DOCKER_TESTS=1`.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use dymini_panel_agent::{Buffer, Identity, RunOptions};
use dymini_panel_core::{ArchiveSource, ExecSpec, NodeDriver};
use dymini_panel_proto::agent::{agent_message, AgentMessage, Hello};
use dymini_panel_proto::NodeAgentClient;
use futures_util::StreamExt;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("dymini-panel-agent-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Panel {
    state: dymini_panel_server::AppState,
    router: axum::Router,
    http: String,
    cookie: String,
    csrf: String,
    _dir: TempDir,
}

impl Panel {
    async fn start(pool: PgPool) -> Self {
        let dir = TempDir::new();
        let agent_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let agent_port = agent_listener.local_addr().unwrap().port();
        let url = std::env::var("DATABASE_URL")
            .unwrap_or_else(|_| "postgres://localhost/dymini-panel".into());
        let vars = HashMap::from([
            ("DATABASE_URL".into(), url),
            ("DYMINI_PANEL_MASTER_KEY".into(), "cc".repeat(32)),
            ("DYMINI_PANEL_SETUP_TOKEN".into(), "test-setup-token".into()),
            ("DYMINI_PANEL_COOKIE_SECURE".into(), "false".into()),
            (
                "DYMINI_PANEL_DATA_DIR".into(),
                dir.path().display().to_string(),
            ),
            (
                "DYMINI_PANEL_AGENT_URL".into(),
                format!("https://127.0.0.1:{agent_port}"),
            ),
            (
                "DYMINI_PANEL_AGENT_BIND".into(),
                format!("127.0.0.1:{agent_port}"),
            ),
        ]);
        let config = dymini_panel_server::Config::from_map(&vars, |_| unreachable!()).unwrap();
        let state = dymini_panel_server::AppState::new(pool, config);
        tokio::spawn(dymini_panel_server::serve_agents(
            state.clone(),
            agent_listener,
        ));
        dymini_panel_server::start_log_pipeline(&state);
        let router = dymini_panel_server::router(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr: SocketAddr = listener.local_addr().unwrap();
        let app = router.clone();
        tokio::spawn(async move {
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .unwrap();
        });

        let response = router
            .clone()
            .oneshot(
                Request::post("/api/v1/setup")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "setup_token": "test-setup-token",
                            "email": "owner@example.com",
                            "password": "correct-horse-battery",
                            "display_name": "Owner"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let cookie = response
            .headers()
            .get("set-cookie")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(';').next())
            .and_then(|v| v.strip_prefix("dymini_panel_session="))
            .unwrap()
            .to_owned();
        let body: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        Self {
            state,
            router,
            http: format!("http://{addr}"),
            cookie,
            csrf: body["csrf_token"].as_str().unwrap().to_owned(),
            _dir: dir,
        }
    }

    async fn call(&self, method: &str, path: &str, body: Option<Value>) -> (StatusCode, Value) {
        let builder = Request::builder()
            .method(method)
            .uri(path)
            .header("host", self.http.trim_start_matches("http://"))
            .header("cookie", format!("dymini_panel_session={}", self.cookie))
            .header("x-csrf-token", &self.csrf);
        let request = match body {
            Some(body) => builder
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
            None => builder.body(Body::empty()).unwrap(),
        };
        let response = self.router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn ok(&self, method: &str, path: &str, body: Option<Value>) -> Value {
        let (status, value) = self.call(method, path, body).await;
        assert!(status.is_success(), "{method} {path}: {status} {value}");
        value
    }

    async fn server(&self, id: &str) -> Value {
        let servers = self.ok("GET", "/api/v1/servers", None).await;
        servers
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["id"] == id)
            .cloned()
            .unwrap_or(Value::Null)
    }

    async fn wait_for_status(&self, id: &str, status: &str) -> Value {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
        loop {
            let server = self.server(id).await;
            if server["status"] == status {
                return server;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "server never became {status}: {server}"
            );
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }

    async fn driver(&self, id: &str) -> Arc<dyn NodeDriver> {
        dymini_panel_server::node_driver(&self.state, id.parse().unwrap())
            .await
            .map_err(|e| e.public_message())
            .unwrap()
    }
}

/// An agent driver that has no Docker socket: files and host stats work.
fn node_driver(node_dir: &Path) -> Arc<dyn NodeDriver> {
    Arc::new(dymini_panel_node::LocalDriver::connect(
        node_dir,
        Some(&node_dir.join("no-docker.sock")),
    ))
}

fn start_agent(
    identity: Identity,
    data_dir: &Path,
    driver: Arc<dyn NodeDriver>,
    buffer: Arc<Buffer>,
) -> tokio::task::JoinHandle<()> {
    let options = RunOptions {
        identity,
        data_dir: data_dir.to_owned(),
        stats_interval: Duration::from_millis(300),
        self_update: None,
        builder: None,
    };
    tokio::spawn(dymini_panel_agent::run_with(
        options,
        driver,
        buffer,
        std::future::pending(),
    ))
}

/// Open a session directly, to check what the listener accepts.
async fn dial(identity: &Identity, cert_pem: &str, key_pem: &str) -> Result<(), String> {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    let tls = tonic::transport::ClientTlsConfig::new()
        .ca_certificate(tonic::transport::Certificate::from_pem(&identity.ca_pem))
        .identity(tonic::transport::Identity::from_pem(cert_pem, key_pem))
        .domain_name(identity.meta.tls_name.clone());
    let channel = tonic::transport::Endpoint::from_shared(identity.meta.grpc_url.clone())
        .unwrap()
        .tls_config(tls)
        .unwrap()
        .connect()
        .await
        .map_err(|e| format!("connect: {e:?}"))?;
    let hello = AgentMessage {
        body: Some(agent_message::Body::Hello(Hello {
            agent_version: "test".into(),
            ..Hello::default()
        })),
    };
    let mut stream = NodeAgentClient::new(channel)
        .session(tokio_stream::iter(vec![hello]))
        .await
        .map_err(|status| format!("{:?}: {}", status.code(), status.message()))?
        .into_inner();
    match stream.message().await {
        Ok(Some(_)) => Ok(()),
        Ok(None) => Err("closed".into()),
        Err(status) => Err(format!("{:?}: {}", status.code(), status.message())),
    }
}

async fn count(pool: &PgPool, sql: &str, server_id: &str) -> i64 {
    sqlx::query_scalar(sql)
        .bind(server_id.parse::<Uuid>().unwrap())
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn wait_for_count(pool: &PgPool, sql: &str, server_id: &str) -> i64 {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    loop {
        let n = count(pool, sql, server_id).await;
        if n > 0 || tokio::time::Instant::now() > deadline {
            return n;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn agent_enrolls_connects_runs_commands_and_ships_telemetry(pool: PgPool) {
    let panel = Panel::start(pool.clone()).await;
    let created = panel
        .ok(
            "POST",
            "/api/v1/servers",
            Some(json!({ "name": "edge-1", "kind": "remote" })),
        )
        .await;
    let server_id = created["id"].as_str().unwrap().to_owned();
    let token = created["join_token"].as_str().unwrap().to_owned();
    let command = created["join_command"].as_str().unwrap();
    assert!(
        command.contains(&format!("{}/mcp/install-agent.sh", panel.http)),
        "{command}"
    );
    assert!(command.ends_with(&format!("--token {token}")), "{command}");
    let ca_sha256 = command
        .split_whitespace()
        .skip_while(|word| *word != "--ca-sha256")
        .nth(1)
        .expect("the join command pins the agent CA")
        .to_owned();
    assert_eq!(ca_sha256.len(), 64, "{command}");
    assert_eq!(created["status"], "pending");

    let agent_dir = TempDir::new();
    let data_dir = agent_dir.path().join("agent");
    let node_dir = agent_dir.path().join("node");
    std::fs::create_dir_all(&node_dir).unwrap();

    let error = dymini_panel_agent::enroll(&panel.http, "join_not-a-real-token", None, &data_dir)
        .await
        .unwrap_err();
    assert!(format!("{error:#}").contains("join token"), "{error:#}");

    let identity = dymini_panel_agent::enroll(&panel.http, &token, Some(&ca_sha256), &data_dir)
        .await
        .unwrap();
    assert_eq!(identity.meta.server_id, server_id);
    assert!(identity.meta.grpc_url.starts_with("https://127.0.0.1:"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(data_dir.join("key.pem"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    let reloaded = Identity::load(&data_dir).unwrap();
    assert_eq!(reloaded.cert_pem, identity.cert_pem);
    let reused =
        dymini_panel_agent::enroll(&panel.http, &token, None, &agent_dir.path().join("again"))
            .await;
    assert!(reused.is_err(), "a join token works once");
    assert!(
        !panel.server(&server_id).await.to_string().contains(&token),
        "the token is not listed"
    );

    let buffer = Arc::new(Buffer::default());
    buffer.push_log(dymini_panel_proto::agent::LogRecord {
        logged_at_unix_ms: chrono::Utc::now().timestamp_millis(),
        text: r#"{"level":"WARN","fields":{"message":"docker socket missing"}}"#.into(),
        source: "agent".into(),
        ..Default::default()
    });
    let agent = start_agent(
        identity.clone(),
        &data_dir,
        node_driver(&node_dir),
        buffer.clone(),
    );
    let online = panel.wait_for_status(&server_id, "online").await;
    assert_eq!(online["agent_version"], dymini_panel_core::VERSION);
    let stored_arch: String = sqlx::query_scalar("SELECT arch FROM servers WHERE id = $1")
        .bind(server_id.parse::<Uuid>().unwrap())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(stored_arch, std::env::consts::ARCH);
    let status = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let status = dymini_panel_agent::status::read(&data_dir).unwrap();
            if status.connected {
                break status;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(status.enrolled && status.connected);
    assert_eq!(status.server_id.as_deref(), Some(server_id.as_str()));
    assert!(online["cpu_count"].as_i64().unwrap() > 0, "{online}");
    assert!(online["memory_bytes"].as_i64().unwrap() > 0, "{online}");

    // Every unary call goes through the session to the agent's driver.
    let driver = panel.driver(&server_id).await;
    driver
        .write_file("traefik/dynamic/routes.yml", b"http: {}\n")
        .await
        .unwrap();
    assert_eq!(
        std::fs::read(node_dir.join("traefik/dynamic/routes.yml")).unwrap(),
        b"http: {}\n"
    );
    assert_eq!(
        driver
            .read_file("traefik/dynamic/routes.yml")
            .await
            .unwrap(),
        b"http: {}\n"
    );
    let escape = driver.read_file("../../etc/passwd").await.unwrap_err();
    assert!(
        matches!(escape, dymini_panel_core::Error::Validation(_)),
        "{escape:?}"
    );
    let host = driver.host_stats().await.unwrap();
    assert!(host.cpu_count > 0);
    let docker = driver.docker_info().await.unwrap_err();
    assert!(
        matches!(docker, dymini_panel_core::Error::Node(_)),
        "{docker:?}"
    );
    // Streaming calls fail cleanly when the agent cannot reach Docker.
    let archive = driver
        .archive_read(&ArchiveSource::Volume {
            name: "data".into(),
        })
        .await;
    assert!(archive.is_err());

    // Telemetry: host samples and the agent's own log line reach the panel.
    let samples = wait_for_count(
        &pool,
        "SELECT count(*) FROM metrics_samples WHERE server_id = $1 AND metric = 'cpu_percent'",
        &server_id,
    )
    .await;
    assert!(samples > 0, "no host samples were stored");
    let agent_logs = wait_for_count(
        &pool,
        "SELECT count(*) FROM log_entries WHERE server_id = $1 AND source = 'agent' AND level = 'warn'",
        &server_id,
    )
    .await;
    assert_eq!(agent_logs, 1);
    let online_events = count(
        &pool,
        "SELECT count(*) FROM system_events WHERE server_id = $1 AND kind = 'server.online'",
        &server_id,
    )
    .await;
    assert_eq!(online_events, 1);

    // A certificate from another CA fails the TLS handshake.
    let other_ca = rcgen::KeyPair::generate().unwrap();
    let mut params = rcgen::CertificateParams::new(Vec::<String>::new()).unwrap();
    params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    let other_ca_cert = params.self_signed(&other_ca).unwrap();
    let issuer = rcgen::Issuer::new(params, other_ca);
    let rogue_key = rcgen::KeyPair::generate().unwrap();
    let rogue = rcgen::CertificateParams::new(vec!["rogue.agent.dymini-panel".to_owned()])
        .unwrap()
        .signed_by(&rogue_key, &issuer)
        .unwrap();
    drop(other_ca_cert);
    assert!(dial(&identity, &rogue.pem(), &rogue_key.serialize_pem())
        .await
        .is_err());

    // Re-enrolling issues a new certificate and retires the old one.
    let rotated = panel
        .ok(
            "POST",
            &format!("/api/v1/servers/{server_id}/join-token"),
            None,
        )
        .await;
    let new_token = rotated["join_token"].as_str().unwrap();
    assert!(rotated["join_command"]
        .as_str()
        .unwrap()
        .contains(new_token));
    let new_dir = agent_dir.path().join("agent2");
    let new_identity = dymini_panel_agent::enroll(&panel.http, new_token, None, &new_dir)
        .await
        .unwrap();
    let old = dial(&identity, &identity.cert_pem, &identity.key_pem)
        .await
        .unwrap_err();
    assert!(old.contains("PermissionDenied"), "{old}");
    agent.abort();
    let agent = start_agent(
        new_identity.clone(),
        &new_dir,
        node_driver(&node_dir),
        Arc::new(Buffer::default()),
    );
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    while dymini_panel_server::node_driver(&panel.state, server_id.parse().unwrap())
        .await
        .is_err()
    {
        assert!(
            tokio::time::Instant::now() < deadline,
            "new agent never connected"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    // Stopping the agent marks the server offline and records it.
    agent.abort();
    panel.wait_for_status(&server_id, "offline").await;
    // The event is written just after the status flips, so wait for it.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let offline_events = count(
            &pool,
            "SELECT count(*) FROM system_events WHERE server_id = $1 AND kind = 'server.offline'",
            &server_id,
        )
        .await;
        if offline_events >= 1 {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "no server.offline event was recorded"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let unavailable =
        dymini_panel_server::node_driver(&panel.state, server_id.parse().unwrap()).await;
    assert!(unavailable.is_err());

    // Deleting the server drops a live session and its certificate stops working.
    let agent = start_agent(
        new_identity.clone(),
        &new_dir,
        node_driver(&node_dir),
        Arc::new(Buffer::default()),
    );
    panel.wait_for_status(&server_id, "online").await;
    panel
        .ok("DELETE", &format!("/api/v1/servers/{server_id}"), None)
        .await;
    assert!(
        dymini_panel_server::node_driver(&panel.state, server_id.parse().unwrap())
            .await
            .is_err()
    );
    let gone = dial(&new_identity, &new_identity.cert_pem, &new_identity.key_pem)
        .await
        .unwrap_err();
    assert!(gone.contains("PermissionDenied"), "{gone}");
    agent.abort();

    let audited: i64 =
        sqlx::query_scalar("SELECT count(*) FROM audit_log WHERE action = 'server.enroll'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(audited, 2);

    // A CA that does not match the pinned fingerprint is refused and nothing is saved.
    let pin = panel
        .ok(
            "POST",
            "/api/v1/servers",
            Some(json!({ "name": "edge-pinned", "kind": "remote" })),
        )
        .await;
    let pinned_dir = agent_dir.path().join("pinned");
    let wrong_ca = dymini_panel_agent::enroll(
        &panel.http,
        pin["join_token"].as_str().unwrap(),
        Some(&"0".repeat(64)),
        &pinned_dir,
    )
    .await
    .unwrap_err();
    assert!(
        format!("{wrong_ca:#}").contains("fingerprint"),
        "{wrong_ca:#}"
    );
    assert!(!pinned_dir.join("agent.json").exists(), "nothing is saved");
}

/// When the direct gRPC port is unreachable — a Cloudflare Tunnel panel with
/// no inbound ports — the agent runs the same mTLS session through the
/// WebSocket tunnel on the panel's HTTPS endpoint.
#[sqlx::test(migrations = "./migrations")]
async fn agent_session_survives_over_the_websocket_tunnel(pool: PgPool) {
    let panel = Panel::start(pool.clone()).await;
    let created = panel
        .ok(
            "POST",
            "/api/v1/servers",
            Some(json!({ "name": "tunneled", "kind": "remote" })),
        )
        .await;
    let server_id = created["id"].as_str().unwrap().to_owned();
    let token = created["join_token"].as_str().unwrap().to_owned();

    let agent_dir = TempDir::new();
    let data_dir = agent_dir.path().join("agent");
    let node_dir = agent_dir.path().join("node");
    std::fs::create_dir_all(&node_dir).unwrap();

    let mut identity = dymini_panel_agent::enroll(&panel.http, &token, None, &data_dir)
        .await
        .unwrap();
    // Enrollment said where the listener is, but nothing can reach that port.
    identity.meta.grpc_url = "https://127.0.0.1:1".into();

    let agent = start_agent(
        identity.clone(),
        &data_dir,
        node_driver(&node_dir),
        Arc::new(Buffer::default()),
    );
    panel.wait_for_status(&server_id, "online").await;
    let driver = panel.driver(&server_id).await;
    driver
        .write_file("through/tunnel.txt", b"ok\n")
        .await
        .unwrap();
    assert_eq!(
        std::fs::read(node_dir.join("through/tunnel.txt")).unwrap(),
        b"ok\n"
    );
    agent.abort();
}

#[sqlx::test(migrations = "./migrations")]
async fn join_tokens_are_for_remote_servers_and_writers_only(pool: PgPool) {
    let panel = Panel::start(pool.clone()).await;
    let local = panel
        .ok(
            "POST",
            "/api/v1/servers",
            Some(json!({ "name": "local", "kind": "local" })),
        )
        .await;
    let local = local["id"].as_str().unwrap();
    let (status, body) = panel
        .call("POST", &format!("/api/v1/servers/{local}/join-token"), None)
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");

    let response = panel
        .router
        .clone()
        .oneshot(
            Request::get("/install-agent.sh")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let script = response.into_body().collect().await.unwrap().to_bytes();
    let script = String::from_utf8(script.to_vec()).unwrap();
    assert!(script.starts_with("#!/bin/sh"));
    assert!(script.contains("dymini-panel-agent enroll") || script.contains("\" enroll --server"));

    for path in [
        "/downloads/dymini-panel-agent",
        "/downloads/..%2Fetc%2Fpasswd",
    ] {
        let response = panel
            .router
            .clone()
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
    }

    let unauthenticated = panel
        .router
        .clone()
        .oneshot(
            Request::post(format!("/api/v1/servers/{local}/join-token"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unauthenticated.status(), StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrations = "./migrations")]
async fn remote_streams_run_on_the_agent_docker(pool: PgPool) {
    if std::env::var("DYMINI_PANEL_DOCKER_TESTS").as_deref() != Ok("1") {
        eprintln!("skipping: set DYMINI_PANEL_DOCKER_TESTS=1 to run Docker tests");
        return;
    }
    let panel = Panel::start(pool.clone()).await;
    let created = panel
        .ok(
            "POST",
            "/api/v1/servers",
            Some(json!({ "name": "edge-docker", "kind": "remote" })),
        )
        .await;
    let server_id = created["id"].as_str().unwrap().to_owned();
    let project = panel
        .ok("POST", "/api/v1/projects", Some(json!({ "name": "apps" })))
        .await;
    let environment = panel
        .ok(
            "POST",
            &format!(
                "/api/v1/projects/{}/environments",
                project["id"].as_str().unwrap()
            ),
            Some(json!({ "name": "prod" })),
        )
        .await;
    let service = panel
        .ok(
            "POST",
            &format!(
                "/api/v1/environments/{}/services",
                environment["id"].as_str().unwrap()
            ),
            Some(json!({ "name": "web", "kind": "application", "server_id": server_id })),
        )
        .await;
    let service_id = service["id"].as_str().unwrap().to_owned();

    let agent_dir = TempDir::new();
    let data_dir = agent_dir.path().join("agent");
    let node_dir = agent_dir.path().join("node");
    std::fs::create_dir_all(&node_dir).unwrap();
    let identity = dymini_panel_agent::enroll(
        &panel.http,
        created["join_token"].as_str().unwrap(),
        None,
        &data_dir,
    )
    .await
    .unwrap();
    let docker: Arc<dyn NodeDriver> =
        Arc::new(dymini_panel_node::LocalDriver::connect(&node_dir, None));
    let agent = start_agent(identity, &data_dir, docker, Arc::new(Buffer::default()));
    panel.wait_for_status(&server_id, "online").await;
    let online = panel.server(&server_id).await;
    assert!(online["docker_version"]
        .as_str()
        .is_some_and(|v| !v.is_empty()));

    let name = format!("dymini-panel-agent-test-{}", &service_id[..8]);
    let started = std::process::Command::new("docker")
        .args([
            "run",
            "-d",
            "--rm",
            "--name",
            &name,
            "--label",
            "dymini-panel.managed=true",
            "--label",
            &format!("dymini-panel.service={service_id}"),
            "alpine:3.20",
            "sh",
            "-c",
            "echo remote-line-one; echo remote-err >&2; sleep 120",
        ])
        .output()
        .unwrap();
    assert!(started.status.success(), "{started:?}");
    let container_id = String::from_utf8(started.stdout).unwrap().trim().to_owned();
    let cleanup = || {
        let _ = std::process::Command::new("docker")
            .args(["rm", "-f", &name])
            .output();
    };

    let driver = panel.driver(&server_id).await;
    assert!(driver.container_running(&container_id).await.unwrap());

    // Output stream.
    let mut out = driver
        .exec_read(
            &container_id,
            ExecSpec {
                command: vec!["sh".into(), "-c".into(), "head -c 300000 /dev/zero".into()],
                ..ExecSpec::default()
            },
        )
        .await
        .unwrap();
    let mut total = 0;
    while let Some(chunk) = out.next().await {
        total += chunk.unwrap().len();
    }
    assert_eq!(total, 300_000);

    // Input stream.
    let payload = bytes::Bytes::from(vec![b'x'; 200_000]);
    let input: dymini_panel_core::ByteStream = Box::pin(futures_util::stream::iter(vec![
        Ok(payload.clone()),
        Ok(payload),
    ]));
    let written = driver
        .exec_write(
            &container_id,
            ExecSpec {
                command: vec!["sh".into(), "-c".into(), "wc -c".into()],
                ..ExecSpec::default()
            },
            input,
        )
        .await
        .unwrap();
    assert_eq!(written.exit_code, 0, "{}", written.output);
    assert_eq!(written.output.trim(), "400000");

    // Log streaming over the remote driver.
    let mut lines = driver.follow_logs(&container_id, 0).await.unwrap();
    let mut seen = Vec::new();
    for _ in 0..2 {
        let line = tokio::time::timeout(Duration::from_secs(10), lines.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        seen.push((line.text, line.stream));
    }
    seen.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        seen,
        [
            (
                "remote-err".to_owned(),
                dymini_panel_core::LogStreamKind::Stderr
            ),
            (
                "remote-line-one".to_owned(),
                dymini_panel_core::LogStreamKind::Stdout
            ),
        ]
    );
    drop(lines);

    // The agent's collector ships the container's lines to the service.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    let rows = loop {
        let rows: Vec<(String, Value)> = sqlx::query_as(
            "SELECT message, fields FROM log_entries WHERE service_id = $1::uuid AND server_id = $2::uuid ORDER BY id",
        )
        .bind(&service_id)
        .bind(&server_id)
        .fetch_all(&pool)
        .await
        .unwrap();
        if rows.len() >= 2 || tokio::time::Instant::now() > deadline {
            break rows;
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    };
    assert!(rows.iter().any(|r| r.0 == "remote-line-one"), "{rows:?}");
    assert!(
        rows.iter()
            .any(|r| r.0 == "remote-err" && r.1["stream"] == "stderr"),
        "{rows:?}"
    );

    // Container samples arrive with the host samples.
    let container_samples = wait_for_count(
        &pool,
        "SELECT count(*) FROM metrics_samples WHERE server_id = $1 AND service_id IS NOT NULL",
        &server_id,
    )
    .await;

    // Built images stream from the panel's engine to the server's engine.
    let panel_engine = dymini_panel_node::LocalDriver::connect(&node_dir, None);
    let tar = panel_engine.save_image("alpine:3.20").await.unwrap();
    driver.load_image(tar).await.unwrap();
    assert!(driver.image_digest("alpine:3.20").await.is_ok());

    // The interactive terminal goes browser -> panel -> agent -> Docker.
    sqlx::query("UPDATE services SET current_container_id = $1 WHERE id = $2::uuid")
        .bind(&container_id)
        .bind(&service_id)
        .execute(&pool)
        .await
        .unwrap();
    let (input_tx, input_rx) = tokio::sync::mpsc::channel(4);
    let mut terminal = driver
        .terminal(
            &container_id,
            100,
            30,
            Box::pin(tokio_stream::wrappers::ReceiverStream::new(input_rx)),
        )
        .await
        .unwrap();
    input_tx
        .send(dymini_panel_core::TerminalInput::Resize {
            cols: 120,
            rows: 40,
        })
        .await
        .unwrap();
    input_tx
        .send(dymini_panel_core::TerminalInput::Data(
            bytes::Bytes::from_static(b"echo remote-$((40+2)); stty size\n"),
        ))
        .await
        .unwrap();
    let mut output = String::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    while !(output.contains("remote-42") && output.contains("40 120")) {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        match tokio::time::timeout(remaining, terminal.next()).await {
            Ok(Some(Ok(bytes))) => output.push_str(&String::from_utf8_lossy(&bytes)),
            other => {
                cleanup();
                panic!("terminal ended early: {other:?}; output so far: {output:?}");
            }
        }
    }
    drop(input_tx);
    drop(terminal);

    cleanup();
    agent.abort();
    assert!(container_samples > 0, "no container samples were stored");
}

/// A pre-rebrand agent speaks the `rudder.agent.v1` proto package. The panel
/// serves it next to the current package so older agents keep their session —
/// and their binary update channel — until they restart onto the rebranded build.
#[sqlx::test(migrations = "./migrations")]
async fn legacy_pre_rebrand_agent_session_connects(pool: PgPool) {
    use dymini_panel_proto::legacy::{
        agent_message as legacy_agent_message, node_agent_client::NodeAgentClient as LegacyClient,
        server_message as legacy_server_message, AgentMessage as LegacyAgentMessage,
        Hello as LegacyHello,
    };

    let panel = Panel::start(pool.clone()).await;
    let created = panel
        .ok(
            "POST",
            "/api/v1/servers",
            Some(json!({ "name": "edge-legacy", "kind": "remote" })),
        )
        .await;
    let server_id = created["id"].as_str().unwrap().to_owned();
    let token = created["join_token"].as_str().unwrap().to_owned();
    let agent_dir = TempDir::new();
    let identity =
        dymini_panel_agent::enroll(&panel.http, &token, None, &agent_dir.path().join("agent"))
            .await
            .unwrap();

    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    let tls = tonic::transport::ClientTlsConfig::new()
        .ca_certificate(tonic::transport::Certificate::from_pem(&identity.ca_pem))
        .identity(tonic::transport::Identity::from_pem(
            &identity.cert_pem,
            &identity.key_pem,
        ))
        .domain_name(identity.meta.tls_name.clone());
    let channel = tonic::transport::Endpoint::from_shared(identity.meta.grpc_url.clone())
        .unwrap()
        .tls_config(tls)
        .unwrap()
        .connect()
        .await
        .unwrap();
    let hello = LegacyAgentMessage {
        body: Some(legacy_agent_message::Body::Hello(LegacyHello {
            agent_version: "0.15.0-pre-rebrand".into(),
            ..LegacyHello::default()
        })),
    };
    let (tx, rx) = tokio::sync::mpsc::channel(4);
    tx.send(hello).await.unwrap();
    let mut stream = LegacyClient::new(channel)
        .session(tokio_stream::wrappers::ReceiverStream::new(rx))
        .await
        .expect("the legacy session is accepted")
        .into_inner();
    let first = stream
        .message()
        .await
        .unwrap()
        .expect("the panel answers with a Welcome");
    let Some(legacy_server_message::Body::Welcome(welcome)) = first.body else {
        panic!("the first legacy server message is not a Welcome: {first:?}");
    };
    assert_eq!(welcome.server_id, server_id);

    // The session registers exactly like a current one: the server is online
    // and carries the agent version the legacy Hello announced.
    let status: String = sqlx::query_scalar("SELECT status FROM servers WHERE id = $1")
        .bind(server_id.parse::<Uuid>().unwrap())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(status, "online");
    let version: String = sqlx::query_scalar("SELECT agent_version FROM servers WHERE id = $1")
        .bind(server_id.parse::<Uuid>().unwrap())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(version, "0.15.0-pre-rebrand");
}
