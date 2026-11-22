use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::collections::HashMap;
use tower::ServiceExt;
use uuid::Uuid;

async fn request(
    app: &axum::Router,
    method: &str,
    path: &str,
    bearer: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("authorization", format!("Bearer {bearer}"))
                .header("content-type", "application/json")
                .body(Body::from(body.map(|v| v.to_string()).unwrap_or_default()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn fixture(
    pool: &PgPool,
    scopes: Vec<&str>,
    tools: Vec<&str>,
) -> (axum::Router, String, Uuid) {
    let vars = HashMap::from([
        (
            "DATABASE_URL".into(),
            std::env::var("DATABASE_URL").unwrap(),
        ),
        ("RUDDER_MASTER_KEY".into(), "cc".repeat(32)),
        ("RUDDER_COOKIE_SECURE".into(), "false".into()),
        ("RUDDER_PUBLIC_URL".into(), "https://panel.example".into()),
    ]);
    let app = rudder_server::router(rudder_server::AppState::new(
        pool.clone(),
        rudder_server::Config::from_map(&vars, |_| unreachable!()).unwrap(),
    ));
    let user: Uuid = sqlx::query_scalar("INSERT INTO users (email, password_hash, role, display_name) VALUES ('mcp@example.com','unused','admin','MCP owner') RETURNING id").fetch_one(pool).await.unwrap();
    sqlx::query("INSERT INTO oauth_clients(client_id, client_name, redirect_uris) VALUES ('test-client','Test client', ARRAY['http://127.0.0.1/callback'])").execute(pool).await.unwrap();
    let grant: Uuid = sqlx::query_scalar("INSERT INTO mcp_grants(user_id,client_id,name,template,scopes,allowed_tools) VALUES ($1,'test-client','Test approval','custom',$2,$3) RETURNING id").bind(user).bind(scopes).bind(tools).fetch_one(pool).await.unwrap();
    let token = format!("rdr_mca_{}", Uuid::new_v4());
    let hash = hex::encode(Sha256::digest(token.as_bytes()));
    sqlx::query("INSERT INTO mcp_access_tokens(token_hash,grant_id,expires_at) VALUES ($1,$2,now()+interval '1 hour')").bind(hash).bind(grant).execute(pool).await.unwrap();
    (app, token, grant)
}

async fn rpc(app: &axum::Router, token: &str, name: &str, arguments: Value) -> Value {
    request(app,"POST","/mcp",token,Some(json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":arguments}}))).await.1
}

#[sqlx::test(migrations = "./migrations")]
async fn delegated_keys_cannot_escalate_and_track_the_creating_client(pool: PgPool) {
    let (app, token, grant) = fixture(
        &pool,
        vec!["auth:read", "auth:write", "catalog:read"],
        vec![],
    )
    .await;
    let denied = rpc(
        &app,
        &token,
        "post_v1_tokens",
        json!({"body":{"name":"too-wide","scopes":["write"],"agent_scopes":["system:write"]}}),
    )
    .await;
    assert_eq!(denied["result"]["isError"], true);
    assert!(denied.to_string().contains("permission_escalation"));
    let created = rpc(&app,&token,"post_v1_tokens",json!({"body":{"name":"narrow","scopes":["read","write"],"agent_scopes":[],"allowed_tools":["get_v1_projects","post_v1_tokens"]}})).await;
    let text = created["result"]["content"][0]["text"].as_str().unwrap();
    let body: Value = serde_json::from_str(text.split_once('\n').unwrap().1).unwrap();
    let key = body["token"].as_str().unwrap();
    assert_eq!(body["created_by_mcp_client"], "test-client");
    assert_eq!(
        request(&app, "GET", "/api/v1/projects", key, None).await.0,
        StatusCode::OK
    );
    assert_eq!(
        request(
            &app,
            "POST",
            "/api/v1/projects",
            key,
            Some(json!({"name":"denied"}))
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(&app, "GET", "/api/v1/users", key, None).await.0,
        StatusCode::FORBIDDEN
    );
    let escalation = request(
        &app,
        "POST",
        "/api/v1/tokens",
        key,
        Some(json!({"name":"escape","scopes":["write"],"agent_scopes":["auth:write"]})),
    )
    .await;
    assert_eq!(escalation.0, StatusCode::FORBIDDEN);
    let listed = rpc(&app, &token, "get_v1_tokens", json!({})).await;
    assert!(!listed.to_string().contains(key));
    sqlx::query("UPDATE mcp_grants SET scopes = ARRAY['auth:read'] WHERE id=$1")
        .bind(grant)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        request(&app, "GET", "/api/v1/projects", key, None).await.0,
        StatusCode::FORBIDDEN
    );
    sqlx::query("UPDATE mcp_grants SET revoked_at = now() WHERE id=$1")
        .bind(grant)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        request(&app, "GET", "/api/v1/projects", key, None).await.0,
        StatusCode::UNAUTHORIZED
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn tool_grants_filter_enforce_and_edit_without_escalation(pool: PgPool) {
    let (app, token, grant) = fixture(
        &pool,
        vec!["auth:read"],
        vec!["post_v1_projects", "put_v1_mcp_grants__id_"],
    )
    .await;
    let listed = request(
        &app,
        "POST",
        "/mcp",
        &token,
        Some(json!({"jsonrpc":"2.0","id":1,"method":"tools/list"})),
    )
    .await
    .1;
    let names: Vec<&str> = listed["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"post_v1_projects"));
    assert!(names.contains(&"get_v1_me"));
    assert!(!names.contains(&"get_v1_projects"));
    assert_eq!(
        rpc(
            &app,
            &token,
            "post_v1_projects",
            json!({"body":{"name":"tool-only"}})
        )
        .await["result"]["isError"],
        false
    );
    assert_eq!(
        request(&app, "GET", "/api/v1/projects", &token, None)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let path = format!("/api/v1/mcp/grants/{grant}");
    assert_eq!(
        request(&app, "PUT", &path, &token, Some(json!({"template":"full"})))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            &app,
            "PUT",
            &path,
            &token,
            Some(json!({"template":"custom"}))
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        request(
            &app,
            "PUT",
            &path,
            &token,
            Some(json!({"template":"custom","allowed_tools":["unknown-tool"]}))
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        request(
            &app,
            "PUT",
            &path,
            &token,
            Some(json!({"template":"custom","allowed_tools":["post_v1_projects"]}))
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let after = request(
        &app,
        "POST",
        "/mcp",
        &token,
        Some(json!({"jsonrpc":"2.0","id":1,"method":"tools/list"})),
    )
    .await
    .1;
    assert_eq!(after["result"]["tools"].as_array().unwrap().len(), 2);
    assert_eq!(
        request(&app, "GET", "/api/v1/me", &token, None).await.0,
        StatusCode::FORBIDDEN
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn calls_record_success_denial_error_and_never_arguments(pool: PgPool) {
    let (app, token, grant) = fixture(&pool, vec!["catalog:read", "monitoring:read"], vec![]).await;
    rpc(&app, &token, "get_v1_projects", json!({})).await;
    rpc(
        &app,
        &token,
        "post_v1_projects",
        json!({"body":{"name":"private-argument-marker"}}),
    )
    .await;
    rpc(
        &app,
        &token,
        "get_v1_projects__id_",
        json!({"path":{"id":"private-argument-marker"}}),
    )
    .await;
    rpc(
        &app,
        &token,
        "private-argument-marker",
        json!({"secret":"private-argument-marker"}),
    )
    .await;
    rpc(&app, "rdr_mca_invalid", "get_v1_projects", json!({})).await;
    let recent = request(&app, "GET", "/api/v1/mcp/calls", &token, None).await;
    assert_eq!(recent.0, StatusCode::OK, "{}", recent.1);
    let rows = recent.1.as_array().unwrap();
    assert_eq!(rows.len(), 5);
    assert!(!recent.1.to_string().contains("private-argument-marker"));
    assert!(!recent.1.to_string().contains(&token));
    assert!(rows.iter().any(|r| r["status"] == "ok"
        && r["http_status"] == 200
        && r["grant_id"] == grant.to_string()));
    assert!(rows
        .iter()
        .any(|r| r["status"] == "error" && r["http_status"] == 400));
    assert!(rows
        .iter()
        .any(|r| r["status"] == "denied" && r["http_status"] == 401 && r["grant_id"].is_null()));
    let filtered = request(
        &app,
        "GET",
        "/api/v1/mcp/calls?client_id=test-client&status=denied&tool=post_v1_projects",
        &token,
        None,
    )
    .await;
    assert_eq!(filtered.1.as_array().unwrap().len(), 1);
    let empty = request(
        &app,
        "GET",
        "/api/v1/mcp/calls?client_id=absent",
        &token,
        None,
    )
    .await;
    assert_eq!(empty.1, json!([]));
    let via_mcp = rpc(
        &app,
        &token,
        "get_v1_mcp_calls",
        json!({"query":{"status":"ok"}}),
    )
    .await;
    assert_eq!(via_mcp["result"]["isError"], false);
}

#[sqlx::test(migrations = "./migrations")]
async fn statistics_aggregate_filter_and_zero_fill_hour_and_day_buckets(pool: PgPool) {
    let (app, token, grant) = fixture(&pool, vec!["monitoring:read"], vec![]).await;
    for (status, latency, tool) in [
        ("ok", 10, "first"),
        ("error", 20, "first"),
        ("denied", 30, "second"),
        ("ok", 40, "second"),
    ] {
        sqlx::query("INSERT INTO mcp_calls(grant_id,client_id,tool,scope,status,http_status,duration_ms,created_at) VALUES ($1,'test-client',$2,'monitoring:read',$3,200,$4,now()-interval '2 hours')").bind(grant).bind(tool).bind(status).bind(i64::from(latency)).execute(&pool).await.unwrap();
    }
    sqlx::query("INSERT INTO mcp_calls(client_id,tool,scope,status,http_status,duration_ms,created_at) VALUES ('test-client','old','self','ok',200,9999,now()-interval '31 days'),('other','other','self','ok',200,100,now())").execute(&pool).await.unwrap();
    for bucket in ["hour", "day"] {
        let stats = request(
            &app,
            "GET",
            &format!("/api/v1/mcp/stats?client_id=test-client&bucket={bucket}"),
            &token,
            None,
        )
        .await;
        assert_eq!(stats.0, StatusCode::OK, "{}", stats.1);
        assert_eq!(stats.1["summary"]["total"], 4);
        assert_eq!(stats.1["summary"]["error_rate"], 0.25);
        assert_eq!(stats.1["summary"]["denied_rate"], 0.25);
        assert_eq!(stats.1["summary"]["p50_ms"], 25.0);
        assert!((stats.1["summary"]["p95_ms"].as_f64().unwrap() - 38.5).abs() < 0.001);
        let buckets = stats.1["buckets"].as_array().unwrap();
        assert_eq!(
            buckets
                .iter()
                .map(|b| b["count"].as_i64().unwrap())
                .sum::<i64>(),
            4
        );
        assert!(buckets.iter().any(|b| b["count"] == 0));
        assert_eq!(stats.1["tools"].as_array().unwrap().len(), 2);
        assert_eq!(stats.1["clients"][0]["count"], 4);
        assert_eq!(stats.1["grants"][0]["grant_id"], grant.to_string());
        assert!(stats.1["grants"][0]["last_used_at"].is_string());
    }
    let empty = request(
        &app,
        "GET",
        "/api/v1/mcp/stats?client_id=missing",
        &token,
        None,
    )
    .await;
    assert_eq!(empty.1["summary"]["total"], 0);
    assert_eq!(empty.1["summary"]["error_rate"], 0.0);
    assert!(empty.1["summary"]["p95_ms"].is_null());
    for invalid in [
        "days=31",
        "days=0",
        "bucket=week",
        "status=bad",
        "limit=0",
        "offset=-1",
    ] {
        assert_eq!(
            request(
                &app,
                "GET",
                &format!("/api/v1/mcp/stats?{invalid}"),
                &token,
                None
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
    }
    let filtered = request(
        &app,
        "GET",
        "/api/v1/mcp/stats?status=error&tool=first",
        &token,
        None,
    )
    .await;
    assert_eq!(filtered.1["summary"]["total"], 1);
}

#[sqlx::test(migrations = "./migrations")]
async fn missing_call_storage_prevents_actions_from_running(pool: PgPool) {
    let (app, token, _) = fixture(&pool, vec!["catalog:write"], vec![]).await;
    sqlx::query("DROP TABLE mcp_calls")
        .execute(&pool)
        .await
        .unwrap();
    let denied = rpc(
        &app,
        &token,
        "post_v1_projects",
        json!({"body":{"name":"must-not-exist"}}),
    )
    .await;
    assert_eq!(denied["error"]["code"], -32603);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM projects")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}
