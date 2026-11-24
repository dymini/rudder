use std::collections::HashMap;
use std::sync::Arc;

use rudder_proto::agent::{server_message, CommandResult, ServerMessage};
use serde_json::json;
use sqlx::PgPool;
use tokio::sync::mpsc;

use super::*;
use crate::driver::remote::RemoteDriver;

struct Fixture {
    state: AppState,
    environment: Uuid,
    target: Uuid,
    connections: Vec<mpsc::Receiver<Result<ServerMessage, tonic::Status>>>,
}

impl Fixture {
    async fn new(pool: PgPool) -> Self {
        let vars = HashMap::from([
            (
                "DATABASE_URL".into(),
                std::env::var("DATABASE_URL").unwrap(),
            ),
            ("RUDDER_MASTER_KEY".into(), "cc".repeat(32)),
            ("RUDDER_SETUP_TOKEN".into(), "test-setup-token".into()),
            (
                "RUDDER_DATA_DIR".into(),
                std::env::temp_dir()
                    .join(format!("rudder-build-{}", Uuid::new_v4()))
                    .display()
                    .to_string(),
            ),
        ]);
        let state = AppState::new(
            pool,
            crate::Config::from_map(&vars, |_| unreachable!()).unwrap(),
        );
        let project: Uuid =
            sqlx::query_scalar("INSERT INTO projects (name) VALUES ('builds') RETURNING id")
                .fetch_one(&state.pool)
                .await
                .unwrap();
        let environment = sqlx::query_scalar(
            "INSERT INTO environments (name, project_id) VALUES ('production', $1) RETURNING id",
        )
        .bind(project)
        .fetch_one(&state.pool)
        .await
        .unwrap();
        let target = sqlx::query_scalar(
            "INSERT INTO servers (name, kind) VALUES ('panel', 'local') RETURNING id",
        )
        .fetch_one(&state.pool)
        .await
        .unwrap();
        Self {
            state,
            environment,
            target,
            connections: Vec::new(),
        }
    }

    async fn builder(&mut self, name: &str) -> Uuid {
        let id = sqlx::query_scalar("INSERT INTO servers (name, kind, builder_enabled, builder_buildx, builder_nixpacks, arch) VALUES ($1, 'remote', TRUE, TRUE, TRUE, $2) RETURNING id")
            .bind(name).bind(std::env::consts::ARCH).fetch_one(&self.state.pool).await.unwrap();
        self.connect(id);
        id
    }

    fn connect(&mut self, id: Uuid) {
        let (tx, rx) = mpsc::channel(32);
        self.state
            .agents
            .insert(Arc::new(RemoteDriver::new(id, tx)));
        self.connections.push(rx);
    }

    async fn queued(
        &self,
        placement: &str,
        fallback: &str,
        chosen: Option<Uuid>,
    ) -> (Uuid, Uuid, Uuid) {
        let service: Uuid = sqlx::query_scalar("INSERT INTO services (name, kind, environment_id, server_id, spec, build_placement, build_fallback, build_builder_id) VALUES ($1, 'application', $2, $3, $4, $5, $6, $7) RETURNING id")
            .bind(format!("app-{}", Uuid::new_v4().simple())).bind(self.environment).bind(self.target)
            .bind(json!({"git": {"provider_id": Uuid::nil(), "repo": "https://example.com/app.git", "branch": "main", "strategy": "dockerfile"}}))
            .bind(placement).bind(fallback).bind(chosen).fetch_one(&self.state.pool).await.unwrap();
        let deployment: Uuid = sqlx::query_scalar(
            "INSERT INTO deployments (service_id, status) VALUES ($1, 'queued') RETURNING id",
        )
        .bind(service)
        .fetch_one(&self.state.pool)
        .await
        .unwrap();
        let queue = sqlx::query_scalar(
            "INSERT INTO build_queue (deployment_id, server_id) VALUES ($1, $2) RETURNING id",
        )
        .bind(deployment)
        .bind(self.target)
        .fetch_one(&self.state.pool)
        .await
        .unwrap();
        (queue, deployment, service)
    }

    async fn assigned(&self, queue: Uuid) -> Option<Uuid> {
        sqlx::query_scalar("SELECT builder_id FROM build_queue WHERE id = $1")
            .bind(queue)
            .fetch_one(&self.state.pool)
            .await
            .unwrap()
    }

    async fn finish(&self, queue: Uuid) {
        sqlx::query("UPDATE build_queue SET status = 'succeeded' WHERE id = $1")
            .bind(queue)
            .execute(&self.state.pool)
            .await
            .unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.state.config.data_dir);
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn panel_test_command_is_optional_fails_the_build_and_shares_its_deadline(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (queue_id, deployment_id, _) = f.queued("panel", "panel", None).await;
    let ticket = BuildTicket {
        queue_id,
        deployment_id,
    };
    std::fs::create_dir_all(&f.state.config.data_dir).unwrap();
    let plan = BuildPlan {
        strategy: BuildStrategy::Dockerfile,
        context: ".".into(),
        dockerfile: "Dockerfile".into(),
        publish_dir: ".".into(),
        image: "app:test".into(),
        build_args: Vec::new(),
    };
    assert_eq!(panel_commands(&plan, queue_id, None).unwrap().len(), 1);
    let commands = panel_commands(&plan, queue_id, Some("exit 7")).unwrap();
    assert_eq!(&commands[1][5..], &["app:test", "sh", "-c", "exit 7"]);
    assert!(!commands[1].iter().any(|arg| arg == "--network"));
    let shell = |script: &str| vec!["/bin/sh".into(), "-c".into(), script.to_owned()];
    for (commands, passes, tested) in [
        (vec![shell("printf built")], true, false),
        (
            vec![shell("printf built"), shell("printf tested")],
            true,
            true,
        ),
        (
            vec![shell("printf built"), shell("printf failed; exit 7")],
            false,
            true,
        ),
    ] {
        let mut logs = String::new();
        let result = run_panel_commands(
            &f.state,
            &ticket,
            &f.state.config.data_dir,
            &commands,
            &[],
            Duration::from_secs(5),
            &mut logs,
        )
        .await;
        assert_eq!(result.is_ok(), passes);
        assert_eq!(logs.contains("testing image"), tested);
    }
    let error = run_panel_commands(
        &f.state,
        &ticket,
        &f.state.config.data_dir,
        &[shell("exec sleep 300")],
        &[],
        Duration::from_millis(20),
        &mut String::new(),
    )
    .await
    .unwrap_err();
    assert!(error.contains("timed out"));
}

#[sqlx::test(migrations = "./migrations")]
async fn ranks_cache_then_free_slots_then_name_and_honors_chosen_builder(pool: PgPool) {
    let mut f = Fixture::new(pool).await;
    let a = f.builder("alpha").await;
    let b = f.builder("bravo").await;
    sqlx::query("UPDATE servers SET builder_max_jobs = 3")
        .execute(&f.state.pool)
        .await
        .unwrap();
    let busy = f.queued("builder", "wait", Some(b)).await;
    assert_eq!(claim(&f.state).await.unwrap(), Some((busy.0, busy.1)));
    let cached = f.queued("auto", "wait", None).await;
    sqlx::query("INSERT INTO build_cache_entries (builder_id, service_id, cache_key, size_bytes) VALUES ($1, $2, 'main', 12)").bind(b).bind(cached.2).execute(&f.state.pool).await.unwrap();
    assert_eq!(claim(&f.state).await.unwrap(), Some((cached.0, cached.1)));
    assert_eq!(f.assigned(cached.0).await, Some(b));
    f.finish(cached.0).await;
    let free = f.queued("auto", "wait", None).await;
    claim(&f.state).await.unwrap().unwrap();
    assert_eq!(f.assigned(free.0).await, Some(a));
    f.finish(free.0).await;
    f.finish(busy.0).await;
    let tie = f.queued("auto", "wait", None).await;
    claim(&f.state).await.unwrap().unwrap();
    assert_eq!(f.assigned(tie.0).await, Some(a));
    f.finish(tie.0).await;
    let chosen = f.queued("builder", "wait", Some(b)).await;
    claim(&f.state).await.unwrap().unwrap();
    assert_eq!(f.assigned(chosen.0).await, Some(b));
}

#[sqlx::test(migrations = "./migrations")]
async fn filters_capabilities_arch_status_and_respects_both_fallbacks(pool: PgPool) {
    let mut f = Fixture::new(pool).await;
    let b = f.builder("home").await;
    let (queue, deployment, service) = f.queued("builder", "wait", Some(b)).await;
    for predicate in [
        "builder_enabled = FALSE",
        "builder_paused = TRUE",
        "builder_draining = TRUE",
        "builder_buildx = FALSE",
        "arch = 'wrong-arch'",
    ] {
        sqlx::query("UPDATE servers SET builder_enabled = TRUE, builder_paused = FALSE, builder_draining = FALSE, builder_buildx = TRUE, arch = $1 WHERE id = $2")
            .bind(std::env::consts::ARCH).bind(b).execute(&f.state.pool).await.unwrap();
        sqlx::query(&format!("UPDATE servers SET {predicate} WHERE id = $1"))
            .bind(b)
            .execute(&f.state.pool)
            .await
            .unwrap();
        assert!(claim(&f.state).await.unwrap().is_none(), "{predicate}");
    }
    sqlx::query("UPDATE servers SET arch = $1, builder_nixpacks = FALSE WHERE id = $2")
        .bind(std::env::consts::ARCH)
        .bind(b)
        .execute(&f.state.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE services SET spec = jsonb_set(spec, '{git,strategy}', '\"nixpacks\"') WHERE id = $1").bind(service).execute(&f.state.pool).await.unwrap();
    assert!(claim(&f.state).await.unwrap().is_none());
    sqlx::query("UPDATE servers SET builder_nixpacks = TRUE WHERE id = $1")
        .bind(b)
        .execute(&f.state.pool)
        .await
        .unwrap();
    f.state.agents.disconnect(b);
    assert!(claim(&f.state).await.unwrap().is_none());
    let logs: String = sqlx::query_scalar("SELECT logs FROM deployments WHERE id = $1")
        .bind(deployment)
        .fetch_one(&f.state.pool)
        .await
        .unwrap();
    assert!(logs.contains("Waiting: no eligible builder"));
    sqlx::query("UPDATE services SET build_fallback = 'panel' WHERE id = $1")
        .bind(service)
        .execute(&f.state.pool)
        .await
        .unwrap();
    assert_eq!(claim(&f.state).await.unwrap(), Some((queue, deployment)));
    assert_eq!(f.assigned(queue).await, None);
    let logs: String = sqlx::query_scalar("SELECT logs FROM deployments WHERE id = $1")
        .bind(deployment)
        .fetch_one(&f.state.pool)
        .await
        .unwrap();
    assert!(logs.contains("Building on panel: no eligible builder; panel fallback"));
}

#[sqlx::test(migrations = "./migrations")]
async fn concurrent_claims_reserve_panel_and_builder_capacity_and_drain(pool: PgPool) {
    let mut f = Fixture::new(pool).await;
    sqlx::query("UPDATE panel_settings SET build_concurrency_default = 2")
        .execute(&f.state.pool)
        .await
        .unwrap();
    for _ in 0..5 {
        f.queued("panel", "panel", None).await;
    }
    let claims = futures_util::future::join_all((0..8).map(|_| claim(&f.state))).await;
    assert_eq!(
        claims
            .into_iter()
            .filter(|claim| claim.as_ref().unwrap().is_some())
            .count(),
        2
    );
    sqlx::query("UPDATE build_queue SET status = 'succeeded'")
        .execute(&f.state.pool)
        .await
        .unwrap();
    let builder = f.builder("home").await;
    sqlx::query("UPDATE servers SET builder_max_jobs = 2 WHERE id = $1")
        .bind(builder)
        .execute(&f.state.pool)
        .await
        .unwrap();
    for _ in 0..4 {
        f.queued("auto", "wait", None).await;
    }
    let claims = futures_util::future::join_all((0..8).map(|_| claim(&f.state))).await;
    let claimed: Vec<_> = claims.into_iter().filter_map(|r| r.unwrap()).collect();
    assert_eq!(claimed.len(), 2);
    assert_eq!(
        crate::guard::stop_builds(&f.state, "panel load guard").await,
        0
    );
    sqlx::query("UPDATE servers SET builder_draining = TRUE WHERE id = $1")
        .bind(builder)
        .execute(&f.state.pool)
        .await
        .unwrap();
    f.finish(claimed[0].0).await;
    assert!(claim(&f.state).await.unwrap().is_none());
    let running: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM build_queue WHERE builder_id = $1 AND status = 'running'",
    )
    .bind(builder)
    .fetch_one(&f.state.pool)
    .await
    .unwrap();
    assert_eq!(running, 1);
    sqlx::query("UPDATE servers SET builder_draining = FALSE WHERE id = $1")
        .bind(builder)
        .execute(&f.state.pool)
        .await
        .unwrap();
    assert!(claim(&f.state).await.unwrap().is_some());
}

#[sqlx::test(migrations = "./migrations")]
async fn cancel_sends_agent_op_and_waiting_jobs_can_be_cancelled(pool: PgPool) {
    let mut f = Fixture::new(pool).await;
    let builder = f.builder("home").await;
    let (queue, _, _) = f.queued("auto", "wait", None).await;
    claim(&f.state).await.unwrap().unwrap();
    let mut rx = f.connections.pop().unwrap();
    let session = f.state.agents.session(builder).unwrap();
    let received = tokio::spawn(async move {
        let message = rx.recv().await.unwrap().unwrap();
        let Some(server_message::Body::Command(command)) = message.body else {
            panic!("expected command")
        };
        let request: rudder_node::ops::Request =
            serde_json::from_slice(&command.payload_json).unwrap();
        assert!(
            matches!(request, rudder_node::ops::Request::BuildCancel { job_id } if job_id == queue.to_string())
        );
        session.resolve(CommandResult {
            id: command.id,
            ok: true,
            payload_json: b"true".to_vec(),
            ..Default::default()
        });
    });
    cancel(&f.state, queue).await.unwrap();
    received.await.unwrap();
    let requested: bool =
        sqlx::query_scalar("SELECT cancel_requested FROM build_queue WHERE id = $1")
            .bind(queue)
            .fetch_one(&f.state.pool)
            .await
            .unwrap();
    assert!(requested);
    let waiting = f.queued("auto", "wait", None).await;
    assert!(claim(&f.state).await.unwrap().is_none());
    cancel(&f.state, waiting.0).await.unwrap();
    let status: String = sqlx::query_scalar("SELECT status FROM deployments WHERE id = $1")
        .bind(waiting.1)
        .fetch_one(&f.state.pool)
        .await
        .unwrap();
    assert_eq!(status, "cancelled");
}

#[sqlx::test(migrations = "./migrations")]
async fn remote_target_arch_is_required_while_local_target_uses_panel_arch(pool: PgPool) {
    let mut f = Fixture::new(pool).await;
    let builder = f.builder("home").await;
    sqlx::query("UPDATE servers SET kind = 'remote', arch = NULL WHERE id = $1")
        .bind(f.target)
        .execute(&f.state.pool)
        .await
        .unwrap();
    let (queue, _, _) = f.queued("auto", "wait", None).await;
    assert!(claim(&f.state).await.unwrap().is_none());
    sqlx::query("UPDATE servers SET arch = 'alternate' WHERE id = $1 OR id = $2")
        .bind(builder)
        .bind(f.target)
        .execute(&f.state.pool)
        .await
        .unwrap();
    assert!(claim(&f.state).await.unwrap().is_some());
    assert_eq!(f.assigned(queue).await, Some(builder));
    f.finish(queue).await;
    sqlx::query("UPDATE servers SET kind = 'local' WHERE id = $1")
        .bind(f.target)
        .execute(&f.state.pool)
        .await
        .unwrap();
    f.queued("auto", "wait", None).await;
    assert!(claim(&f.state).await.unwrap().is_none());
    sqlx::query("UPDATE servers SET arch = $1 WHERE id = $2")
        .bind(std::env::consts::ARCH)
        .bind(builder)
        .execute(&f.state.pool)
        .await
        .unwrap();
    assert!(claim(&f.state).await.unwrap().is_some());
}

#[sqlx::test(migrations = "./migrations")]
async fn disconnect_requeues_once_and_reconsiders_placement(pool: PgPool) {
    let mut f = Fixture::new(pool).await;
    let a = f.builder("alpha").await;
    let b = f.builder("bravo").await;
    let (queue, deployment, service) = f.queued("auto", "wait", None).await;
    claim(&f.state).await.unwrap().unwrap();
    assert_eq!(f.assigned(queue).await, Some(a));
    f.state.agents.disconnect(a);
    assert!(
        remote::requeue_once(&f.state, queue, deployment, service, false, "log\n")
            .await
            .unwrap()
    );
    claim(&f.state).await.unwrap().unwrap();
    assert_eq!(f.assigned(queue).await, Some(b));
    assert!(
        !remote::requeue_once(&f.state, queue, deployment, service, false, "log\n")
            .await
            .unwrap()
    );
    fail_job(
        &f.state,
        queue,
        deployment,
        service,
        false,
        remote::BuildFailure::Disconnected.text(),
    )
    .await
    .unwrap();
    let error: String = sqlx::query_scalar("SELECT error FROM deployments WHERE id = $1")
        .bind(deployment)
        .fetch_one(&f.state.pool)
        .await
        .unwrap();
    assert!(error.contains("failed after one retry"));
}

#[sqlx::test(migrations = "./migrations")]
async fn cache_catalog_refresh_replaces_stale_entries_and_accepts_an_empty_cache(pool: PgPool) {
    let mut f = Fixture::new(pool).await;
    let builder = f.builder("home").await;
    let (_, _, first) = f.queued("panel", "panel", None).await;
    let (_, _, second) = f.queued("panel", "panel", None).await;
    sqlx::query("INSERT INTO build_cache_entries (builder_id, service_id, cache_key, size_bytes) VALUES ($1, $2, 'stale', 1)").bind(builder).bind(first).execute(&f.state.pool).await.unwrap();
    let entries: Vec<_> = [first, second, Uuid::new_v4()]
        .into_iter()
        .map(|service| rudder_node::ops::BuildCacheEntry {
            service_id: service.to_string(),
            cache_key: "branch".into(),
            size_bytes: 42,
            last_used_at: 1700000001000,
            created_at: 1700000000000,
        })
        .collect();
    let mut rx = f.connections.pop().unwrap();
    let session = f.state.agents.session(builder).unwrap();
    let responder = session.clone();
    let task = tokio::spawn(async move {
        for entries in [entries, Vec::new()] {
            let message = rx.recv().await.unwrap().unwrap();
            let Some(server_message::Body::Command(command)) = message.body else {
                panic!("expected command")
            };
            assert_eq!(command.op, "build_cache_usage");
            responder.resolve(CommandResult {
                id: command.id,
                ok: true,
                payload_json: serde_json::to_vec(&entries).unwrap(),
                ..Default::default()
            });
        }
    });
    remote::refresh_catalog(&f.state, &session).await.unwrap();
    let rows: Vec<(Uuid, String, i64)> = sqlx::query_as(
        "SELECT service_id, cache_key, size_bytes FROM build_cache_entries WHERE builder_id = $1",
    )
    .bind(builder)
    .fetch_all(&f.state.pool)
    .await
    .unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|r| r.1 == "branch" && r.2 == 42));
    remote::refresh_catalog(&f.state, &session).await.unwrap();
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM build_cache_entries WHERE builder_id = $1")
            .bind(builder)
            .fetch_one(&f.state.pool)
            .await
            .unwrap();
    assert_eq!(count, 0);
    task.await.unwrap();
}

struct FakeAgent {
    task: tokio::task::JoinHandle<()>,
    received: Arc<std::sync::Mutex<Vec<Vec<u8>>>>,
    requests: Arc<std::sync::Mutex<Vec<rudder_node::ops::Request>>>,
}

async fn fake_agent(
    state: &AppState,
    id: Uuid,
    url: &str,
    release: Arc<tokio::sync::Notify>,
) -> FakeAgent {
    use rudder_proto::agent::{agent_message, AgentMessage, Frame, Hello};
    use tokio_stream::wrappers::ReceiverStream;
    use tonic::transport::{Certificate, ClientTlsConfig, Identity};
    let token = Uuid::new_v4().to_string();
    sqlx::query("UPDATE servers SET join_token_hash = $2, join_token_expires_at = now() + interval '1 hour' WHERE id = $1")
        .bind(id).bind(crate::auth::sha256_hex(&token)).execute(&state.pool).await.unwrap();
    let key = rcgen::KeyPair::generate().unwrap();
    let csr = rcgen::CertificateParams::new(Vec::<String>::new())
        .unwrap()
        .serialize_request(&key)
        .unwrap()
        .pem()
        .unwrap();
    let enrolled = crate::agent::enroll(state, &token, &csr, "fake")
        .await
        .unwrap();
    let tls = ClientTlsConfig::new()
        .ca_certificate(Certificate::from_pem(enrolled.ca_pem))
        .identity(Identity::from_pem(
            enrolled.client_cert_pem,
            key.serialize_pem(),
        ))
        .domain_name(crate::agent::pki::PANEL_TLS_NAME);
    let channel = tonic::transport::Endpoint::from_shared(url.to_owned())
        .unwrap()
        .tls_config(tls)
        .unwrap()
        .connect()
        .await
        .unwrap();
    let mut client = rudder_proto::NodeAgentClient::new(channel);
    let (tx, rx) = mpsc::channel(32);
    tx.send(AgentMessage {
        body: Some(agent_message::Body::Hello(Hello {
            agent_version: rudder_core::VERSION.into(),
            arch: std::env::consts::ARCH.into(),
            builder_buildx: true,
            builder_nixpacks: true,
            ..Default::default()
        })),
    })
    .await
    .unwrap();
    let mut messages = client
        .session(ReceiverStream::new(rx))
        .await
        .unwrap()
        .into_inner();
    assert!(matches!(
        messages.message().await.unwrap().unwrap().body,
        Some(server_message::Body::Welcome(_))
    ));
    let received = Arc::new(std::sync::Mutex::new(Vec::new()));
    let requests = Arc::new(std::sync::Mutex::new(Vec::new()));
    let got = received.clone();
    let seen = requests.clone();
    let task = tokio::spawn(async move {
        let mut service = Uuid::nil().to_string();
        while let Ok(Some(message)) = messages.message().await {
            let Some(server_message::Body::Command(command)) = message.body else {
                continue;
            };
            let request: rudder_node::ops::Request =
                serde_json::from_slice(&command.payload_json).unwrap();
            seen.lock().unwrap().push(request.clone());
            let mut payload = serde_json::Value::Null;
            let mut error = String::new();
            match &request {
                rudder_node::ops::Request::LoadImage
                | rudder_node::ops::Request::ExtractTree { .. } => {
                    let first = Frame {
                        stream_id: command.stream_id.clone(),
                        ..Default::default()
                    };
                    let mut frames = client
                        .channel(tokio_stream::iter(vec![first]))
                        .await
                        .unwrap()
                        .into_inner();
                    let mut bytes = Vec::new();
                    while let Some(frame) = frames.message().await.unwrap() {
                        bytes.extend(frame.data);
                        error = frame.error;
                        if frame.eof {
                            break;
                        }
                    }
                    got.lock().unwrap().push(bytes);
                }
                rudder_node::ops::Request::BuildCacheUsage { .. } => {
                    payload = json!([rudder_node::ops::BuildCacheEntry {
                        service_id: service.clone(),
                        cache_key: "branch".into(),
                        size_bytes: 42,
                        last_used_at: 1700000001000,
                        created_at: 1700000000000
                    }]);
                }
                rudder_node::ops::Request::BuildCancel { .. } => payload = json!(true),
                rudder_node::ops::Request::BuildStart { request } => {
                    service = request.service_id.clone()
                }
                _ => {}
            }
            tx.send(AgentMessage {
                body: Some(agent_message::Body::Result(CommandResult {
                    id: command.id,
                    ok: error.is_empty(),
                    error,
                    payload_json: serde_json::to_vec(&payload).unwrap(),
                })),
            })
            .await
            .unwrap();
            if matches!(
                request,
                rudder_node::ops::Request::SaveImage { .. }
                    | rudder_node::ops::Request::BuildStart { .. }
            ) {
                let mut client = client.clone();
                let stream_id = command.stream_id;
                let release = release.clone();
                tokio::spawn(async move {
                    let (frames, input) = mpsc::channel(8);
                    let first = match &request {
                        rudder_node::ops::Request::BuildStart { .. } => {
                            serde_json::to_vec(&rudder_node::ops::BuildEvent::Log {
                                stderr: false,
                                text: "live log secret-build-value\n".into(),
                            })
                            .unwrap()
                        }
                        rudder_node::ops::Request::SaveImage { image } if image == "empty" => {
                            Vec::new()
                        }
                        _ => b"fake image archive".to_vec(),
                    };
                    frames
                        .send(Frame {
                            stream_id: stream_id.clone(),
                            data: first,
                            ..Default::default()
                        })
                        .await
                        .unwrap();
                    let response = client.channel(ReceiverStream::new(input)).await.unwrap();
                    if matches!(request, rudder_node::ops::Request::BuildStart { .. }) {
                        release.notified().await;
                        let event = rudder_node::ops::BuildEvent::Finished {
                            status: rudder_node::ops::BuildStatus::Succeeded,
                            error: None,
                            cache: Vec::new(),
                        };
                        frames
                            .send(Frame {
                                stream_id: stream_id.clone(),
                                data: serde_json::to_vec(&event).unwrap(),
                                ..Default::default()
                            })
                            .await
                            .unwrap();
                    }
                    let error = if matches!(request, rudder_node::ops::Request::SaveImage { ref image } if image == "broken")
                    {
                        "image export failed"
                    } else {
                        ""
                    };
                    let _ = frames
                        .send(Frame {
                            stream_id,
                            eof: true,
                            error: error.into(),
                            ..Default::default()
                        })
                        .await;
                    drop(frames);
                    let mut output = response.into_inner();
                    while let Ok(Some(_)) = output.message().await {}
                });
            }
        }
    });
    FakeAgent {
        task,
        received,
        requests,
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn fake_agents_relay_images_and_stream_builder_logs_before_finish(pool: PgPool) {
    let mut f = Fixture::new(pool).await;
    crate::agent::install_crypto_provider();
    crate::agent::pki::AgentCa::load_or_create(&f.state.pool, &f.state.cipher)
        .await
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("https://{}", listener.local_addr().unwrap());
    let state = f.state.clone();
    let server = tokio::spawn(crate::agent::serve(state, listener));
    let builder = f.builder("builder").await;
    let target = f.builder("target").await;
    let release = Arc::new(tokio::sync::Notify::new());
    let from = fake_agent(&f.state, builder, &url, release.clone()).await;
    let to = fake_agent(&f.state, target, &url, Arc::new(tokio::sync::Notify::new())).await;
    let driver = f.state.agents.driver(builder).unwrap();
    let target_driver = f.state.agents.driver(target).unwrap();
    crate::deploy::copy_image(driver.as_ref(), target_driver.as_ref(), "image:tag")
        .await
        .unwrap();
    assert_eq!(to.received.lock().unwrap()[0], b"fake image archive");
    crate::deploy::copy_image(driver.as_ref(), target_driver.as_ref(), "empty")
        .await
        .unwrap();
    assert!(to.received.lock().unwrap()[1].is_empty());
    assert!(
        crate::deploy::copy_image(driver.as_ref(), target_driver.as_ref(), "broken")
            .await
            .is_err()
    );

    f.target = target;
    let (queue, deployment, service) = f.queued("builder", "wait", Some(builder)).await;
    claim(&f.state).await.unwrap().unwrap();
    let settings = remote::settings(&f.state, queue).await.unwrap();
    let source = f.state.config.data_dir.join("source");
    let checkout = f.state.config.data_dir.join("checkout");
    std::fs::create_dir_all(&source).unwrap();
    let git = |args: &[&str]| {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(&source)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    git(&["init", "--initial-branch=main"]);
    std::fs::write(source.join("Dockerfile"), "FROM scratch\n").unwrap();
    git(&["add", "Dockerfile"]);
    git(&[
        "-c",
        "user.name=Test",
        "-c",
        "user.email=test@example.com",
        "commit",
        "-m",
        "source",
    ]);
    git(&[
        "clone",
        source.to_str().unwrap(),
        checkout.to_str().unwrap(),
    ]);
    let state = f.state.clone();
    let build = tokio::spawn(async move {
        let source = GitSource {
            provider_id: Uuid::nil(),
            repo: "https://example.com/app.git".into(),
            branch: "feature".into(),
            strategy: BuildStrategy::Dockerfile,
            context: ".".into(),
            dockerfile: "Dockerfile".into(),
            publish_dir: ".".into(),
        };
        let plan = BuildPlan {
            strategy: BuildStrategy::Dockerfile,
            context: ".".into(),
            dockerfile: "Dockerfile".into(),
            publish_dir: ".".into(),
            image: "app:built".into(),
            build_args: vec![("TOKEN".into(), "secret-build-value".into())],
        };
        let mut logs = String::new();
        let result = remote::execute(
            &state,
            remote::RemoteBuild {
                queue,
                deployment,
                service,
                source: &source,
                repo: &checkout,
                env: &[],
                plan: &plan,
                settings: &settings,
                secrets: &["secret-build-value"],
            },
            &mut logs,
        )
        .await;
        (result, logs)
    });
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let logs: String = sqlx::query_scalar("SELECT logs FROM deployments WHERE id = $1")
                .bind(deployment)
                .fetch_one(&f.state.pool)
                .await
                .unwrap();
            assert!(!logs.contains("secret-build-value"));
            if logs.contains("live log ***") {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(!build.is_finished());
    release.notify_one();
    let (result, logs) = build.await.unwrap();
    result.unwrap();
    assert!(!logs.contains("secret-build-value"));
    assert_eq!(
        to.received.lock().unwrap().last().unwrap(),
        b"fake image archive"
    );
    assert!(from.requests.lock().unwrap().iter().any(|request| matches!(request, rudder_node::ops::Request::RemoveImage { image } if image == "app:built")));
    assert!(from.requests.lock().unwrap().iter().any(|request| matches!(request, rudder_node::ops::Request::BuildStart { request } if request.cache_fallback_keys.len() == 1 && request.cache_key != request.cache_fallback_keys[0])));
    let cached: i64 = sqlx::query_scalar(
        "SELECT size_bytes FROM build_cache_entries WHERE builder_id = $1 AND service_id = $2",
    )
    .bind(builder)
    .bind(service)
    .fetch_one(&f.state.pool)
    .await
    .unwrap();
    assert_eq!(cached, 42);
    let _ = driver.remove_image("app:built").await;
    from.task.abort();
    to.task.abort();
    server.abort();
}
