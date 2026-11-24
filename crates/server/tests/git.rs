//! Git provider storage, webhook checks, and the build queue.

use std::collections::HashMap;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tower::ServiceExt;

fn state(pool: PgPool) -> rudder_server::AppState {
    let url =
        std::env::var("DATABASE_URL").unwrap_or_else(|_| "postgres://localhost/rudder".into());
    let data_dir = std::env::temp_dir().join(format!("rudder-git-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&data_dir).unwrap();
    let vars = HashMap::from([
        ("DATABASE_URL".into(), url),
        ("RUDDER_MASTER_KEY".into(), "cc".repeat(32)),
        ("RUDDER_SETUP_TOKEN".into(), "test-setup-token".into()),
        ("RUDDER_COOKIE_SECURE".into(), "false".into()),
        ("RUDDER_PUBLIC_URL".into(), "https://panel.example".into()),
        ("RUDDER_DATA_DIR".into(), data_dir.display().to_string()),
    ]);
    let config = rudder_server::Config::from_map(&vars, |_| unreachable!()).unwrap();
    rudder_server::AppState::new(pool, config)
}

struct Reply {
    status: StatusCode,
    body: Value,
    cookie: Option<String>,
}

async fn call(
    app: &axum::Router,
    method: &str,
    path: &str,
    cookie: Option<&str>,
    csrf: Option<&str>,
    body: Option<Value>,
) -> Reply {
    let bytes = body.map(|value| value.to_string());
    call_raw(app, method, path, cookie, csrf, bytes.as_deref(), &[]).await
}

async fn call_raw(
    app: &axum::Router,
    method: &str,
    path: &str,
    cookie: Option<&str>,
    csrf: Option<&str>,
    body: Option<&str>,
    extra: &[(&str, &str)],
) -> Reply {
    let mut builder = Request::builder().method(method).uri(path);
    if let Some(cookie) = cookie {
        builder = builder.header("cookie", format!("rudder_session={cookie}"));
    }
    if let Some(csrf) = csrf {
        builder = builder.header("x-csrf-token", csrf);
    }
    for (name, value) in extra {
        builder = builder.header(*name, *value);
    }
    let request = if let Some(body) = body {
        builder
            .header("content-type", "application/json")
            .body(Body::from(body.to_owned()))
            .unwrap()
    } else {
        builder.body(Body::empty()).unwrap()
    };
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let cookie = response
        .headers()
        .get("set-cookie")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .and_then(|pair| pair.strip_prefix("rudder_session="))
        .map(str::to_owned);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    Reply {
        status,
        body,
        cookie,
    }
}

fn hmac_sha256(key: &[u8], message: &[u8]) -> String {
    const BLOCK: usize = 64;
    let mut key_block = [0u8; BLOCK];
    if key.len() > BLOCK {
        let digest = Sha256::digest(key);
        key_block[..digest.len()].copy_from_slice(&digest);
    } else {
        key_block[..key.len()].copy_from_slice(key);
    }
    let mut ipad = [0u8; BLOCK];
    let mut opad = [0u8; BLOCK];
    for (index, byte) in key_block.iter().enumerate() {
        ipad[index] = byte ^ 0x36;
        opad[index] = byte ^ 0x5c;
    }
    let mut inner = Sha256::new();
    inner.update(ipad);
    inner.update(message);
    let inner = inner.finalize();
    let mut outer = Sha256::new();
    outer.update(opad);
    outer.update(inner);
    hex::encode(outer.finalize())
}

#[sqlx::test(migrations = "./migrations")]
async fn builders_inventory_creation_validation_offline_cache_and_permissions(pool: PgPool) {
    let state = state(pool.clone());
    let app = rudder_server::router(state.clone());
    let (cookie, csrf) = owner(&app).await;
    let empty = call(&app, "GET", "/api/v1/builders", Some(&cookie), None, None).await;
    assert_eq!(empty.status, StatusCode::OK);
    assert_eq!(empty.body["builders"], json!([]));
    assert_eq!(empty.body["waiting_jobs"], json!([]));
    let created = call(
        &app,
        "POST",
        "/api/v1/builders",
        Some(&cookie),
        Some(&csrf),
        Some(json!({"name":"home"})),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let id = created.body["server"]["id"].as_str().unwrap();
    assert_eq!(created.body["server"]["enabled"], true);
    assert_eq!(created.body["server"]["online"], false);
    assert_eq!(created.body["server"]["max_jobs"], 1);
    assert_eq!(created.body["server"]["cache_bytes"], 0);
    let command = created.body["join_command"].as_str().unwrap();
    assert!(command.contains("--ca-sha256"));
    assert!(command.ends_with("--builder --no-edge"));
    let services: i64 = sqlx::query_scalar("SELECT count(*) FROM services WHERE server_id = $1")
        .bind(id.parse::<uuid::Uuid>().unwrap())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(services, 0);
    let duplicate = call(
        &app,
        "POST",
        "/api/v1/builders",
        Some(&cookie),
        Some(&csrf),
        Some(json!({"name":"home"})),
    )
    .await;
    assert_eq!(duplicate.status, StatusCode::CONFLICT);
    for (body, rule) in [
        (json!({"max_jobs":0}), "1 to 32"),
        (json!({"max_jobs":33}), "1 to 32"),
        (json!({"cache_limit_bytes":-1}), "non-negative"),
    ] {
        let reply = call(
            &app,
            "PATCH",
            &format!("/api/v1/builders/{id}"),
            Some(&cookie),
            Some(&csrf),
            Some(body),
        )
        .await;
        assert_eq!(reply.status, StatusCode::BAD_REQUEST);
        assert!(reply.body["error"]["message"]
            .as_str()
            .unwrap()
            .contains(rule));
    }
    let update = call(&app, "PATCH", &format!("/api/v1/builders/{id}"), Some(&cookie), Some(&csrf), Some(json!({"enabled":false,"paused":true,"draining":true,"max_jobs":32,"cache_limit_bytes":0}))).await;
    assert_eq!(update.status, StatusCode::OK);
    assert_eq!(update.body["max_jobs"], 32);
    let list = call(&app, "GET", "/api/v1/builders", Some(&cookie), None, None).await;
    assert_eq!(list.body["builders"][0]["enabled"], false);
    assert!(!list
        .body
        .to_string()
        .contains(created.body["join_token"].as_str().unwrap()));
    for suffix in ["cache", "jobs"] {
        let reply = call(
            &app,
            "GET",
            &format!("/api/v1/builders/{id}/{suffix}"),
            Some(&cookie),
            None,
            None,
        )
        .await;
        assert_eq!(reply.status, StatusCode::OK);
        assert_eq!(reply.body, json!([]));
    }
    let invalid = call(
        &app,
        "DELETE",
        &format!("/api/v1/builders/{id}/cache?key=../main"),
        Some(&cookie),
        Some(&csrf),
        None,
    )
    .await;
    assert_eq!(invalid.status, StatusCode::BAD_REQUEST);
    for (method, suffix) in [("DELETE", "cache"), ("POST", "cache/prune")] {
        let reply = call(
            &app,
            method,
            &format!("/api/v1/builders/{id}/{suffix}"),
            Some(&cookie),
            Some(&csrf),
            None,
        )
        .await;
        assert_eq!(reply.status, StatusCode::CONFLICT);
        assert_eq!(reply.body["error"]["code"], "builder_offline");
    }
    let missing = call(
        &app,
        "GET",
        &format!("/api/v1/builders/{}/jobs", uuid::Uuid::new_v4()),
        Some(&cookie),
        None,
        None,
    )
    .await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
    call(
        &app,
        "POST",
        "/api/v1/users",
        Some(&cookie),
        Some(&csrf),
        Some(json!({"email":"viewer@example.com","password":"viewer-password-ok","role":"viewer"})),
    )
    .await;
    let viewer = call(
        &app,
        "POST",
        "/api/v1/login",
        None,
        None,
        Some(json!({"email":"viewer@example.com","password":"viewer-password-ok"})),
    )
    .await;
    let forbidden = call(
        &app,
        "PATCH",
        &format!("/api/v1/builders/{id}"),
        viewer.cookie.as_deref(),
        viewer.body["csrf_token"].as_str(),
        Some(json!({"enabled":true})),
    )
    .await;
    assert_eq!(forbidden.status, StatusCode::FORBIDDEN);
    let allowed = call(
        &app,
        "GET",
        "/api/v1/builders",
        viewer.cookie.as_deref(),
        None,
        None,
    )
    .await;
    assert_eq!(allowed.status, StatusCode::OK);
}

#[sqlx::test(migrations = "./migrations")]
async fn source_build_settings_validate_builder_and_preserve_settings_when_omitted(pool: PgPool) {
    let app = rudder_server::router(state(pool.clone()));
    let (cookie, csrf) = owner(&app).await;
    let server: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO servers (name,kind) VALUES ('remote','remote') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let project: uuid::Uuid =
        sqlx::query_scalar("INSERT INTO projects (name) VALUES ('builds') RETURNING id")
            .fetch_one(&pool)
            .await
            .unwrap();
    let environment: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO environments (name,project_id) VALUES ('prod',$1) RETURNING id",
    )
    .bind(project)
    .fetch_one(&pool)
    .await
    .unwrap();
    let service: uuid::Uuid = sqlx::query_scalar("INSERT INTO services (name,kind,server_id,environment_id) VALUES ('app','application',$1,$2) RETURNING id").bind(server).bind(environment).fetch_one(&pool).await.unwrap();
    let provider = call(
        &app,
        "POST",
        "/api/v1/git-providers",
        Some(&cookie),
        Some(&csrf),
        Some(json!({"name":"github","kind":"github_pat","token":"fake-test-token"})),
    )
    .await;
    let path = format!("/api/v1/services/{service}/source");
    let mut source = json!({"provider_id":provider.body["id"],"repo":"https://github.com/example/app.git","branch":"main","strategy":"dockerfile"});
    for (build, rule) in [
        (json!({"placement":"bad"}), "placement"),
        (json!({"fallback":"bad"}), "fallback"),
        (json!({"timeout_secs":0}), "positive"),
        (json!({"placement":"builder"}), "required"),
        (json!({"builder_id":uuid::Uuid::new_v4()}), "does not exist"),
        (json!({"builder_id":server}), "enable builder mode"),
    ] {
        source["build"] = build;
        let reply = call(
            &app,
            "PUT",
            &path,
            Some(&cookie),
            Some(&csrf),
            Some(source.clone()),
        )
        .await;
        assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{}", reply.body);
        assert!(reply.body["error"]["message"]
            .as_str()
            .unwrap()
            .contains(rule));
    }
    let enabled = call(
        &app,
        "PATCH",
        &format!("/api/v1/builders/{server}"),
        Some(&cookie),
        Some(&csrf),
        Some(json!({"enabled":true})),
    )
    .await;
    assert_eq!(enabled.status, StatusCode::OK);
    let build = json!({"placement":"builder","builder_id":server,"fallback":"wait","timeout_secs":120,"test_command":"test -f /app/ready"});
    source["build"] = build.clone();
    let saved = call(
        &app,
        "PUT",
        &path,
        Some(&cookie),
        Some(&csrf),
        Some(source.clone()),
    )
    .await;
    assert_eq!(saved.status, StatusCode::OK, "{}", saved.body);
    assert_eq!(saved.body["build"], build);
    source.as_object_mut().unwrap().remove("build");
    let saved = call(&app, "PUT", &path, Some(&cookie), Some(&csrf), Some(source)).await;
    assert_eq!(saved.body["build"], build);
    let fetched = call(&app, "GET", &path, Some(&cookie), None, None).await;
    assert_eq!(fetched.body["build"], build);
}

async fn owner(app: &axum::Router) -> (String, String) {
    let created = call(
        app,
        "POST",
        "/api/v1/setup",
        None,
        None,
        Some(json!({
            "setup_token": "test-setup-token",
            "email": "owner@example.com",
            "password": "correct-horse-battery",
            "display_name": "Owner"
        })),
    )
    .await;
    assert_eq!(created.status, StatusCode::OK);
    (
        created.cookie.expect("session cookie"),
        created.body["csrf_token"].as_str().unwrap().to_owned(),
    )
}

#[sqlx::test(migrations = "./migrations")]
async fn webhook_enqueues_and_remote_build_fails_closed(pool: PgPool) {
    let state = state(pool);
    let app = rudder_server::router(state.clone());
    let (cookie, csrf) = owner(&app).await;
    let docs = call(&app, "GET", "/api/openapi.json", None, None, None).await;
    let document = docs.body.to_string();
    assert!(document.contains("/api/v1/git-providers"));
    assert!(document.contains("/api/v1/services/{id}/webhook"));

    let server = call(
        &app,
        "POST",
        "/api/v1/servers",
        Some(&cookie),
        Some(&csrf),
        Some(json!({ "name": "local", "kind": "local" })),
    )
    .await;
    assert_eq!(server.status, StatusCode::CREATED);
    let server_id = server.body["id"].as_str().unwrap();
    let remote = call(
        &app,
        "POST",
        "/api/v1/servers",
        Some(&cookie),
        Some(&csrf),
        Some(json!({ "name": "edge", "kind": "remote" })),
    )
    .await;
    assert_eq!(remote.status, StatusCode::CREATED);
    let remote_id = remote.body["id"].as_str().unwrap();

    let project = call(
        &app,
        "POST",
        "/api/v1/projects",
        Some(&cookie),
        Some(&csrf),
        Some(json!({ "name": "web" })),
    )
    .await;
    let project_id = project.body["id"].as_str().unwrap();
    let environment = call(
        &app,
        "POST",
        &format!("/api/v1/projects/{project_id}/environments"),
        Some(&cookie),
        Some(&csrf),
        Some(json!({ "name": "prod" })),
    )
    .await;
    let environment_id = environment.body["id"].as_str().unwrap();

    let token = "github-pat-secret-value";
    let provider = call(
        &app,
        "POST",
        "/api/v1/git-providers",
        Some(&cookie),
        Some(&csrf),
        Some(json!({
            "name": "github",
            "kind": "github_pat",
            "token": token
        })),
    )
    .await;
    assert_eq!(provider.status, StatusCode::CREATED);
    assert!(!provider.body.to_string().contains(token));
    let provider_id = provider.body["id"].as_str().unwrap();
    let listed = call(
        &app,
        "GET",
        "/api/v1/git-providers",
        Some(&cookie),
        None,
        None,
    )
    .await;
    assert!(!listed.body.to_string().contains(token));

    let service = call(
        &app,
        "POST",
        &format!("/api/v1/environments/{environment_id}/services"),
        Some(&cookie),
        Some(&csrf),
        Some(json!({
            "name": "web",
            "kind": "application",
            "server_id": server_id
        })),
    )
    .await;
    let service_id = service.body["id"].as_str().unwrap();
    let source = call(
        &app,
        "PUT",
        &format!("/api/v1/services/{service_id}/source"),
        Some(&cookie),
        Some(&csrf),
        Some(json!({
            "provider_id": provider_id,
            "repo": "https://github.com/acme/web.git",
            "branch": "main",
            "strategy": "dockerfile"
        })),
    )
    .await;
    assert_eq!(source.status, StatusCode::OK);
    let secret = source.body["webhook_secret"].as_str().unwrap().to_owned();
    assert!(!source.body.to_string().contains(token));
    let loaded = call(
        &app,
        "GET",
        &format!("/api/v1/services/{service_id}"),
        Some(&cookie),
        None,
        None,
    )
    .await;
    assert!(!loaded.body.to_string().contains(&secret));
    assert!(!loaded.body.to_string().contains(token));

    let payload = r#"{"ref":"refs/heads/main","after":"abcdef1"}"#;
    let bad = call_raw(
        &app,
        "POST",
        &format!("/api/v1/services/{service_id}/webhook"),
        None,
        None,
        Some(payload),
        &[
            ("x-github-event", "push"),
            (
                "x-hub-signature-256",
                "sha256=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            ),
        ],
    )
    .await;
    assert_eq!(bad.status, StatusCode::UNAUTHORIZED);

    let signature = format!(
        "sha256={}",
        hmac_sha256(secret.as_bytes(), payload.as_bytes())
    );
    let accepted = call_raw(
        &app,
        "POST",
        &format!("/api/v1/services/{service_id}/webhook"),
        None,
        None,
        Some(payload),
        &[
            ("x-github-event", "push"),
            ("x-hub-signature-256", &signature),
        ],
    )
    .await;
    assert_eq!(accepted.status, StatusCode::ACCEPTED);
    assert_eq!(accepted.body["deploy"], true);
    let queue_id = accepted.body["queue_id"].as_str().unwrap();

    let other = r#"{"ref":"refs/heads/dev","after":"abcdef1"}"#;
    let other_sig = format!(
        "sha256={}",
        hmac_sha256(secret.as_bytes(), other.as_bytes())
    );
    let filtered = call_raw(
        &app,
        "POST",
        &format!("/api/v1/services/{service_id}/webhook"),
        None,
        None,
        Some(other),
        &[
            ("x-github-event", "push"),
            ("x-hub-signature-256", &other_sig),
        ],
    )
    .await;
    assert_eq!(filtered.status, StatusCode::OK);
    assert_eq!(filtered.body["deploy"], false);

    let cancel = call(
        &app,
        "POST",
        &format!("/api/v1/build-queue/{queue_id}/cancel"),
        Some(&cookie),
        Some(&csrf),
        None,
    )
    .await;
    assert_eq!(cancel.status, StatusCode::NO_CONTENT);

    let remote_service = call(
        &app,
        "POST",
        &format!("/api/v1/environments/{environment_id}/services"),
        Some(&cookie),
        Some(&csrf),
        Some(json!({
            "name": "api",
            "kind": "application",
            "server_id": remote_id
        })),
    )
    .await;
    let remote_service_id = remote_service.body["id"].as_str().unwrap();
    let remote_source = call(
        &app,
        "PUT",
        &format!("/api/v1/services/{remote_service_id}/source"),
        Some(&cookie),
        Some(&csrf),
        Some(json!({
            "provider_id": provider_id,
            "repo": "https://github.com/acme/api.git",
            "branch": "main",
            "strategy": "nixpacks"
        })),
    )
    .await;
    assert_eq!(remote_source.status, StatusCode::OK);
    let queued = call(
        &app,
        "POST",
        &format!("/api/v1/services/{remote_service_id}/build"),
        Some(&cookie),
        Some(&csrf),
        None,
    )
    .await;
    assert_eq!(queued.status, StatusCode::ACCEPTED);
    assert!(rudder_server::build_once(&state).await.unwrap());
    let deployments = call(
        &app,
        "GET",
        &format!("/api/v1/services/{remote_service_id}/deployments"),
        Some(&cookie),
        None,
        None,
    )
    .await;
    let rendered = deployments.body.to_string();
    assert!(rendered.contains("failed"));
    assert!(rendered.contains("agent"));
    assert!(!rendered.contains(token));

    let viewer = call(
        &app,
        "POST",
        "/api/v1/users",
        Some(&cookie),
        Some(&csrf),
        Some(json!({
            "email": "viewer@example.com",
            "password": "viewer-password-ok",
            "role": "viewer"
        })),
    )
    .await;
    assert_eq!(viewer.status, StatusCode::CREATED);
    let login = call(
        &app,
        "POST",
        "/api/v1/login",
        None,
        None,
        Some(json!({
            "email": "viewer@example.com",
            "password": "viewer-password-ok"
        })),
    )
    .await;
    let denied = call(
        &app,
        "POST",
        "/api/v1/git-providers",
        login.cookie.as_deref(),
        login.body["csrf_token"].as_str(),
        Some(json!({
            "name": "nope",
            "kind": "github_pat",
            "token": "another-secret-token"
        })),
    )
    .await;
    assert_eq!(denied.status, StatusCode::FORBIDDEN);
}
