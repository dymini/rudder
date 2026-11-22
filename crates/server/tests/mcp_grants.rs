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
