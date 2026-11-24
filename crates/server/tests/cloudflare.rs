//! The optional Cloudflare Zero Trust mode against a fake Cloudflare API: saving
//! credentials, the tunnel, publishing hostnames and DNS, Access in front of the
//! panel, SSH through Access for Infrastructure, the readiness report, and
//! removing everything again.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Body;
use axum::extract::{Path, Query, Request as AxumRequest, State};
use axum::http::{Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::Json;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

const ACCOUNT: &str = "0123456789abcdef0123456789abcdef";
const ZONE: &str = "fedcba9876543210fedcba9876543210";
const OTHER_ZONE: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const TOKEN: &str = "cfapitoken_1234567890";
const TUNNEL: &str = "4f1c5f1e-2f64-4a5b-9d0f-3a8e7d9c1b22";
const CA_KEY: &str =
    "ecdsa-sha2-nistp256 AAAAE2VjZHNhLXNoYTItbmlzdHAyNTYAAAAIbmlzdHAyNTYAAABBBExample";

#[derive(Default)]
struct Fake {
    failed_dns_zone: Option<String>,
    zones: Option<Vec<Value>>,
    fail_discovery: bool,
    zone_page_size: usize,
    fail_config: bool,
    fail_releases: bool,
    dns: Vec<Value>,
    configs: Vec<Value>,
    apps: HashMap<String, Value>,
    calls: Vec<String>,
    next: u32,
}

type Shared = Arc<Mutex<Fake>>;

fn ok(result: Value) -> Json<Value> {
    Json(json!({ "success": true, "errors": [], "result": result }))
}

fn record(fake: &Shared, call: impl Into<String>) {
    fake.lock().unwrap().calls.push(call.into());
}

async fn authenticate(State(fake): State<Shared>, request: AxumRequest, next: Next) -> Response {
    let header = request
        .headers()
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_owned();
    let path = request.uri().path().to_owned();
    record(&fake, format!("{} {}", request.method(), path));
    if path.starts_with("/repos/cloudflare/cloudflared/") {
        return next.run(request).await;
    }
    if header == "Bearer denied" && path.contains("/cfd_tunnel") {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({
                "success": false,
                "errors": [{ "code": 10000, "message": "Authentication error" }],
            })),
        )
            .into_response();
    }
    if header != format!("Bearer {TOKEN}") && header != "Bearer denied" {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "success": false,
                "errors": [{ "code": 9109, "message": "Invalid access token" }],
            })),
        )
            .into_response();
    }
    next.run(request).await
}

async fn zone(Path(id): Path<String>) -> Json<Value> {
    let account = if id == OTHER_ZONE {
        "ffffffffffffffffffffffffffffffff"
    } else {
        ACCOUNT
    };
    ok(json!({ "id": id, "name": "example.com", "account": { "id": account } }))
}

async fn empty_list() -> Json<Value> {
    ok(json!([]))
}

async fn list_zones(
    State(fake): State<Shared>,
    Query(query): Query<HashMap<String, String>>,
) -> Json<Value> {
    let mut fake = fake.lock().unwrap();
    if fake.fail_discovery {
        return Json(
            json!({"success":false,"errors":[{"code":10000,"message":"Zone Read permission denied"}]}),
        );
    }
    let page: usize = query.get("page").unwrap().parse().unwrap();
    assert_eq!(query.get("account.id").unwrap(), ACCOUNT);
    fake.calls.push(format!("zones page {page}"));
    let zones = fake.zones.clone().unwrap_or_else(|| {
        vec![json!({"id":ZONE,"name":"example.com","status":"active","account":{"id":ACCOUNT}})]
    });
    let size = if fake.zone_page_size == 0 {
        50
    } else {
        fake.zone_page_size
    };
    let rows: Vec<_> = zones
        .iter()
        .skip((page - 1) * size)
        .take(size)
        .cloned()
        .collect();
    Json(
        json!({"success":true,"result":rows,"result_info":{"total_pages":zones.len().div_ceil(size).max(1)}}),
    )
}

async fn create_tunnel(State(fake): State<Shared>, Json(body): Json<Value>) -> Json<Value> {
    record(&fake, format!("tunnel {}", body["config_src"]));
    ok(json!({ "id": TUNNEL, "name": body["name"] }))
}

async fn tunnel_token() -> Json<Value> {
    ok(json!("dGVzdC10dW5uZWwtdG9rZW4="))
}

async fn put_config(State(fake): State<Shared>, Json(body): Json<Value>) -> Json<Value> {
    if fake.lock().unwrap().fail_config {
        return Json(
            json!({"success":false,"errors":[{"code":10000,"message":"configuration unavailable"}],"result":null}),
        );
    }
    fake.lock().unwrap().configs.push(body.clone());
    ok(body)
}

async fn connections() -> Json<Value> {
    ok(json!([{ "id": "a", "conns": [{ "id": "1" }, { "id": "2" }] }]))
}

async fn nothing() -> Json<Value> {
    ok(Value::Null)
}

async fn releases(State(fake): State<Shared>) -> (StatusCode, Json<Value>) {
    if fake.lock().unwrap().fail_releases {
        return (
            StatusCode::BAD_GATEWAY,
            Json(json!({"message":"upstream unavailable"})),
        );
    }
    (
        StatusCode::OK,
        Json(json!([
            {"tag_name":"2026.9.0"},
            {"tag_name":"2026.11.0", "html_url":"https://github.com/cloudflare/cloudflared/releases/tag/2026.11.0"},
            {"tag_name":"2027.1.0", "prerelease":true}
        ])),
    )
}

async fn list_dns(
    State(fake): State<Shared>,
    Path(zone): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> Json<Value> {
    let name = query.get("name").cloned().unwrap_or_default();
    if fake.lock().unwrap().failed_dns_zone.as_deref() == Some(&zone) {
        return Json(
            json!({"success":false,"errors":[{"code":10000,"message":"DNS permission denied"}]}),
        );
    }
    let found: Vec<Value> = fake
        .lock()
        .unwrap()
        .dns
        .iter()
        .filter(|record| {
            record["name"] == name
                && record.get("_zone").and_then(Value::as_str).unwrap_or(ZONE) == zone
        })
        .cloned()
        .collect();
    ok(Value::Array(found))
}

async fn create_dns(
    State(fake): State<Shared>,
    Path(zone): Path<String>,
    Json(mut body): Json<Value>,
) -> Json<Value> {
    let mut fake = fake.lock().unwrap();
    fake.next += 1;
    body["id"] = json!(format!("dns-{}", fake.next));
    body["_zone"] = json!(zone);
    fake.dns.push(body.clone());
    ok(body)
}

async fn delete_dns(
    State(fake): State<Shared>,
    Path((zone, id)): Path<(String, String)>,
) -> Json<Value> {
    fake.lock().unwrap().dns.retain(|record| {
        record["id"] != id || record.get("_zone").and_then(Value::as_str).unwrap_or(ZONE) != zone
    });
    ok(json!({ "id": id }))
}

async fn update_dns(
    State(fake): State<Shared>,
    Path((zone, id)): Path<(String, String)>,
    Json(mut body): Json<Value>,
) -> Json<Value> {
    body["id"] = json!(id);
    body["_zone"] = json!(zone);
    let mut fake = fake.lock().unwrap();
    let index = fake
        .dns
        .iter()
        .position(|r| {
            r["id"] == id && r.get("_zone").and_then(Value::as_str).unwrap_or(ZONE) == zone
        })
        .unwrap();
    fake.dns[index] = body.clone();
    ok(body)
}

#[sqlx::test(migrations = "./migrations")]
async fn domain_routing_migration_preserves_existing_tunnels_and_direct_defaults(pool: PgPool) {
    let mut tx = pool.begin().await.unwrap();
    sqlx::raw_sql("SET LOCAL search_path TO pg_temp; CREATE TEMP TABLE domains(id int, service_id int); CREATE TEMP TABLE services(id int, server_id int); CREATE TEMP TABLE servers(id int, kind text); CREATE TEMP TABLE cloudflare_settings(tunnel_id text, tunnel_published bool); INSERT INTO servers VALUES(1,'local'),(2,'agent'); INSERT INTO services VALUES(1,1),(2,2); INSERT INTO domains VALUES(1,1),(2,2); INSERT INTO cloudflare_settings VALUES('tunnel',true);")
        .execute(&mut *tx).await.unwrap();
    sqlx::raw_sql(include_str!("../migrations/0019_domain_routing.sql"))
        .execute(&mut *tx)
        .await
        .unwrap();
    let modes: Vec<String> = sqlx::query_scalar("SELECT routing_mode FROM domains ORDER BY id")
        .fetch_all(&mut *tx)
        .await
        .unwrap();
    assert_eq!(modes, vec!["cloudflare", "direct"]);
    sqlx::query("INSERT INTO domains(id,service_id) VALUES(3,1)")
        .execute(&mut *tx)
        .await
        .unwrap();
    let mode: String = sqlx::query_scalar("SELECT routing_mode FROM domains WHERE id=3")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(mode, "direct");
    tx.rollback().await.unwrap();
}

async fn get_dns(
    State(fake): State<Shared>,
    Path((zone, id)): Path<(String, String)>,
) -> Json<Value> {
    let record = fake
        .lock()
        .unwrap()
        .dns
        .iter()
        .find(|r| r["id"] == id && r.get("_zone").and_then(Value::as_str).unwrap_or(ZONE) == zone)
        .cloned()
        .unwrap_or(json!({}));
    ok(record)
}

async fn create_app(State(fake): State<Shared>, Json(body): Json<Value>) -> Json<Value> {
    let id = Uuid::new_v4().to_string();
    let mut stored = body.clone();
    stored["id"] = json!(id);
    stored["aud"] = json!(format!("aud-{}", &id[..8]));
    fake.lock().unwrap().apps.insert(id, stored.clone());
    ok(stored)
}

async fn update_app(
    State(fake): State<Shared>,
    Path((_, id)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> Json<Value> {
    let mut fake = fake.lock().unwrap();
    let aud = fake.apps.get(&id).map(|app| app["aud"].clone());
    let mut stored = body;
    stored["id"] = json!(id);
    stored["aud"] = aud.unwrap_or(json!("aud-new"));
    fake.apps.insert(id, stored.clone());
    ok(stored)
}

async fn delete_app(
    State(fake): State<Shared>,
    Path((_, id)): Path<(String, String)>,
) -> Json<Value> {
    fake.lock().unwrap().apps.remove(&id);
    ok(json!({ "id": id }))
}

/// The real API reports an account without an SSH CA as an error, not an empty list.
async fn gateway_ca_list() -> (StatusCode, Json<Value>) {
    (
        StatusCode::NOT_FOUND,
        Json(json!({
            "success": false,
            "errors": [{ "code": 12112, "message": "access.api.error.gateway_ca_not_found" }],
            "result": null
        })),
    )
}

async fn gateway_ca_create() -> Json<Value> {
    ok(json!({ "id": "ca", "public_key": CA_KEY }))
}

async fn virtual_networks() -> Json<Value> {
    ok(json!([
        { "id": "11111111-1111-1111-1111-111111111111", "is_default_network": false },
        { "id": "22222222-2222-2222-2222-222222222222", "is_default_network": true },
    ]))
}

async fn create_route(State(fake): State<Shared>, Json(body): Json<Value>) -> Json<Value> {
    record(
        &fake,
        format!(
            "route {} via {} on {}",
            body["network"], body["tunnel_id"], body["virtual_network_id"]
        ),
    );
    ok(body)
}

async fn delete_route(
    State(fake): State<Shared>,
    Path((_, cidr)): Path<(String, String)>,
) -> Json<Value> {
    record(&fake, format!("route deleted {cidr}"));
    ok(Value::Null)
}

async fn create_target(State(fake): State<Shared>, Json(body): Json<Value>) -> Json<Value> {
    record(&fake, format!("target {}", body["hostname"]));
    ok(json!({ "id": "target-1", "hostname": body["hostname"] }))
}

async fn delete_target(
    State(fake): State<Shared>,
    Path((_, id)): Path<(String, String)>,
) -> Json<Value> {
    record(&fake, format!("target deleted {id}"));
    ok(Value::Null)
}

async fn fake_cloudflare() -> (String, Shared) {
    let fake = Shared::default();
    let router = axum::Router::new()
        .route("/zones", get(list_zones))
        .route("/zones/{id}", get(zone))
        .route("/zones/{id}/dns_records", get(list_dns).post(create_dns))
        .route(
            "/zones/{id}/dns_records/{record}",
            get(get_dns).put(update_dns).delete(delete_dns),
        )
        .route(
            "/accounts/{account}/cfd_tunnel",
            get(empty_list).post(create_tunnel),
        )
        .route("/accounts/{account}/cfd_tunnel/{id}", delete(nothing))
        .route("/repos/cloudflare/cloudflared/releases", get(releases))
        .route(
            "/accounts/{account}/cfd_tunnel/{id}/token",
            get(tunnel_token),
        )
        .route(
            "/accounts/{account}/cfd_tunnel/{id}/configurations",
            put(put_config),
        )
        .route(
            "/accounts/{account}/cfd_tunnel/{id}/connections",
            get(connections).delete(nothing),
        )
        .route(
            "/accounts/{account}/access/apps",
            get(empty_list).post(create_app),
        )
        .route(
            "/accounts/{account}/access/apps/{id}",
            put(update_app).delete(delete_app),
        )
        .route(
            "/accounts/{account}/access/gateway_ca",
            get(gateway_ca_list).post(gateway_ca_create),
        )
        .route(
            "/accounts/{account}/teamnet/virtual_networks",
            get(virtual_networks),
        )
        .route("/accounts/{account}/teamnet/routes", post(create_route))
        .route(
            "/accounts/{account}/teamnet/routes/network/{cidr}",
            delete(delete_route),
        )
        .route(
            "/accounts/{account}/infrastructure/targets",
            post(create_target),
        )
        .route(
            "/accounts/{account}/infrastructure/targets/{id}",
            delete(delete_target),
        )
        .layer(axum::middleware::from_fn_with_state(
            fake.clone(),
            authenticate,
        ))
        .with_state(fake.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    (url, fake)
}

fn app(pool: PgPool, data_dir: &std::path::Path, cloudflare: &str) -> axum::Router {
    let url =
        std::env::var("DATABASE_URL").unwrap_or_else(|_| "postgres://localhost/rudder".into());
    let vars = HashMap::from([
        ("DATABASE_URL".into(), url),
        ("RUDDER_MASTER_KEY".into(), "cc".repeat(32)),
        ("RUDDER_SETUP_TOKEN".into(), "test-setup-token".into()),
        ("RUDDER_COOKIE_SECURE".into(), "false".into()),
        ("RUDDER_DATA_DIR".into(), data_dir.display().to_string()),
        ("RUDDER_CLOUDFLARE_API_URL".into(), cloudflare.to_owned()),
        ("RUDDER_GITHUB_API_URL".into(), cloudflare.to_owned()),
        // These assertions describe a host without Docker, even when one is running.
        (
            "RUDDER_DOCKER_SOCKET".into(),
            data_dir.join("no-docker.sock").display().to_string(),
        ),
    ]);
    let config = rudder_server::Config::from_map(&vars, |_| unreachable!()).unwrap();
    rudder_server::router(rudder_server::AppState::new(pool.clone(), config)).layer(
        axum::middleware::from_fn_with_state(pool, finish_mutation_request),
    )
}

async fn wait_for_mutation_rollbacks(pool: &PgPool) {
    tokio::time::timeout(Duration::from_secs(10), async {
        let mut guard = pool.begin().await.unwrap();
        for key in [77_302_461_i64, 77_302_462] {
            sqlx::query("SELECT pg_advisory_xact_lock($1)")
                .bind(key)
                .execute(&mut *guard)
                .await
                .unwrap();
        }
        guard.rollback().await.unwrap();
    })
    .await
    .expect("Cloudflare mutation transactions must finish rolling back");
}

async fn finish_mutation_request(
    State(pool): State<PgPool>,
    request: AxumRequest,
    next: Next,
) -> Response {
    let mutation = !matches!(request.method().as_str(), "GET" | "HEAD" | "OPTIONS");
    let held_lock = request.extensions().get::<HeldMutationLock>().is_some();
    let response = next.run(request).await;
    // SQLx queues rollback on drop; wait for PostgreSQL before the next request.
    // Do not wait for deliberately held locks or accepted background updates.
    if mutation && !held_lock && response.status() != StatusCode::ACCEPTED {
        wait_for_mutation_rollbacks(&pool).await;
    }
    response
}

#[derive(Clone, Copy)]
struct HeldMutationLock;

fn holding_mutation_lock(app: &axum::Router) -> axum::Router {
    app.clone().layer(axum::middleware::from_fn(
        |mut request: AxumRequest, next: Next| async move {
            request.extensions_mut().insert(HeldMutationLock);
            next.run(request).await
        },
    ))
}

struct Reply {
    status: StatusCode,
    body: Value,
}

async fn call_with(
    app: &axum::Router,
    method: &str,
    path: &str,
    session: Option<&(String, String)>,
    body: Option<Value>,
    host: Option<&str>,
) -> Reply {
    let mut builder = Request::builder().method(method).uri(path);
    if let Some((cookie, csrf)) = session {
        builder = builder
            .header("cookie", format!("rudder_session={cookie}"))
            .header("x-csrf-token", csrf);
    }
    if let Some(host) = host {
        builder = builder.header("host", host);
    }
    let request = match body {
        Some(body) => builder
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    };
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    Reply {
        status,
        body: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    }
}

async fn call(
    app: &axum::Router,
    method: &str,
    path: &str,
    session: &(String, String),
    body: Option<Value>,
) -> Reply {
    call_with(app, method, path, Some(session), body, None).await
}

async fn owner(app: &axum::Router) -> (String, String) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/setup")
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
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .and_then(|pair| pair.strip_prefix("rudder_session="))
        .unwrap()
        .to_owned();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    (cookie, body["csrf_token"].as_str().unwrap().to_owned())
}

fn scratch_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("rudder-cloudflare-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn calls(fake: &Shared) -> Vec<String> {
    fake.lock().unwrap().calls.clone()
}

#[sqlx::test(migrations = "./migrations")]
async fn connector_status_caches_versions_and_serializes_updates(pool: PgPool) {
    let (cloudflare, fake) = fake_cloudflare().await;
    let data_dir = scratch_dir();
    let app = app(pool.clone(), &data_dir, &cloudflare);
    let session = owner(&app).await;
    let status = call(
        &app,
        "GET",
        "/api/v1/cloudflare/tunnel/status",
        &session,
        None,
    )
    .await;
    assert_eq!(status.status, StatusCode::OK, "{}", status.body);
    assert_eq!(status.body["connections"], json!([]));
    assert_eq!(status.body["replicas"], json!([]));
    assert_eq!(status.body["update_available"], false);
    assert_eq!(status.body["latest_release"]["version"], "2026.11.0");
    assert!(status.body["replica_error"].is_string());
    call(
        &app,
        "GET",
        "/api/v1/cloudflare/tunnel/status",
        &session,
        None,
    )
    .await;
    assert_eq!(
        calls(&fake)
            .iter()
            .filter(|s| s.contains("/repos/cloudflare/cloudflared/releases"))
            .count(),
        1
    );
    call(
        &app,
        "POST",
        "/api/v1/cloudflare/tunnel/check",
        &session,
        None,
    )
    .await;
    assert_eq!(
        calls(&fake)
            .iter()
            .filter(|s| s.contains("/repos/cloudflare/cloudflared/releases"))
            .count(),
        2
    );
    fake.lock().unwrap().fail_releases = true;
    let failed_check = call(
        &app,
        "POST",
        "/api/v1/cloudflare/tunnel/check",
        &session,
        None,
    )
    .await;
    assert!(failed_check.body["check_error"].is_string());
    assert_eq!(failed_check.body["latest_release"]["version"], "2026.11.0");
    fake.lock().unwrap().fail_releases = false;
    let credentials = call(
        &app,
        "PUT",
        "/api/v1/cloudflare/credentials",
        &session,
        Some(json!({"account_id":ACCOUNT,"zone_id":ZONE,"api_token":TOKEN})),
    )
    .await;
    assert_eq!(credentials.status, StatusCode::OK, "{}", credentials.body);
    let created = call(
        &app,
        "POST",
        "/api/v1/cloudflare/tunnel",
        &session,
        Some(json!({"replicas":1})),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let status = call(
        &app,
        "GET",
        "/api/v1/cloudflare/tunnel/status",
        &session,
        None,
    )
    .await;
    assert_eq!(status.body["connections"].as_array().unwrap().len(), 2);
    assert_eq!(status.body["pinned_version"], "2026.11.0");
    assert!(!status.body.to_string().contains(TOKEN));
    let invalid = call(
        &app,
        "POST",
        "/api/v1/cloudflare/tunnel/update",
        &session,
        Some(json!({"version":"latest"})),
    )
    .await;
    assert_eq!(invalid.status, StatusCode::BAD_REQUEST);
    let contended = holding_mutation_lock(&app);
    let mut guard = pool.begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(77302461)")
        .execute(&mut *guard)
        .await
        .unwrap();
    let busy = call(
        &contended,
        "POST",
        "/api/v1/cloudflare/tunnel/update",
        &session,
        Some(json!({})),
    )
    .await;
    assert_eq!(busy.status, StatusCode::CONFLICT);
    let scale = call(
        &contended,
        "PUT",
        "/api/v1/cloudflare/tunnel/replicas",
        &session,
        Some(json!({"replicas":2})),
    )
    .await;
    assert_eq!(scale.status, StatusCode::CONFLICT);
    guard.rollback().await.unwrap();
    let accepted = call(
        &app,
        "POST",
        "/api/v1/cloudflare/tunnel/update",
        &session,
        Some(json!({"version":"2026.10.0"})),
    )
    .await;
    assert_eq!(accepted.status, StatusCode::ACCEPTED, "{}", accepted.body);
    let mut done = false;
    for _ in 0..30 {
        let status = call(
            &app,
            "GET",
            "/api/v1/cloudflare/tunnel/status",
            &session,
            None,
        )
        .await;
        if status.body["update"]["status"] == "failed" {
            assert_eq!(status.body["pinned_version"], "2026.11.0");
            assert_eq!(status.body["update"]["completed"], 0);
            done = true;
            break;
        }
        tokio::task::yield_now().await;
    }
    assert!(
        done,
        "Docker-less update must report a failure, without changing the pin"
    );
    std::fs::remove_dir_all(data_dir).unwrap();
}

#[sqlx::test(migrations = "./migrations")]
async fn domain_routing_is_opt_in_and_dns_cleanup_respects_ownership(pool: PgPool) {
    let (cloudflare, fake) = fake_cloudflare().await;
    let data_dir = scratch_dir();
    let app = app(pool.clone(), &data_dir, &cloudflare);
    let session = owner(&app).await;
    let empty = call(&app, "GET", "/api/v1/domains", &session, None).await;
    assert_eq!(empty.status, StatusCode::OK);
    assert_eq!(empty.body, json!([]));
    let server = call(
        &app,
        "POST",
        "/api/v1/servers",
        &session,
        Some(json!({"name":"local","kind":"local","public_ip":"203.0.113.10"})),
    )
    .await;
    let project = call(
        &app,
        "POST",
        "/api/v1/projects",
        &session,
        Some(json!({"name":"web"})),
    )
    .await;
    let env = call(
        &app,
        "POST",
        &format!(
            "/api/v1/projects/{}/environments",
            project.body["id"].as_str().unwrap()
        ),
        &session,
        Some(json!({"name":"prod"})),
    )
    .await;
    let svc = call(&app,"POST",&format!("/api/v1/environments/{}/services",env.body["id"].as_str().unwrap()),&session,Some(json!({"name":"web","kind":"application","server_id":server.body["id"],"spec":{"image":"nginx:1.27"}}))).await;
    let path = format!(
        "/api/v1/services/{}/domains",
        svc.body["id"].as_str().unwrap()
    );
    let body = json!({"host":"managed.example.com","target_port":80,"ssl_mode":"none","routing_mode":"cloudflare"});
    let missing = call(&app, "POST", &path, &session, Some(body.clone())).await;
    assert_eq!(missing.status, StatusCode::BAD_REQUEST, "{}", missing.body);
    assert!(missing.body.to_string().contains("credentials"));
    let credentials = call(
        &app,
        "PUT",
        "/api/v1/cloudflare/credentials",
        &session,
        Some(json!({"account_id":ACCOUNT,"zone_id":ZONE,"api_token":TOKEN})),
    )
    .await;
    assert_eq!(credentials.status, StatusCode::OK);
    let mut wrong = body.clone();
    wrong["host"] = json!("outside.other.org");
    let rejected = call(&app, "POST", &path, &session, Some(wrong)).await;
    assert_eq!(rejected.status, StatusCode::BAD_REQUEST);
    assert!(rejected.body.to_string().contains("example.com"));
    sqlx::query("UPDATE cloudflare_settings SET tunnel_id=$1,tunnel_published=false WHERE id=TRUE")
        .bind(TUNNEL)
        .execute(&pool)
        .await
        .unwrap();
    let created = call(&app, "POST", &path, &session, Some(body.clone())).await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    assert_eq!(created.body["routing_mode"], "cloudflare");
    assert!(fake
        .lock()
        .unwrap()
        .dns
        .iter()
        .any(|r| r["name"] == "managed.example.com" && r["type"] == "A" && r["proxied"] == true));
    let list = call(&app, "GET", "/api/v1/domains", &session, None).await;
    assert_eq!(list.status, StatusCode::OK, "{}", list.body);
    assert_eq!(list.body[0]["routing"], "cloudflare-proxied");
    let domain_path = format!("/api/v1/domains/{}", created.body["id"].as_str().unwrap());
    let deleted = call(&app, "DELETE", &domain_path, &session, None).await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT);
    assert!(fake.lock().unwrap().dns.is_empty());
    sqlx::query("UPDATE cloudflare_settings SET tunnel_id=NULL WHERE id=TRUE")
        .execute(&pool)
        .await
        .unwrap();
    fake.lock().unwrap().dns.push(
        json!({"id":"foreign","type":"A","name":"managed.example.com","content":"192.0.2.99"}),
    );
    let rejected = call(&app, "POST", &path, &session, Some(body.clone())).await;
    assert_eq!(rejected.status, StatusCode::CONFLICT, "{}", rejected.body);
    assert_eq!(fake.lock().unwrap().dns[0]["id"], "foreign");
    let mut overwrite = body.clone();
    overwrite["overwrite_dns"] = json!(true);
    let created = call(&app, "POST", &path, &session, Some(overwrite)).await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let tunnel = call(
        &app,
        "POST",
        "/api/v1/cloudflare/tunnel",
        &session,
        Some(json!({})),
    )
    .await;
    assert_eq!(tunnel.status, StatusCode::CREATED, "{}", tunnel.body);
    assert!(fake
        .lock()
        .unwrap()
        .dns
        .iter()
        .any(|r| r["name"] == "managed.example.com" && r["type"] == "CNAME"));
    let domain_path = format!("/api/v1/domains/{}", created.body["id"].as_str().unwrap());
    let direct = call(
        &app,
        "PATCH",
        &domain_path,
        &session,
        Some(json!({"routing_mode":"direct"})),
    )
    .await;
    assert_eq!(direct.status, StatusCode::OK, "{}", direct.body);
    assert!(direct.body["routing_options"]["direct_warning"]
        .as_str()
        .unwrap()
        .contains("will not be reachable"));
    assert!(fake.lock().unwrap().dns.is_empty());
    let mut direct_body = body;
    direct_body["host"] = json!("new-direct.example.com");
    direct_body.as_object_mut().unwrap().remove("routing_mode");
    let direct = call(&app, "POST", &path, &session, Some(direct_body)).await;
    assert_eq!(direct.status, StatusCode::CREATED, "{}", direct.body);
    assert_eq!(direct.body["routing_mode"], "direct");
    assert!(fake.lock().unwrap().dns.is_empty());
    sqlx::query("UPDATE panel_settings SET panel_domain='panel.example.com' WHERE id=TRUE")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE access_settings SET portal_host='access.example.com' WHERE id=TRUE")
        .execute(&pool)
        .await
        .unwrap();
    let list = call(&app, "GET", "/api/v1/domains", &session, None).await;
    assert_eq!(list.status, StatusCode::OK, "{}", list.body);
    assert!(list
        .body
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["kind"] == "panel"));
    assert!(list
        .body
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["kind"] == "access"));
}

#[sqlx::test(migrations = "./migrations")]
async fn zones_discovery_is_paginated_and_permission_loss_is_non_destructive(pool: PgPool) {
    let (cloudflare, fake) = fake_cloudflare().await;
    {
        let mut state = fake.lock().unwrap();
        state.zone_page_size = 1;
        state.zones = Some(vec![
            json!({"id":ZONE,"name":"example.com","status":"active","account":{"id":ACCOUNT}}),
            json!({"id":OTHER_ZONE,"name":"second.net","status":"active","account":{"id":ACCOUNT}}),
        ]);
    }
    let data_dir = scratch_dir();
    let app = app(pool.clone(), &data_dir, &cloudflare);
    let session = owner(&app).await;
    let saved = call(
        &app,
        "PUT",
        "/api/v1/cloudflare/credentials",
        &session,
        Some(json!({"account_id":ACCOUNT,"api_token":TOKEN})),
    )
    .await;
    assert_eq!(saved.status, StatusCode::OK, "{}", saved.body);
    assert_eq!(saved.body["zone_id"], "");
    let discovered = call(&app, "GET", "/api/v1/cloudflare/zones", &session, None).await;
    assert_eq!(discovered.body.as_array().unwrap().len(), 2);
    assert!(discovered
        .body
        .as_array()
        .unwrap()
        .iter()
        .all(|z| z["enabled"] == false));
    assert!(fake
        .lock()
        .unwrap()
        .calls
        .iter()
        .any(|c| c == "zones page 2"));
    let enabled = call(
        &app,
        "PUT",
        &format!("/api/v1/cloudflare/zones/{OTHER_ZONE}"),
        &session,
        Some(json!({"enabled":true})),
    )
    .await;
    assert_eq!(enabled.status, StatusCode::OK, "{}", enabled.body);
    let status = call(&app, "GET", "/api/v1/cloudflare", &session, None).await;
    assert_eq!(status.body["zone_name"], "second.net");
    sqlx::query(
        "INSERT INTO domain_dns_records VALUES($1,'app.second.net','retained','A','192.0.2.10')",
    )
    .bind(OTHER_ZONE)
    .execute(&pool)
    .await
    .unwrap();
    fake.lock().unwrap().zones = Some(vec![]);
    let lost = call(
        &app,
        "POST",
        "/api/v1/cloudflare/zones/refresh",
        &session,
        None,
    )
    .await;
    assert_eq!(lost.status, StatusCode::OK, "{}", lost.body);
    assert!(lost
        .body
        .as_array()
        .unwrap()
        .iter()
        .all(|z| z["accessible"] == false));
    fake.lock().unwrap().fail_discovery = true;
    let denied = call(
        &app,
        "POST",
        "/api/v1/cloudflare/zones/refresh",
        &session,
        None,
    )
    .await;
    assert_eq!(denied.status, StatusCode::FAILED_DEPENDENCY);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM domain_dns_records")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
    let enabled: bool = sqlx::query_scalar("SELECT enabled FROM cloudflare_zones WHERE zone_id=$1")
        .bind(OTHER_ZONE)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(enabled);
}

#[sqlx::test(migrations = "./migrations")]
async fn zones_migration_preserves_legacy_zone_and_publication(pool: PgPool) {
    let mut tx = pool.begin().await.unwrap();
    sqlx::raw_sql("CREATE TEMP TABLE cloudflare_settings(zone_id TEXT,account_id TEXT,zone_name TEXT,published_hosts TEXT[]); INSERT INTO cloudflare_settings VALUES('legacy','account','Example.Com',ARRAY['app.example.com']); DROP TABLE cloudflare_zones;")
        .execute(&mut *tx).await.unwrap();
    sqlx::raw_sql(include_str!("../migrations/0027_cloudflare_zones.sql"))
        .execute(&mut *tx)
        .await
        .unwrap();
    let row: (String, bool, Vec<String>) = sqlx::query_as(
        "SELECT name,enabled,published_hosts FROM cloudflare_zones WHERE zone_id='legacy'",
    )
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(
        row,
        ("example.com".into(), true, vec!["app.example.com".into()])
    );
    tx.rollback().await.unwrap();
}

#[sqlx::test(migrations = "./migrations")]
async fn credentials_are_checked_and_never_returned(pool: PgPool) {
    let (cloudflare, fake) = fake_cloudflare().await;
    let data_dir = scratch_dir();
    let app = app(pool.clone(), &data_dir, &cloudflare);

    let anonymous = call_with(&app, "GET", "/api/v1/cloudflare", None, None, None).await;
    assert_eq!(anonymous.status, StatusCode::UNAUTHORIZED);

    let session = owner(&app).await;
    let fresh = call(&app, "GET", "/api/v1/cloudflare", &session, None).await;
    assert_eq!(fresh.status, StatusCode::OK, "{}", fresh.body);
    assert_eq!(fresh.body["token_saved"], false);
    assert_eq!(fresh.body["tunnel_id"], Value::Null);
    assert_eq!(fresh.body["max_replicas"], 5);

    let save = |account: &str, zone: &str, token: Option<&str>| {
        let mut body = json!({ "account_id": account, "zone_id": zone });
        if let Some(token) = token {
            body["api_token"] = json!(token);
        }
        body
    };
    let short = call(
        &app,
        "PUT",
        "/api/v1/cloudflare/credentials",
        &session,
        Some(save("abc", ZONE, Some(TOKEN))),
    )
    .await;
    assert_eq!(short.status, StatusCode::BAD_REQUEST);
    let missing = call(
        &app,
        "PUT",
        "/api/v1/cloudflare/credentials",
        &session,
        Some(save(ACCOUNT, ZONE, None)),
    )
    .await;
    assert_eq!(missing.status, StatusCode::BAD_REQUEST);
    let foreign = call(
        &app,
        "PUT",
        "/api/v1/cloudflare/credentials",
        &session,
        Some(save(ACCOUNT, OTHER_ZONE, Some(TOKEN))),
    )
    .await;
    assert_eq!(foreign.status, StatusCode::BAD_REQUEST);
    assert_eq!(foreign.body["error"]["code"], "zone_account_mismatch");
    let denied = call(
        &app,
        "PUT",
        "/api/v1/cloudflare/credentials",
        &session,
        Some(save(ACCOUNT, ZONE, Some("denied"))),
    )
    .await;
    assert_eq!(denied.status, StatusCode::FAILED_DEPENDENCY);
    assert_eq!(denied.body["error"]["code"], "cloudflare_permission");
    assert!(
        denied.body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("listing tunnels"),
        "{}",
        denied.body
    );
    let wrong = call(
        &app,
        "PUT",
        "/api/v1/cloudflare/credentials",
        &session,
        Some(save(ACCOUNT, ZONE, Some("nottheone"))),
    )
    .await;
    assert_eq!(wrong.status, StatusCode::FAILED_DEPENDENCY);
    assert!(!wrong.body.to_string().contains("nottheone"));

    let saved = call(
        &app,
        "PUT",
        "/api/v1/cloudflare/credentials",
        &session,
        Some(save(ACCOUNT, ZONE, Some(TOKEN))),
    )
    .await;
    assert_eq!(saved.status, StatusCode::OK, "{}", saved.body);
    assert_eq!(saved.body["token_saved"], true);
    assert_eq!(saved.body["zone_name"], "example.com");
    assert!(!saved.body.to_string().contains(TOKEN));
    // The saved token is reused when the field is left out.
    let again = call(
        &app,
        "PUT",
        "/api/v1/cloudflare/credentials",
        &session,
        Some(save(ACCOUNT, ZONE, None)),
    )
    .await;
    assert_eq!(again.status, StatusCode::OK, "{}", again.body);

    let stored: Vec<u8> =
        sqlx::query_scalar("SELECT api_token_enc FROM cloudflare_settings WHERE id = TRUE")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(!String::from_utf8_lossy(&stored).contains(TOKEN));
    assert!(calls(&fake)
        .iter()
        .any(|call| call.starts_with("GET /zones/")));
    let contended = holding_mutation_lock(&app);
    for (key, code) in [
        (77_302_461_i64, "tunnel_busy"),
        (77_302_462, "cloudflare_busy"),
    ] {
        let mut guard = pool.begin().await.unwrap();
        sqlx::query("SELECT pg_advisory_xact_lock($1)")
            .bind(key)
            .execute(&mut *guard)
            .await
            .unwrap();
        let busy = call(
            &contended,
            "PUT",
            "/api/v1/cloudflare/credentials",
            &session,
            Some(save(ACCOUNT, ZONE, None)),
        )
        .await;
        assert_eq!(busy.status, StatusCode::CONFLICT, "{}", busy.body);
        assert_eq!(busy.body["error"]["code"], code);
        guard.rollback().await.unwrap();
        wait_for_mutation_rollbacks(&pool).await;
        let recovered = call(
            &app,
            "PUT",
            "/api/v1/cloudflare/credentials",
            &session,
            Some(save(ACCOUNT, ZONE, None)),
        )
        .await;
        assert_eq!(recovered.status, StatusCode::OK, "{}", recovered.body);
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn tunnel_creation_publishes_an_empty_ingress(pool: PgPool) {
    let (cloudflare, fake) = fake_cloudflare().await;
    let data_dir = scratch_dir();
    let app = app(pool, &data_dir, &cloudflare);
    let session = owner(&app).await;
    let credentials = call(
        &app,
        "PUT",
        "/api/v1/cloudflare/credentials",
        &session,
        Some(json!({"account_id":ACCOUNT,"api_token":TOKEN})),
    )
    .await;
    assert_eq!(credentials.status, StatusCode::OK);
    let tunnel = call(
        &app,
        "POST",
        "/api/v1/cloudflare/tunnel",
        &session,
        Some(json!({})),
    )
    .await;
    assert_eq!(tunnel.status, StatusCode::CREATED, "{}", tunnel.body);
    assert_eq!(tunnel.body["tunnel_published"], true);
    assert_eq!(tunnel.body["published_hosts"], json!([]));
    let fake = fake.lock().unwrap();
    assert_eq!(fake.configs.len(), 1);
    assert_eq!(
        fake.configs[0]["config"]["ingress"],
        json!([{"service":"http_status:404"}])
    );
    assert!(fake.dns.is_empty());
}

#[sqlx::test(migrations = "./migrations")]
async fn multi_zone_portal_keeps_plugin_access_and_dns_ownership_on_partial_failure(pool: PgPool) {
    let (cloudflare, fake) = fake_cloudflare().await;
    fake.lock().unwrap().zones = Some(vec![
        json!({"id":ZONE,"name":"example.com","status":"active","account":{"id":ACCOUNT}}),
        json!({"id":OTHER_ZONE,"name":"second.net","status":"active","account":{"id":ACCOUNT}}),
    ]);
    let data_dir = scratch_dir();
    let app = app(pool.clone(), &data_dir, &cloudflare);
    let session = owner(&app).await;
    let server = call(
        &app,
        "POST",
        "/api/v1/servers",
        &session,
        Some(json!({"name":"local","kind":"local","public_ip":"203.0.113.10"})),
    )
    .await;
    let project = call(
        &app,
        "POST",
        "/api/v1/projects",
        &session,
        Some(json!({"name":"web"})),
    )
    .await;
    let env = call(
        &app,
        "POST",
        &format!(
            "/api/v1/projects/{}/environments",
            project.body["id"].as_str().unwrap()
        ),
        &session,
        Some(json!({"name":"prod"})),
    )
    .await;
    let service = call(&app,"POST",&format!("/api/v1/environments/{}/services",env.body["id"].as_str().unwrap()),&session,Some(json!({"name":"web","kind":"application","server_id":server.body["id"],"spec":{"image":"nginx:1.27"}}))).await;
    assert_eq!(service.status, StatusCode::CREATED, "{}", service.body);
    let service_id = service.body["id"].as_str().unwrap();
    let credentials = call(
        &app,
        "PUT",
        "/api/v1/cloudflare/credentials",
        &session,
        Some(json!({"account_id":ACCOUNT,"zone_id":ZONE,"api_token":TOKEN})),
    )
    .await;
    assert_eq!(credentials.status, StatusCode::OK, "{}", credentials.body);
    let enabled = call(
        &app,
        "PUT",
        &format!("/api/v1/cloudflare/zones/{OTHER_ZONE}"),
        &session,
        Some(json!({"enabled":true})),
    )
    .await;
    assert_eq!(enabled.status, StatusCode::OK, "{}", enabled.body);
    sqlx::query("UPDATE panel_settings SET panel_domain='panel.example.com' WHERE id=TRUE")
        .execute(&pool)
        .await
        .unwrap();
    let tunnel = call(
        &app,
        "POST",
        "/api/v1/cloudflare/tunnel",
        &session,
        Some(json!({"replicas":1})),
    )
    .await;
    assert_eq!(tunnel.status, StatusCode::CREATED, "{}", tunnel.body);
    let portal = call(
        &app,
        "PUT",
        "/api/v1/access/settings",
        &session,
        Some(json!({"portal_host":"access.second.net"})),
    )
    .await;
    assert_eq!(portal.status, StatusCode::OK, "{}", portal.body);
    let domain = call(&app,"POST",&format!("/api/v1/services/{service_id}/domains"),&session,Some(json!({"host":"dockside.example.com","target_port":8080,"ssl_mode":"letsencrypt","routing_mode":"cloudflare"}))).await;
    assert_eq!(domain.status, StatusCode::CREATED, "{}", domain.body);
    let policy = call(
        &app,
        "PUT",
        &format!("/api/v1/access/services/{service_id}"),
        &session,
        Some(json!({"enabled":true,"rules":[{"kind":"email","email":"owner@example.com"}]})),
    )
    .await;
    assert_eq!(policy.status, StatusCode::OK, "{}", policy.body);
    sqlx::query("INSERT INTO plugin_installs(plugin,service_id,installed_version,image) VALUES('dockside',$1,'0.2.2','dockside:test')")
        .bind(Uuid::parse_str(service_id).unwrap()).execute(&pool).await.unwrap();
    let publish = call(
        &app,
        "POST",
        "/api/v1/cloudflare/tunnel/publish",
        &session,
        Some(json!({})),
    )
    .await;
    assert_eq!(publish.status, StatusCode::OK, "{}", publish.body);
    assert_eq!(publish.body["pending"], 0, "{}", publish.body);
    assert_eq!(publish.body["zones"].as_array().unwrap().len(), 2);
    let ownership: Vec<(String, String)> =
        sqlx::query_as("SELECT zone_id,host FROM domain_dns_records ORDER BY host")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert!(ownership.contains(&(OTHER_ZONE.into(), "access.second.net".into())));
    assert!(ownership.contains(&(ZONE.into(), "dockside.example.com".into())));
    let readiness = call(&app, "GET", "/api/v1/cloudflare/preflight", &session, None).await;
    assert_eq!(
        readiness.body["checks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == "rudder_access_portal")
            .unwrap()["ok"],
        true
    );
    sqlx::query("UPDATE cloudflare_settings SET access_enforced=TRUE,access_aud='cross-zone-test',team_domain='myteam.cloudflareaccess.com' WHERE id=TRUE").execute(&pool).await.unwrap();
    let settings = call(&app, "GET", "/api/v1/access/settings", &session, None).await;
    assert_eq!(settings.status, StatusCode::OK, "{}", settings.body);
    assert_eq!(settings.body["warnings"], json!([]));
    let readiness = call(&app, "GET", "/api/v1/cloudflare/preflight", &session, None).await;
    assert!(!readiness.body["checks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|check| check["id"] == "rudder_access_portal_cloudflare"));
    let routes = std::fs::read_to_string(data_dir.join("traefik/dynamic/routes.yml")).unwrap();
    let protected = routes
        .lines()
        .skip_while(|line| !line.contains("Host(`dockside.example.com`) && PathPrefix(`/`)"))
        .take_while(|line| line.starts_with("      "))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(protected.contains("- rudder-gate"), "{protected}");
    assert!(protected.contains("- websecure"), "{protected}");
    let secret: Vec<u8> =
        sqlx::query_scalar("SELECT gate_secret_enc FROM access_settings WHERE id=TRUE")
            .fetch_one(&pool)
            .await
            .unwrap();
    let secret = String::from_utf8(
        rudder_core::Cipher::new([0xcc; 32])
            .decrypt(&secret)
            .unwrap(),
    )
    .unwrap();
    for (path, expected) in [
        ("/", StatusCode::TEMPORARY_REDIRECT),
        ("/v1/blobs/item", StatusCode::OK),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/edge/gate?k={secret}"))
                    .header("x-forwarded-host", "dockside.example.com")
                    .header("x-forwarded-proto", "https")
                    .header("x-forwarded-uri", path)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
        if expected == StatusCode::TEMPORARY_REDIRECT {
            assert!(response.headers()["location"]
                .to_str()
                .unwrap()
                .starts_with("https://access.second.net/access?"));
        }
    }
    sqlx::query("INSERT INTO access_identities(email) VALUES('owner@example.com') ON CONFLICT(email) DO UPDATE SET status='active'").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO access_codes(email,code_hash,purpose,return_host,return_path,expires_at) VALUES('owner@example.com',encode(digest('cross-zone-handoff','sha256'),'hex'),'handoff','dockside.example.com','/dashboard',now()+interval '2 minutes')").execute(&pool).await.unwrap();
    let callback_app = self::app(pool.clone(), &data_dir, &cloudflare);
    let wrong_host = call_with(
        &callback_app,
        "GET",
        "/_rudder/callback?code=cross-zone-handoff",
        None,
        None,
        Some("access.second.net"),
    )
    .await;
    assert_eq!(wrong_host.status, StatusCode::FORBIDDEN);
    let callback = callback_app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/_rudder/callback?code=cross-zone-handoff")
                .header("host", "dockside.example.com")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(callback.status(), StatusCode::TEMPORARY_REDIRECT);
    assert_eq!(callback.headers()["location"], "/dashboard");
    let cookie = callback.headers()["set-cookie"].to_str().unwrap();
    assert!(cookie.contains("Secure"));
    let authorized = callback_app
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/edge/gate?k={secret}"))
                .header("x-forwarded-host", "dockside.example.com")
                .header("x-forwarded-uri", "/dashboard")
                .header("cookie", cookie.split(';').next().unwrap())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(authorized.status(), StatusCode::OK);
    fake.lock().unwrap().failed_dns_zone = Some(OTHER_ZONE.into());
    let late = call(&app,"POST",&format!("/api/v1/services/{service_id}/domains"),&session,Some(json!({"host":"late.example.com","target_port":8080,"ssl_mode":"letsencrypt","routing_mode":"cloudflare"}))).await;
    assert_eq!(late.status, StatusCode::CREATED, "{}", late.body);
    assert!(fake
        .lock()
        .unwrap()
        .dns
        .iter()
        .any(|r| r["name"] == "late.example.com"));
    let partial = call(
        &app,
        "POST",
        "/api/v1/cloudflare/tunnel/publish",
        &session,
        Some(json!({})),
    )
    .await;
    assert_eq!(partial.status, StatusCode::OK, "{}", partial.body);
    assert!(!partial.body["zones"]
        .as_array()
        .unwrap()
        .iter()
        .find(|z| z["zone_id"] == OTHER_ZONE)
        .unwrap()["errors"]
        .as_array()
        .unwrap()
        .is_empty());
    let status = call(&app, "GET", "/api/v1/cloudflare", &session, None).await;
    assert!(status.body["published_hosts"]
        .as_array()
        .unwrap()
        .contains(&json!("access.second.net")));
    assert!(
        fake.lock().unwrap().configs.last().unwrap()["config"]["ingress"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["hostname"] == "access.second.net")
    );
    fake.lock().unwrap().failed_dns_zone = None;
    for _ in 0..2 {
        let retry = call(
            &app,
            "POST",
            "/api/v1/cloudflare/tunnel/publish",
            &session,
            Some(json!({})),
        )
        .await;
        assert_eq!(retry.body["pending"], 0, "{}", retry.body);
    }
    assert_eq!(fake.lock().unwrap().dns.len(), 4);
    fake.lock().unwrap().zones = Some(vec![
        json!({"id":ZONE,"name":"example.com","status":"active","account":{"id":ACCOUNT}}),
    ]);
    let invisible = call(
        &app,
        "POST",
        "/api/v1/cloudflare/zones/refresh",
        &session,
        None,
    )
    .await;
    assert_eq!(invisible.status, StatusCode::OK, "{}", invisible.body);
    let retained = call(
        &app,
        "POST",
        "/api/v1/cloudflare/tunnel/publish",
        &session,
        Some(json!({})),
    )
    .await;
    assert_eq!(retained.status, StatusCode::OK, "{}", retained.body);
    assert_eq!(fake.lock().unwrap().dns.len(), 4);
    let disabled = call(
        &app,
        "PUT",
        &format!("/api/v1/cloudflare/zones/{OTHER_ZONE}"),
        &session,
        Some(json!({"enabled":false})),
    )
    .await;
    assert_eq!(disabled.status, StatusCode::OK);
    let retained = call(
        &app,
        "POST",
        "/api/v1/cloudflare/tunnel/publish",
        &session,
        Some(json!({})),
    )
    .await;
    assert_eq!(retained.status, StatusCode::OK, "{}", retained.body);
    assert_eq!(fake.lock().unwrap().dns.len(), 4);
    let blocked = call(
        &app,
        "PUT",
        "/api/v1/cloudflare/credentials",
        &session,
        Some(json!({"account_id":"ffffffffffffffffffffffffffffffff","api_token":TOKEN})),
    )
    .await;
    assert_eq!(blocked.status, StatusCode::CONFLICT);
    // The same record id in another zone must not be touched during cleanup.
    let primary_id = fake
        .lock()
        .unwrap()
        .dns
        .iter()
        .find(|r| r["name"] == "panel.example.com")
        .unwrap()["id"]
        .clone();
    fake.lock().unwrap().dns.push(json!({"_zone":OTHER_ZONE,"id":primary_id,"name":"foreign.second.net","type":"CNAME","content":format!("{TUNNEL}.cfargotunnel.com")}));
    let removed = call(&app, "DELETE", "/api/v1/cloudflare/tunnel", &session, None).await;
    assert_eq!(removed.status, StatusCode::OK, "{}", removed.body);
    assert!(removed.body["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v.as_str().unwrap().contains("second.net")));
    let records = fake.lock().unwrap().dns.clone();
    assert_eq!(records.len(), 4, "{records:?}");
    assert_eq!(
        records.iter().filter(|r| r["_zone"] == OTHER_ZONE).count(),
        2
    );
    assert!(records
        .iter()
        .filter(|r| r["_zone"] == ZONE)
        .all(|r| r["type"] == "A" && r["content"] == "203.0.113.10"));
    let retained: Vec<String> =
        sqlx::query_scalar("SELECT zone_id FROM domain_dns_records WHERE zone_id=$1")
            .bind(OTHER_ZONE)
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(retained, vec![OTHER_ZONE.to_owned()]);
}

#[sqlx::test(migrations = "./migrations")]
async fn teardown_removes_owned_dns_in_each_enabled_zone(pool: PgPool) {
    let (cloudflare, fake) = fake_cloudflare().await;
    fake.lock().unwrap().zones = Some(vec![
        json!({"id":ZONE,"name":"example.com","status":"active","account":{"id":ACCOUNT}}),
        json!({"id":OTHER_ZONE,"name":"second.net","status":"active","account":{"id":ACCOUNT}}),
    ]);
    let data_dir = scratch_dir();
    let app = app(pool.clone(), &data_dir, &cloudflare);
    let session = owner(&app).await;
    let saved = call(
        &app,
        "PUT",
        "/api/v1/cloudflare/credentials",
        &session,
        Some(json!({"account_id":ACCOUNT,"zone_id":ZONE,"api_token":TOKEN})),
    )
    .await;
    assert_eq!(saved.status, StatusCode::OK, "{}", saved.body);
    call(
        &app,
        "PUT",
        &format!("/api/v1/cloudflare/zones/{OTHER_ZONE}"),
        &session,
        Some(json!({"enabled":true})),
    )
    .await;
    sqlx::query("UPDATE panel_settings SET panel_domain='panel.example.com' WHERE id=TRUE")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE access_settings SET portal_host='access.second.net' WHERE id=TRUE")
        .execute(&pool)
        .await
        .unwrap();
    let tunnel = call(
        &app,
        "POST",
        "/api/v1/cloudflare/tunnel",
        &session,
        Some(json!({"replicas":1})),
    )
    .await;
    assert_eq!(tunnel.status, StatusCode::CREATED, "{}", tunnel.body);
    assert_eq!(fake.lock().unwrap().dns.len(), 2);
    for record in &mut fake.lock().unwrap().dns {
        record["id"] = json!("shared-zone-local-id");
    }
    sqlx::query("UPDATE domain_dns_records SET record_id='shared-zone-local-id'")
        .execute(&pool)
        .await
        .unwrap();
    let removed = call(&app, "DELETE", "/api/v1/cloudflare/tunnel", &session, None).await;
    assert_eq!(removed.status, StatusCode::OK, "{}", removed.body);
    assert!(removed.body["removed_dns"]
        .as_array()
        .unwrap()
        .contains(&json!("panel.example.com")));
    assert!(removed.body["removed_dns"]
        .as_array()
        .unwrap()
        .contains(&json!("access.second.net")));
    assert!(fake.lock().unwrap().dns.is_empty());
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM domain_dns_records")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[sqlx::test(migrations = "./migrations")]
async fn the_whole_mode_can_be_set_up_and_removed(pool: PgPool) {
    let (cloudflare, fake) = fake_cloudflare().await;
    let data_dir = scratch_dir();
    let app = app(pool.clone(), &data_dir, &cloudflare);
    let session = owner(&app).await;

    // A local server with one app domain inside the zone and one outside it.
    let server = call(
        &app,
        "POST",
        "/api/v1/servers",
        &session,
        Some(json!({ "name": "local", "kind": "local", "public_ip": "203.0.113.10" })),
    )
    .await;
    assert_eq!(server.status, StatusCode::CREATED);
    let project = call(
        &app,
        "POST",
        "/api/v1/projects",
        &session,
        Some(json!({ "name": "web" })),
    )
    .await;
    let project_id = project.body["id"].as_str().unwrap().to_owned();
    let environment = call(
        &app,
        "POST",
        &format!("/api/v1/projects/{project_id}/environments"),
        &session,
        Some(json!({ "name": "prod" })),
    )
    .await;
    let environment_id = environment.body["id"].as_str().unwrap().to_owned();
    let service = call(
        &app,
        "POST",
        &format!("/api/v1/environments/{environment_id}/services"),
        &session,
        Some(json!({
            "name": "web",
            "kind": "application",
            "server_id": server.body["id"],
            "spec": { "image": "nginx:1.27" }
        })),
    )
    .await;
    let service_id = service.body["id"].as_str().unwrap().to_owned();
    for host in ["app.example.com", "app.other.org"] {
        let domain = call(
            &app,
            "POST",
            &format!("/api/v1/services/{service_id}/domains"),
            &session,
            Some(json!({ "host": host, "target_port": 8080, "ssl_mode": "letsencrypt" })),
        )
        .await;
        assert_eq!(domain.status, StatusCode::CREATED, "{}", domain.body);
    }
    sqlx::query("UPDATE panel_settings SET panel_domain = 'panel.example.com' WHERE id = TRUE")
        .execute(&pool)
        .await
        .unwrap();
    let routes_path = data_dir.join("traefik/dynamic/routes.yml");
    let direct = std::fs::read_to_string(&routes_path).unwrap();
    assert!(direct.contains("certResolver"), "{direct}");

    let credentials = call(
        &app,
        "PUT",
        "/api/v1/cloudflare/credentials",
        &session,
        Some(json!({ "account_id": ACCOUNT, "zone_id": ZONE, "api_token": TOKEN })),
    )
    .await;
    assert_eq!(credentials.status, StatusCode::OK, "{}", credentials.body);

    // The tunnel is created at Cloudflare and saved even without Docker; the
    // status says why no replica runs.
    assert!(fake.lock().unwrap().configs.is_empty());
    // Existing pre-feature domains are backfilled when the migration finds an active tunnel.
    sqlx::query("UPDATE domains SET routing_mode='cloudflare'")
        .execute(&pool)
        .await
        .unwrap();
    // An existing DNS record must not be overwritten by automatic publication.
    fake.lock().unwrap().dns.push(json!({
        "id": "old-a", "type": "A", "name": "app.example.com", "content": "203.0.113.10",
    }));
    let too_many = call(
        &app,
        "POST",
        "/api/v1/cloudflare/tunnel",
        &session,
        Some(json!({ "replicas": 9 })),
    )
    .await;
    assert_eq!(too_many.status, StatusCode::BAD_REQUEST);
    let tunnel = call(
        &app,
        "POST",
        "/api/v1/cloudflare/tunnel",
        &session,
        Some(json!({ "replicas": 3 })),
    )
    .await;
    assert_eq!(tunnel.status, StatusCode::CREATED, "{}", tunnel.body);
    assert_eq!(tunnel.body["tunnel_id"], TUNNEL);
    assert_eq!(tunnel.body["replicas"], 3);
    assert_eq!(tunnel.body["replicas_running"], 0);
    assert_eq!(tunnel.body["connections"], 2);
    assert_eq!(tunnel.body["tunnel_published"], true);
    assert_eq!(tunnel.body["published_hosts"], json!(["panel.example.com"]));
    {
        let fake = fake.lock().unwrap();
        let ingress = fake.configs.last().unwrap()["config"]["ingress"]
            .as_array()
            .unwrap();
        assert_eq!(ingress.len(), 3);
        assert_eq!(ingress[0]["hostname"], "app.example.com");
        assert_eq!(ingress[1]["hostname"], "panel.example.com");
        assert!(fake
            .dns
            .iter()
            .any(|record| record["name"] == "panel.example.com"
                && record["type"] == "CNAME"
                && record["content"] == format!("{TUNNEL}.cfargotunnel.com")));
        assert!(fake.dns.iter().any(|record| record["id"] == "old-a"));
    }
    assert!(tunnel.body["replica_error"].is_string(), "{}", tunnel.body);
    assert!(!tunnel.body.to_string().contains("dGVzdC10dW5uZWwtdG9rZW4"));
    let twice = call(
        &app,
        "POST",
        "/api/v1/cloudflare/tunnel",
        &session,
        Some(json!({})),
    )
    .await;
    assert_eq!(twice.status, StatusCode::CONFLICT);
    let scaled = call(
        &app,
        "PUT",
        "/api/v1/cloudflare/tunnel/replicas",
        &session,
        Some(json!({ "replicas": 0 })),
    )
    .await;
    assert_eq!(scaled.status, StatusCode::BAD_REQUEST);

    let first = call(
        &app,
        "POST",
        "/api/v1/cloudflare/tunnel/publish",
        &session,
        Some(json!({})),
    )
    .await;
    assert_eq!(first.status, StatusCode::OK, "{}", first.body);
    assert_eq!(
        first.body["hostnames"],
        json!(["app.example.com", "panel.example.com"])
    );
    assert_eq!(first.body["skipped"][0]["host"], "app.other.org");
    assert_eq!(first.body["pending"], 1);
    let actions: HashMap<String, String> = first.body["dns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| {
            (
                item["host"].as_str().unwrap().to_owned(),
                item["action"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    assert_eq!(actions["app.example.com"], "conflict");
    assert_eq!(actions["panel.example.com"], "kept");
    assert!(fake
        .lock()
        .unwrap()
        .dns
        .iter()
        .any(|record| record["id"] == "old-a"));
    let config = fake.lock().unwrap().configs.last().cloned().unwrap();
    let ingress = config["config"]["ingress"].as_array().unwrap();
    assert_eq!(ingress.len(), 3);
    assert_eq!(ingress[0]["hostname"], "app.example.com");
    assert_eq!(ingress[0]["service"], "https://rudder-traefik:443");
    assert_eq!(ingress[2]["service"], "http_status:404");
    let tunnelled = std::fs::read_to_string(&routes_path).unwrap();
    assert_eq!(tunnelled.matches("certResolver").count(), 1, "{tunnelled}");
    assert!(tunnelled.contains("tls: {}"));

    let report = call(&app, "GET", "/api/v1/cloudflare/preflight", &session, None).await;
    assert_eq!(report.status, StatusCode::OK, "{}", report.body);
    assert_eq!(report.body["ready"], false);
    let failing: Vec<&str> = report.body["checks"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|check| check["ok"] == false)
        .map(|check| check["id"].as_str().unwrap())
        .collect();
    assert!(failing.contains(&"published"));
    assert!(failing.contains(&"access"));
    assert!(failing.contains(&"replicas"));

    sqlx::query("UPDATE access_settings SET portal_host='access.other.org' WHERE id=TRUE")
        .execute(&pool)
        .await
        .unwrap();
    let report = call(&app, "GET", "/api/v1/cloudflare/preflight", &session, None).await;
    let portal = report.body["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|check| check["id"] == "rudder_access_portal")
        .unwrap();
    assert_eq!(portal["ok"], false);
    assert_eq!(portal["required"], true);
    assert!(portal["message"]
        .as_str()
        .unwrap()
        .contains("access.other.org"));
    let invalid = call(
        &app,
        "PUT",
        "/api/v1/access/settings",
        &session,
        Some(json!({"portal_host":"access.other.org"})),
    )
    .await;
    assert_eq!(invalid.status, StatusCode::BAD_REQUEST, "{}", invalid.body);
    assert_eq!(invalid.body["error"]["code"], "portal_outside_tunnel_zone");
    sqlx::query("UPDATE access_settings SET portal_host='panel.example.com' WHERE id=TRUE")
        .execute(&pool)
        .await
        .unwrap();
    let report = call(&app, "GET", "/api/v1/cloudflare/preflight", &session, None).await;
    assert_eq!(
        report.body["checks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|check| check["id"] == "rudder_access_portal")
            .unwrap()["ok"],
        true
    );
    sqlx::query("UPDATE access_settings SET portal_host=NULL WHERE id=TRUE")
        .execute(&pool)
        .await
        .unwrap();

    let policy_uri = format!("/api/v1/access/services/{service_id}");
    let policy_body =
        json!({"enabled":true,"rules":[{"kind":"email","email":"admin@example.com"}]});
    let protected = call(
        &app,
        "PUT",
        &policy_uri,
        &session,
        Some(policy_body.clone()),
    )
    .await;
    assert_eq!(protected.status, StatusCode::OK, "{}", protected.body);
    let routes = std::fs::read_to_string(&routes_path).unwrap();
    assert!(routes.contains("- rudder-gate"));
    assert!(routes.contains("forwardAuth:"));
    assert!(routes.contains("PathPrefix(`/_rudder`)"));

    // Reproduce an upgraded install whose default flag disagrees with its old policy.
    let service_uuid = Uuid::parse_str(&service_id).unwrap();
    sqlx::query("INSERT INTO plugin_installs(plugin,service_id,installed_version,image) VALUES('dockside',$1,'0.2.2','dockside:test')")
        .bind(service_uuid).execute(&pool).await.unwrap();
    sqlx::query("UPDATE access_policies SET enabled=false WHERE service_id=$1")
        .bind(service_uuid)
        .execute(&pool)
        .await
        .unwrap();
    let republished = call(
        &app,
        "POST",
        "/api/v1/cloudflare/tunnel/publish",
        &session,
        Some(json!({})),
    )
    .await;
    assert_eq!(republished.status, StatusCode::OK, "{}", republished.body);
    let policy = call(&app, "GET", &policy_uri, &session, None).await;
    assert_eq!(policy.body["enabled"], true);
    assert_eq!(policy.body["rules"][0]["email"], "admin@example.com");
    assert!(policy.body["bypass_paths"]
        .as_array()
        .unwrap()
        .contains(&json!("/v1/health")));
    let secret: Vec<u8> =
        sqlx::query_scalar("SELECT gate_secret_enc FROM access_settings WHERE id=TRUE")
            .fetch_one(&pool)
            .await
            .unwrap();
    let secret = String::from_utf8(
        rudder_core::Cipher::new([0xcc; 32])
            .decrypt(&secret)
            .unwrap(),
    )
    .unwrap();
    let routes = std::fs::read_to_string(&routes_path).unwrap();
    let protected = routes
        .lines()
        .skip_while(|line| !line.contains("Host(`app.example.com`) && PathPrefix(`/`)"))
        .take_while(|line| line.starts_with("      "))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(protected.contains("- websecure"), "{protected}");
    assert!(protected.contains("- rudder-gate"), "{protected}");
    assert!(protected.contains("tls: {}"), "{protected}");
    for (path, expected) in [
        ("/", StatusCode::TEMPORARY_REDIRECT),
        ("/v1/blobs/item", StatusCode::OK),
        ("/v1/queue/drain", StatusCode::TEMPORARY_REDIRECT),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/edge/gate?k={secret}"))
                    .header("x-forwarded-host", "app.example.com")
                    .header("x-forwarded-proto", "https")
                    .header("x-forwarded-uri", path)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), expected, "{path}");
        if expected == StatusCode::TEMPORARY_REDIRECT {
            assert!(response.headers()["location"]
                .to_str()
                .unwrap()
                .starts_with("https://panel.example.com/access?"));
        }
    }
    sqlx::query("UPDATE access_settings SET portal_host='panel.example.com' WHERE id=TRUE")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE cloudflare_settings SET access_enforced=TRUE,access_aud='callback-test',team_domain='myteam.cloudflareaccess.com' WHERE id=TRUE").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO access_identities(email) VALUES('admin@example.com') ON CONFLICT(email) DO UPDATE SET status='active'").execute(&pool).await.unwrap();
    let callback_app = self::app(pool.clone(), &data_dir, &cloudflare);
    let callback_route = routes
        .lines()
        .skip_while(|line| !line.contains("Host(`app.example.com`) && PathPrefix(`/_rudder`)"))
        .take_while(|line| line.starts_with("      "))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        callback_route.contains("priority: 1000"),
        "{callback_route}"
    );
    assert!(
        callback_route.contains("service: rudder-panel"),
        "{callback_route}"
    );
    assert!(callback_route.contains("- websecure"), "{callback_route}");
    assert!(!callback_route.contains("rudder-gate"), "{callback_route}");
    sqlx::query("INSERT INTO access_codes(email,code_hash,purpose,return_host,return_path,expires_at) VALUES('admin@example.com',encode(digest('tunnel-handoff','sha256'),'hex'),'handoff','app.example.com','/dashboard?view=files',now()+interval '2 minutes')").execute(&pool).await.unwrap();
    let wrong = call_with(
        &callback_app,
        "GET",
        "/_rudder/callback?code=tunnel-handoff",
        None,
        None,
        Some("other.example.com"),
    )
    .await;
    assert_eq!(wrong.status, StatusCode::FORBIDDEN, "{}", wrong.body);
    let callback = callback_app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/_rudder/callback?code=tunnel-handoff")
                .header("host", "app.example.com")
                .header("x-forwarded-proto", "http")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(callback.status(), StatusCode::TEMPORARY_REDIRECT);
    assert_eq!(callback.headers()["location"], "/dashboard?view=files");
    assert_eq!(callback.headers()["cache-control"], "no-store");
    let cookie = callback.headers()["set-cookie"].to_str().unwrap();
    for attribute in ["Secure", "HttpOnly", "SameSite=Lax", "Path=/"] {
        assert!(cookie.contains(attribute), "{cookie}");
    }
    assert!(!cookie.contains("Domain="), "{cookie}");
    let cookie = cookie.split(';').next().unwrap().to_owned();
    let authorized = callback_app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/edge/gate?k={secret}"))
                .header("x-forwarded-host", "app.example.com")
                .header("x-forwarded-uri", "/dashboard?view=files")
                .header("cookie", cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(authorized.status(), StatusCode::OK);
    let replay = call_with(
        &callback_app,
        "GET",
        "/_rudder/callback?code=tunnel-handoff",
        None,
        None,
        Some("app.example.com"),
    )
    .await;
    assert_eq!(replay.status, StatusCode::FORBIDDEN);
    let missing = call_with(
        &callback_app,
        "GET",
        "/_rudder/unknown",
        None,
        None,
        Some("app.example.com"),
    )
    .await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
    let invalid = call_with(
        &callback_app,
        "GET",
        "/_rudder/callback?code=x",
        None,
        None,
        Some("app.example.com"),
    )
    .await;
    assert_eq!(invalid.status, StatusCode::FORBIDDEN);
    for denied in ["expired", "revoked", "blocked"] {
        sqlx::query("INSERT INTO access_codes(email,code_hash,purpose,return_host,return_path,expires_at) VALUES('admin@example.com',encode(digest($1,'sha256'),'hex'),'handoff','app.example.com','/',CASE WHEN $1='expired' THEN now()-interval '1 minute' ELSE now()+interval '2 minutes' END)").bind(denied).execute(&pool).await.unwrap();
        if denied == "revoked" {
            sqlx::query("UPDATE access_policies SET enabled=FALSE WHERE service_id=$1")
                .bind(service_uuid)
                .execute(&pool)
                .await
                .unwrap();
        }
        if denied == "blocked" {
            sqlx::query(
                "UPDATE access_identities SET status='blocked' WHERE email='admin@example.com'",
            )
            .execute(&pool)
            .await
            .unwrap();
        }
        let rejected = call_with(
            &callback_app,
            "GET",
            &format!("/_rudder/callback?code={denied}"),
            None,
            None,
            Some("app.example.com"),
        )
        .await;
        assert_eq!(
            rejected.status,
            StatusCode::FORBIDDEN,
            "{denied}: {}",
            rejected.body
        );
        sqlx::query("UPDATE access_policies SET enabled=TRUE WHERE service_id=$1")
            .bind(service_uuid)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("UPDATE access_identities SET status='active' WHERE email='admin@example.com'")
            .execute(&pool)
            .await
            .unwrap();
    }
    let sessions: i64 =
        sqlx::query_scalar("SELECT count(*) FROM access_sessions WHERE host='app.example.com'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(sessions, 1);
    let panel_blocked = call_with(
        &callback_app,
        "GET",
        "/api/v1/me",
        Some(&session),
        None,
        Some("panel.example.com"),
    )
    .await;
    assert_eq!(panel_blocked.status, StatusCode::FORBIDDEN);
    sqlx::query("UPDATE access_settings SET portal_host=NULL WHERE id=TRUE")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE cloudflare_settings SET access_enforced=FALSE,access_aud=NULL,team_domain='' WHERE id=TRUE").execute(&pool).await.unwrap();

    let disabled = call(
        &app,
        "PUT",
        &policy_uri,
        &session,
        Some(json!({"enabled":false,"rules":policy_body["rules"]})),
    )
    .await;
    assert_eq!(disabled.status, StatusCode::OK, "{}", disabled.body);
    let enabled: bool =
        sqlx::query_scalar("SELECT access_enabled FROM plugin_installs WHERE service_id=$1")
            .bind(service_uuid)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(!enabled);
    std::fs::remove_file(&routes_path).unwrap();
    std::fs::create_dir(&routes_path).unwrap();
    let failed = call(
        &app,
        "PUT",
        &policy_uri,
        &session,
        Some(policy_body.clone()),
    )
    .await;
    assert!(failed.status.is_server_error(), "{}", failed.body);
    let error: Option<String> =
        sqlx::query_scalar("SELECT last_error FROM plugin_installs WHERE service_id=$1")
            .bind(service_uuid)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(error
        .unwrap()
        .starts_with("Access route publication failed:"));
    std::fs::remove_dir(&routes_path).unwrap();
    let repaired = call(&app, "PUT", &policy_uri, &session, Some(policy_body)).await;
    assert_eq!(repaired.status, StatusCode::OK, "{}", repaired.body);
    let error: Option<String> =
        sqlx::query_scalar("SELECT last_error FROM plugin_installs WHERE service_id=$1")
            .bind(service_uuid)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(error.is_none());

    let second = call(
        &app,
        "POST",
        "/api/v1/cloudflare/tunnel/publish",
        &session,
        Some(json!({ "overwrite_dns": true })),
    )
    .await;
    assert_eq!(second.status, StatusCode::OK, "{}", second.body);
    assert_eq!(second.body["pending"], 0);
    {
        let fake = fake.lock().unwrap();
        assert_eq!(fake.dns.len(), 2);
        assert!(fake.dns.iter().all(|record| {
            record["type"] == "CNAME"
                && record["content"] == format!("{TUNNEL}.cfargotunnel.com")
                && record["proxied"] == true
        }));
    }
    let settled = call(&app, "GET", "/api/v1/cloudflare", &session, None).await;
    assert_eq!(settled.body["tunnel_published"], true);
    assert_eq!(
        settled.body["published_hosts"],
        json!(["app.example.com", "panel.example.com"])
    );

    // Domains added after connecting need no separate Publish action.
    let late = call(
        &app,
        "POST",
        &format!("/api/v1/services/{service_id}/domains"),
        &session,
        Some(json!({"host":"late.example.com", "target_port":8080, "ssl_mode":"letsencrypt", "routing_mode":"cloudflare"})),
    )
    .await;
    assert_eq!(late.status, StatusCode::CREATED, "{}", late.body);
    {
        let fake = fake.lock().unwrap();
        assert!(fake.configs.last().unwrap()["config"]["ingress"]
            .as_array()
            .unwrap()
            .iter()
            .any(|rule| rule["hostname"] == "late.example.com"));
        assert!(fake
            .dns
            .iter()
            .any(|record| record["name"] == "late.example.com"
                && record["content"] == format!("{TUNNEL}.cfargotunnel.com")
                && record["proxied"] == true));
    }
    let removed = call(
        &app,
        "DELETE",
        &format!("/api/v1/domains/{}", late.body["id"].as_str().unwrap()),
        &session,
        None,
    )
    .await;
    assert_eq!(removed.status, StatusCode::NO_CONTENT, "{}", removed.body);
    assert!(!fake
        .lock()
        .unwrap()
        .dns
        .iter()
        .any(|record| record["name"] == "late.example.com"));

    // A failed automatic sync is visible and can be retried without losing domains.
    fake.lock().unwrap().fail_config = true;
    let failed = call(
        &app,
        "POST",
        &format!("/api/v1/services/{service_id}/domains"),
        &session,
        Some(json!({"host":"retry.example.com", "target_port":8080, "ssl_mode":"letsencrypt", "routing_mode":"cloudflare"})),
    )
    .await;
    assert_eq!(
        failed.status,
        StatusCode::FAILED_DEPENDENCY,
        "{}",
        failed.body
    );
    assert!(!fake
        .lock()
        .unwrap()
        .dns
        .iter()
        .any(|record| record["name"] == "retry.example.com"));
    fake.lock().unwrap().fail_config = false;
    let retried = call(
        &app,
        "POST",
        "/api/v1/cloudflare/tunnel/publish",
        &session,
        Some(json!({})),
    )
    .await;
    assert_eq!(retried.status, StatusCode::OK, "{}", retried.body);
    assert!(fake
        .lock()
        .unwrap()
        .dns
        .iter()
        .any(|record| record["name"] == "retry.example.com"));
    let retry_id: Uuid =
        sqlx::query_scalar("SELECT id FROM domains WHERE host='retry.example.com'")
            .fetch_one(&pool)
            .await
            .unwrap();
    let removed = call(
        &app,
        "DELETE",
        &format!("/api/v1/domains/{retry_id}"),
        &session,
        None,
    )
    .await;
    assert_eq!(removed.status, StatusCode::NO_CONTENT);

    // Panel domain edits use the same automatic reconciliation path.
    for host in ["panel-new.example.com", "panel.example.com"] {
        let changed = call(
            &app,
            "PATCH",
            "/api/v1/panel",
            &session,
            Some(json!({"panel_domain":host})),
        )
        .await;
        assert_eq!(changed.status, StatusCode::OK, "{}", changed.body);
        let fake = fake.lock().unwrap();
        assert!(fake.configs.last().unwrap()["config"]["ingress"]
            .as_array()
            .unwrap()
            .iter()
            .any(|rule| rule["hostname"] == host));
        assert!(fake.dns.iter().any(|record| record["name"] == host));
    }
    assert!(!fake
        .lock()
        .unwrap()
        .dns
        .iter()
        .any(|record| record["name"] == "panel-new.example.com"));

    // Access in front of the panel.
    for body in [
        json!({ "team_domain": "bad domain!", "emails": ["a@example.com"] }),
        json!({ "team_domain": "https://myteam.cloudflareaccess.com/", "emails": [] }),
        json!({ "team_domain": "https://myteam.cloudflareaccess.com/", "emails": ["not-an-email"] }),
    ] {
        let rejected = call(
            &app,
            "PUT",
            "/api/v1/cloudflare/access",
            &session,
            Some(body),
        )
        .await;
        assert_eq!(
            rejected.status,
            StatusCode::BAD_REQUEST,
            "{}",
            rejected.body
        );
    }
    let access = call(
        &app,
        "PUT",
        "/api/v1/cloudflare/access",
        &session,
        Some(json!({
            "team_domain": "https://myteam.cloudflareaccess.com/",
            "emails": ["Admin@Example.com", "admin@example.com"],
            "agent_bypass": true
        })),
    )
    .await;
    assert_eq!(access.status, StatusCode::OK, "{}", access.body);
    assert_eq!(access.body["team_domain"], "myteam.cloudflareaccess.com");
    assert_eq!(access.body["access_emails"], json!(["admin@example.com"]));
    assert_eq!(access.body["access_configured"], true);
    assert_eq!(access.body["access_enforced"], false);
    {
        let fake = fake.lock().unwrap();
        // The panel plus one bypass application for each agent path.
        assert_eq!(fake.apps.len(), 6);
        let panel = fake
            .apps
            .values()
            .find(|app| app["name"] == "Rudder panel")
            .unwrap();
        assert_eq!(panel["domain"], "panel.example.com");
        assert_eq!(panel["policies"][0]["decision"], "allow");
        assert!(fake
            .apps
            .values()
            .any(|app| app["domain"] == "panel.example.com/mcp"
                && app["policies"][0]["decision"] == "bypass"));
    }

    // Enforcement needs proof that Access works when asked through the panel domain.
    let unproven = call_with(
        &app,
        "PUT",
        "/api/v1/cloudflare/access/enforcement",
        Some(&session),
        Some(json!({ "enforced": true })),
        Some("panel.example.com"),
    )
    .await;
    assert_eq!(unproven.status, StatusCode::CONFLICT, "{}", unproven.body);
    assert_eq!(unproven.body["error"]["code"], "access_not_verified");
    let enforced = call(
        &app,
        "PUT",
        "/api/v1/cloudflare/access/enforcement",
        &session,
        Some(json!({ "enforced": true })),
    )
    .await;
    assert_eq!(enforced.status, StatusCode::OK, "{}", enforced.body);
    assert_eq!(enforced.body["access_enforced"], true);
    // The panel domain now refuses requests without an Access token, except the
    // agent paths and CLI/Bearer API paths; other hostnames (the raw address, localhost) are untouched.
    let blocked = call_with(
        &app,
        "GET",
        "/api/v1/me",
        Some(&session),
        None,
        Some("panel.example.com"),
    )
    .await;
    assert_eq!(blocked.status, StatusCode::FORBIDDEN);
    let blocked_mcp_cookie = call_with(
        &app,
        "GET",
        "/mcp/api/v1/me",
        Some(&session),
        None,
        Some("panel.example.com"),
    )
    .await;
    assert_eq!(blocked_mcp_cookie.status, StatusCode::FORBIDDEN);
    let cli_start = call_with(
        &app,
        "POST",
        "/mcp/api/v1/cli/auth/start",
        None,
        Some(json!({ "method": "device_code", "client_name": "rudder-cli" })),
        Some("panel.example.com"),
    )
    .await;
    assert_eq!(cli_start.status, StatusCode::CREATED, "{}", cli_start.body);
    let exempt = call_with(
        &app,
        "GET",
        "/api/health",
        None,
        None,
        Some("panel.example.com"),
    )
    .await;
    assert_eq!(exempt.status, StatusCode::OK);
    let local = call_with(
        &app,
        "GET",
        "/api/v1/me",
        Some(&session),
        None,
        Some("localhost:3000"),
    )
    .await;
    assert_eq!(local.status, StatusCode::OK);
    let preflight = call(&app, "GET", "/api/v1/cloudflare/preflight", &session, None).await;
    let access_check = preflight.body["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|check| check["id"] == "access")
        .unwrap();
    assert_eq!(access_check["ok"], true);

    // Turning the bypass off removes its applications.
    let narrowed = call(
        &app,
        "PUT",
        "/api/v1/cloudflare/access",
        &session,
        Some(json!({ "team_domain": "https://myteam.cloudflareaccess.com/", "emails": ["admin@example.com"] })),
    )
    .await;
    assert_eq!(narrowed.status, StatusCode::OK, "{}", narrowed.body);
    assert_eq!(fake.lock().unwrap().apps.len(), 1);

    // SSH through Access for Infrastructure.
    for body in [
        json!({ "address": "127.0.0.1", "users": ["deploy"] }),
        json!({ "address": "not-an-ip", "users": ["deploy"] }),
        json!({ "address": "172.18.0.1", "users": [] }),
        json!({ "address": "172.18.0.1", "users": ["bad user"] }),
    ] {
        let rejected = call(&app, "PUT", "/api/v1/cloudflare/ssh", &session, Some(body)).await;
        assert_eq!(
            rejected.status,
            StatusCode::BAD_REQUEST,
            "{}",
            rejected.body
        );
    }
    let ssh = call(
        &app,
        "PUT",
        "/api/v1/cloudflare/ssh",
        &session,
        Some(json!({ "address": "172.18.0.1", "users": ["deploy", "root"] })),
    )
    .await;
    assert_eq!(ssh.status, StatusCode::OK, "{}", ssh.body);
    assert!(ssh.body["script"]
        .as_str()
        .unwrap()
        .contains("TrustedUserCAKeys"));
    assert!(ssh.body["script"].as_str().unwrap().contains(CA_KEY));
    assert_eq!(ssh.body["status"]["ssh_configured"], true);
    assert_eq!(ssh.body["status"]["ssh_ready"], false);
    assert!(calls(&fake)
        .iter()
        .any(|call| call.contains("route \"172.18.0.1/32\"")
            && call.contains("22222222-2222-2222-2222-222222222222")));
    {
        let fake = fake.lock().unwrap();
        let infra = fake
            .apps
            .values()
            .find(|app| app["type"] == "infrastructure")
            .unwrap();
        assert_eq!(
            infra["policies"][0]["connection_rules"]["ssh"]["usernames"],
            json!(["deploy", "root"])
        );
        assert_eq!(infra["target_criteria"][0]["port"], 22);
    }
    let script = call(&app, "GET", "/api/v1/cloudflare/ssh/script", &session, None).await;
    assert_eq!(script.status, StatusCode::OK);
    assert!(script.body["script"].as_str().unwrap().contains(CA_KEY));
    let confirmed = call(
        &app,
        "PUT",
        "/api/v1/cloudflare/ssh/confirmation",
        &session,
        Some(json!({ "confirmed": true })),
    )
    .await;
    assert_eq!(confirmed.body["ssh_ready"], true, "{}", confirmed.body);

    let removed_ssh = call(&app, "DELETE", "/api/v1/cloudflare/ssh", &session, None).await;
    assert_eq!(removed_ssh.status, StatusCode::NO_CONTENT);
    assert!(calls(&fake)
        .iter()
        .any(|call| call.starts_with("route deleted 172.18.0.1")));
    assert!(calls(&fake)
        .iter()
        .any(|call| call == "target deleted target-1"));
    let no_script = call(&app, "GET", "/api/v1/cloudflare/ssh/script", &session, None).await;
    assert_eq!(no_script.status, StatusCode::BAD_REQUEST);

    // Removing the tunnel restores origin TLS; opted-in services retain proxied DNS.
    let gone = call(&app, "DELETE", "/api/v1/cloudflare/tunnel", &session, None).await;
    assert_eq!(gone.status, StatusCode::OK, "{}", gone.body);
    assert_eq!(
        gone.body["removed_dns"],
        json!(["app.example.com", "panel.example.com"])
    );
    {
        let fake = fake.lock().unwrap();
        assert_eq!(fake.dns.len(), 1, "{:?}", fake.dns);
        assert_eq!(fake.dns[0]["type"], "A");
        assert_eq!(fake.dns[0]["name"], "app.example.com");
        assert_eq!(fake.dns[0]["content"], "203.0.113.10");
        assert_eq!(fake.dns[0]["proxied"], true);
        assert!(fake.apps.is_empty());
    }
    let after = call(&app, "GET", "/api/v1/cloudflare", &session, None).await;
    assert_eq!(after.body["tunnel_id"], Value::Null);
    assert_eq!(after.body["tunnel_published"], false);
    assert_eq!(after.body["access_enforced"], false);
    assert_eq!(after.body["token_saved"], true);
    let restored = std::fs::read_to_string(&routes_path).unwrap();
    assert!(restored.contains("certResolver"), "{restored}");
    let open = call_with(
        &app,
        "GET",
        "/api/v1/me",
        Some(&session),
        None,
        Some("panel.example.com"),
    )
    .await;
    assert_eq!(open.status, StatusCode::OK);
}
