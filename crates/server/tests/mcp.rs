//! Agent login: approval in the browser, rotating refresh tokens, and MCP tools.

use std::collections::HashMap;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::Engine;
use http_body_util::BodyExt;
use rudder_server::AppState;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tower::ServiceExt;

fn test_config(database_url: &str) -> rudder_server::Config {
    let vars = HashMap::from([
        ("DATABASE_URL".into(), database_url.to_owned()),
        ("RUDDER_MASTER_KEY".into(), "cc".repeat(32)),
        ("RUDDER_SETUP_TOKEN".into(), "test-setup-token".into()),
        ("RUDDER_COOKIE_SECURE".into(), "false".into()),
        ("RUDDER_LOG_FORMAT".into(), "pretty".into()),
        ("RUDDER_PUBLIC_URL".into(), "https://panel.example".into()),
    ]);
    rudder_server::Config::from_map(&vars, |_| unreachable!("no key file")).expect("config")
}

fn app(pool: PgPool) -> axum::Router {
    let url =
        std::env::var("DATABASE_URL").unwrap_or_else(|_| "postgres://localhost/rudder".into());
    rudder_server::router(AppState::new(pool, test_config(&url)))
}

struct Reply {
    status: StatusCode,
    body: Value,
    headers: axum::http::HeaderMap,
}

#[allow(clippy::too_many_arguments)]
async fn call(
    app: &axum::Router,
    method: &str,
    path: &str,
    cookie: Option<&str>,
    csrf: Option<&str>,
    bearer: Option<&str>,
    content_type: Option<&str>,
    body: Option<Vec<u8>>,
) -> Reply {
    let mut builder = Request::builder().method(method).uri(path);
    if let Some(cookie) = cookie {
        builder = builder.header("cookie", format!("rudder_session={cookie}"));
    }
    if let Some(csrf) = csrf {
        builder = builder.header("x-csrf-token", csrf);
    }
    if let Some(bearer) = bearer {
        builder = builder.header("authorization", format!("Bearer {bearer}"));
    }
    if let Some(content_type) = content_type {
        builder = builder.header("content-type", content_type);
    }
    let request = builder.body(Body::from(body.unwrap_or_default())).unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes)
            .unwrap_or(Value::String(String::from_utf8_lossy(&bytes).into()))
    };
    Reply {
        status,
        body,
        headers,
    }
}

fn session_of(reply: &Reply) -> (String, String) {
    let cookie = reply
        .headers
        .get("set-cookie")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .and_then(|pair| pair.strip_prefix("rudder_session="))
        .unwrap()
        .to_owned();
    let csrf = reply.body["csrf_token"].as_str().unwrap().to_owned();
    (cookie, csrf)
}

fn s256(verifier: &str) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

fn code_from(redirect: &str) -> String {
    url::Url::parse(redirect)
        .unwrap()
        .query_pairs()
        .find(|(key, _)| key == "code")
        .unwrap()
        .1
        .into_owned()
}

async fn owner(app: &axum::Router) -> (String, String) {
    let created = call(
        app,
        "POST",
        "/api/v1/setup",
        None,
        None,
        None,
        Some("application/json"),
        Some(
            json!({
                "setup_token": "test-setup-token",
                "email": "owner@example.com",
                "password": "correct-horse-battery",
                "display_name": "Owner"
            })
            .to_string()
            .into_bytes(),
        ),
    )
    .await;
    assert_eq!(created.status, StatusCode::OK, "{:?}", created.body);
    session_of(&created)
}

#[sqlx::test(migrations = "./migrations")]
async fn agent_login_refreshes_and_can_control_the_panel(pool: PgPool) {
    let app = app(pool);
    let meta = call(
        &app,
        "GET",
        "/.well-known/oauth-authorization-server",
        None,
        None,
        None,
        None,
        None,
    )
    .await;
    assert_eq!(meta.status, StatusCode::OK);
    assert_eq!(
        meta.body["authorization_endpoint"],
        "https://panel.example/oauth/authorize"
    );
    assert!(meta.headers.get("cache-control").is_some());

    let registered = call(
        &app,
        "POST",
        "/oauth/register",
        None,
        None,
        None,
        Some("application/json"),
        Some(
            json!({
                "client_name": "Cursor",
                "redirect_uris": ["http://127.0.0.1:9/callback"],
                "token_endpoint_auth_method": "none",
                "grant_types": ["authorization_code", "refresh_token"],
                "response_types": ["code"]
            })
            .to_string()
            .into_bytes(),
        ),
    )
    .await;
    assert_eq!(
        registered.status,
        StatusCode::CREATED,
        "{:?}",
        registered.body
    );
    let client_id = registered.body["client_id"].as_str().unwrap();

    let bad = call(
        &app,
        "POST",
        "/oauth/register",
        None,
        None,
        None,
        Some("application/json"),
        Some(
            json!({"client_name": "Bad", "redirect_uris": ["http://evil.example/callback"]})
                .to_string()
                .into_bytes(),
        ),
    )
    .await;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST);

    let verifier = "a".repeat(64);
    let challenge = s256(&verifier);
    let start = format!(
        "/oauth/authorize?response_type=code&client_id={client_id}&redirect_uri={}&code_challenge={challenge}&code_challenge_method=S256&state=xyz&resource=https://panel.example/mcp",
        percent("http://127.0.0.1:9/callback")
    );
    let anonymous = call(&app, "GET", &start, None, None, None, None, None).await;
    assert_eq!(anonymous.status, StatusCode::TEMPORARY_REDIRECT);
    let location = anonymous.headers.get("location").unwrap().to_str().unwrap();
    assert!(location.starts_with("/login?next="), "{location}");

    let (cookie, csrf) = owner(&app).await;
    let signed = call(&app, "GET", &start, Some(&cookie), None, None, None, None).await;
    assert_eq!(signed.status, StatusCode::TEMPORARY_REDIRECT);
    assert!(signed
        .headers
        .get("location")
        .unwrap()
        .to_str()
        .unwrap()
        .starts_with("/mcp/consent?"));

    let approved = call(
        &app,
        "POST",
        "/api/v1/mcp/consent",
        Some(&cookie),
        Some(&csrf),
        None,
        Some("application/json"),
        Some(
            json!({
                "client_id": client_id,
                "redirect_uri": "http://127.0.0.1:9/callback",
                "code_challenge": challenge,
                "code_challenge_method": "S256",
                "state": "xyz",
                "template": "full",
                "name": "Laptop"
            })
            .to_string()
            .into_bytes(),
        ),
    )
    .await;
    assert_eq!(approved.status, StatusCode::OK, "{:?}", approved.body);
    let code = code_from(approved.body["redirect"].as_str().unwrap());

    let wrong = token_request(
        &app,
        &format!(
            "grant_type=authorization_code&code={code}&redirect_uri={}&client_id={client_id}&code_verifier={}",
            percent("http://127.0.0.1:9/callback"),
            "b".repeat(64)
        ),
    )
    .await;
    assert_eq!(wrong.status, StatusCode::BAD_REQUEST);
    assert_eq!(wrong.body["error"], "invalid_grant");

    let approved = call(
        &app,
        "POST",
        "/api/v1/mcp/consent",
        Some(&cookie),
        Some(&csrf),
        None,
        Some("application/json"),
        Some(
            json!({
                "client_id": client_id,
                "redirect_uri": "http://127.0.0.1:9/callback",
                "code_challenge": challenge,
                "code_challenge_method": "S256",
                "state": "xyz",
                "template": "full"
            })
            .to_string()
            .into_bytes(),
        ),
    )
    .await;
    let code = code_from(approved.body["redirect"].as_str().unwrap());
    let tokens = token_request(
        &app,
        &format!(
            "grant_type=authorization_code&code={code}&redirect_uri={}&client_id={client_id}&code_verifier={verifier}",
            percent("http://127.0.0.1:9/callback")
        ),
    )
    .await;
    assert_eq!(tokens.status, StatusCode::OK, "{:?}", tokens.body);
    let access = tokens.body["access_token"].as_str().unwrap().to_owned();
    let refresh = tokens.body["refresh_token"].as_str().unwrap().to_owned();
    assert!(access.starts_with("rdr_mca_"));
    assert_eq!(tokens.body["expires_in"], 900);
    assert!(tokens.body["scope"]
        .as_str()
        .unwrap()
        .contains("auth:write"));

    let listed = rpc(
        &app,
        &access,
        json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}),
    )
    .await;
    assert_eq!(listed.status, StatusCode::OK);
    let names: Vec<&str> = listed.body["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"rudder_whoami"));
    assert!(names.contains(&"get_v1_projects"));
    assert!(names.contains(&"post_v1_projects"));
    assert!(!names.contains(&"post_v1_login"));

    let me = rpc(
        &app,
        &access,
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {"name": "get_v1_me", "arguments": {}}
        }),
    )
    .await;
    let text = me.body["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("owner@example.com"), "{text}");
    assert_eq!(me.body["result"]["isError"], false);

    let created = rpc(
        &app,
        &access,
        json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "post_v1_projects",
                "arguments": {"body": {"name": "from-agent"}}
            }
        }),
    )
    .await;
    let text = created.body["result"]["content"][0]["text"]
        .as_str()
        .unwrap();
    assert!(text.starts_with("HTTP 201"), "{text}");

    let refreshed = token_request(
        &app,
        &format!("grant_type=refresh_token&refresh_token={refresh}&client_id={client_id}"),
    )
    .await;
    assert_eq!(refreshed.status, StatusCode::OK, "{:?}", refreshed.body);
    let refresh_2 = refreshed.body["refresh_token"].as_str().unwrap().to_owned();
    let access_2 = refreshed.body["access_token"].as_str().unwrap().to_owned();

    let reused = token_request(
        &app,
        &format!("grant_type=refresh_token&refresh_token={refresh}&client_id={client_id}"),
    )
    .await;
    assert_eq!(reused.status, StatusCode::BAD_REQUEST);
    assert_eq!(reused.body["error"], "invalid_grant");
    let after = token_request(
        &app,
        &format!("grant_type=refresh_token&refresh_token={refresh_2}&client_id={client_id}"),
    )
    .await;
    assert_eq!(after.status, StatusCode::BAD_REQUEST);

    let dead = rpc(
        &app,
        &access_2,
        json!({"jsonrpc": "2.0", "id": 4, "method": "ping"}),
    )
    .await;
    assert_eq!(dead.status, StatusCode::UNAUTHORIZED);

    let watch = call(
        &app,
        "POST",
        "/api/v1/mcp/consent",
        Some(&cookie),
        Some(&csrf),
        None,
        Some("application/json"),
        Some(
            json!({
                "client_id": client_id,
                "redirect_uri": "http://127.0.0.1:9/callback",
                "code_challenge": challenge,
                "code_challenge_method": "S256",
                "state": "watch",
                "template": "read-all"
            })
            .to_string()
            .into_bytes(),
        ),
    )
    .await;
    let code = code_from(watch.body["redirect"].as_str().unwrap());
    let limited = token_request(
        &app,
        &format!(
            "grant_type=authorization_code&code={code}&redirect_uri={}&client_id={client_id}&code_verifier={verifier}",
            percent("http://127.0.0.1:9/callback")
        ),
    )
    .await;
    let limited_access = limited.body["access_token"].as_str().unwrap();
    assert!(!limited.body["scope"].as_str().unwrap().contains(":write"));
    let denied = rpc(
        &app,
        limited_access,
        json!({
            "jsonrpc": "2.0",
            "id": 5,
            "method": "tools/call",
            "params": {
                "name": "post_v1_projects",
                "arguments": {"body": {"name": "nope"}}
            }
        }),
    )
    .await;
    let text = denied.body["result"]["content"][0]["text"]
        .as_str()
        .unwrap();
    assert_eq!(denied.body["result"]["isError"], true, "{text}");

    let missing = rpc(
        &app,
        "",
        json!({"jsonrpc": "2.0", "id": 6, "method": "initialize", "params": {}}),
    )
    .await;
    assert_eq!(missing.status, StatusCode::UNAUTHORIZED);
    assert!(missing
        .headers
        .get("www-authenticate")
        .unwrap()
        .to_str()
        .unwrap()
        .contains("oauth-protected-resource"));
}

fn percent(value: &str) -> String {
    percent_encoding::utf8_percent_encode(value, percent_encoding::NON_ALPHANUMERIC).to_string()
}

async fn client_request(
    app: &axum::Router,
    path: &str,
    body: Value,
    basic: Option<String>,
) -> Reply {
    let (content_type, payload) = if body["grant_type"] == "refresh_token" {
        let fields: Vec<(&str, &str)> = body
            .as_object()
            .unwrap()
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str().unwrap()))
            .collect();
        (
            "application/x-www-form-urlencoded",
            serde_urlencoded::to_string(fields).unwrap(),
        )
    } else {
        ("application/json", body.to_string())
    };
    let mut request = Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", content_type);
    if let Some(basic) = basic {
        request = request.header("authorization", format!("Basic {basic}"));
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::from(payload)).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    Reply {
        status,
        headers,
        body: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn confidential_clients_authenticate_for_codes_refresh_and_revocation(pool: PgPool) {
    let (cookie, csrf) = owner(&app(pool.clone())).await;
    for (method, transport) in [
        ("client_secret_post", "client_secret_post"),
        ("client_secret_post", "client_secret_basic"),
        ("client_secret_basic", "client_secret_post"),
        ("client_secret_basic", "client_secret_basic"),
    ] {
        let app = app(pool.clone());
        let registration = client_request(
            &app,
            "/oauth/register",
            json!({
                "redirect_uris": ["https://client.example/callback"],
                "token_endpoint_auth_method": method,
                "extra_metadata": true
            }),
            None,
        )
        .await;
        assert_eq!(
            registration.status,
            StatusCode::CREATED,
            "{}",
            registration.body
        );
        assert_eq!(registration.body["client_name"], "MCP client");
        assert_eq!(registration.body["token_endpoint_auth_method"], method);
        assert_eq!(registration.body["client_secret_expires_at"], 0);
        assert!(registration.body["client_id_issued_at"].as_i64().unwrap() > 0);
        let id = registration.body["client_id"].as_str().unwrap();
        let secret = registration.body["client_secret"].as_str().unwrap();
        let stored: String =
            sqlx::query_scalar("SELECT client_secret_hash FROM oauth_clients WHERE client_id = $1")
                .bind(id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(stored, hex::encode(Sha256::digest(secret.as_bytes())));
        let verifier = "a".repeat(64);
        let start = format!("/oauth/authorize?response_type=code&client_id={id}&redirect_uri={}&code_challenge={}&code_challenge_method=S256&state=test", percent("https://client.example/callback"), s256(&verifier));
        let authorized = call(&app, "GET", &start, Some(&cookie), None, None, None, None).await;
        assert_eq!(authorized.status, StatusCode::TEMPORARY_REDIRECT);
        let consent = call(&app, "POST", "/api/v1/mcp/consent", Some(&cookie), Some(&csrf), None, Some("application/json"), Some(json!({
            "client_id": id, "redirect_uri": "https://client.example/callback",
            "code_challenge": s256(&verifier), "code_challenge_method": "S256", "template": "full", "state": "test"
        }).to_string().into_bytes())).await;
        assert_eq!(consent.status, StatusCode::OK, "{}", consent.body);
        let code = code_from(consent.body["redirect"].as_str().unwrap());
        let mut exchange = json!({"grant_type": "authorization_code", "client_id": id, "code": code, "redirect_uri": "https://client.example/callback", "code_verifier": verifier});
        for secret in [None, Some("wrong")] {
            let mut bad = exchange.clone();
            if let Some(secret) = secret {
                bad["client_secret"] = json!(secret);
            }
            let denied = client_request(&app, "/oauth/token", bad, None).await;
            assert_eq!(denied.status, StatusCode::UNAUTHORIZED);
            assert_eq!(denied.body["error"], "invalid_client");
        }
        let basic = if transport == "client_secret_basic" {
            exchange.as_object_mut().unwrap().remove("client_id");
            // Encode every byte to exercise RFC 6749 credential decoding.
            Some(base64::engine::general_purpose::STANDARD.encode(format!(
                "{}:{}",
                percent(id),
                percent(secret)
            )))
        } else {
            exchange["client_secret"] = json!(secret);
            None
        };
        if basic.is_some() {
            for bad in [
                "not-base64".to_owned(),
                base64::engine::general_purpose::STANDARD.encode(format!("{id}:wrong")),
            ] {
                let denied =
                    client_request(&app, "/oauth/token", exchange.clone(), Some(bad)).await;
                assert_eq!(denied.status, StatusCode::UNAUTHORIZED);
                assert_eq!(denied.body["error"], "invalid_client");
            }
        }
        let mut bad_pkce = exchange.clone();
        bad_pkce["code_verifier"] = json!("b".repeat(64));
        assert_eq!(
            client_request(&app, "/oauth/token", bad_pkce, basic.clone())
                .await
                .status,
            StatusCode::BAD_REQUEST
        );
        let issued = client_request(&app, "/oauth/token", exchange.clone(), basic.clone()).await;
        assert_eq!(issued.status, StatusCode::OK, "{}", issued.body);
        let mut refresh = json!({"grant_type": "refresh_token", "refresh_token": issued.body["refresh_token"], "client_id": id});
        let denied = client_request(&app, "/oauth/token", refresh.clone(), None).await;
        assert_eq!(denied.status, StatusCode::UNAUTHORIZED);
        if transport == "client_secret_post" {
            refresh["client_secret"] = json!(secret);
        }
        let refreshed = client_request(&app, "/oauth/token", refresh, basic.clone()).await;
        assert_eq!(refreshed.status, StatusCode::OK, "{}", refreshed.body);
        let mut revoke = json!({"token": refreshed.body["refresh_token"], "client_id": id});
        let denied = client_request(&app, "/oauth/revoke", revoke.clone(), None).await;
        assert_eq!(denied.status, StatusCode::UNAUTHORIZED);
        if transport == "client_secret_post" {
            revoke["client_secret"] = json!(secret);
        }
        let revoked = client_request(&app, "/oauth/revoke", revoke, basic).await;
        assert_eq!(revoked.status, StatusCode::OK);
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn path_specific_discovery_and_registration_defaults(pool: PgPool) {
    let app = app(pool);
    for path in [
        "/.well-known/oauth-protected-resource",
        "/.well-known/oauth-authorization-server",
    ] {
        let root = call(&app, "GET", path, None, None, None, None, None).await;
        let inserted = call(
            &app,
            "GET",
            &format!("{path}/mcp"),
            None,
            None,
            None,
            None,
            None,
        )
        .await;
        assert_eq!(inserted.status, StatusCode::OK);
        assert_eq!(root.body, inserted.body);
        if path.ends_with("authorization-server") {
            assert_eq!(
                root.body["token_endpoint_auth_methods_supported"],
                json!(["none", "client_secret_post", "client_secret_basic"])
            );
            assert_eq!(
                root.body["revocation_endpoint_auth_methods_supported"],
                root.body["token_endpoint_auth_methods_supported"]
            );
        }
    }
    let public = client_request(
        &app,
        "/oauth/register",
        json!({"redirect_uris": ["https://client.example/callback"]}),
        None,
    )
    .await;
    assert_eq!(public.status, StatusCode::CREATED);
    assert_eq!(public.body["token_endpoint_auth_method"], "none");
    assert!(public.body.get("client_secret").is_none());
    let id = public.body["client_id"].as_str().unwrap();
    for basic in [false, true] {
        let mut request = json!({"grant_type": "authorization_code", "client_id": id});
        let credentials = if basic {
            Some(base64::engine::general_purpose::STANDARD.encode(format!("{id}:secret")))
        } else {
            request["client_secret"] = json!("secret");
            None
        };
        let denied = client_request(&app, "/oauth/token", request, credentials).await;
        assert_eq!(denied.status, StatusCode::UNAUTHORIZED);
        assert_eq!(denied.body["error"], "invalid_client");
    }
    for body in [
        json!({"redirect_uris": []}),
        json!({"redirect_uris": ["https://client.example"], "client_name": ""}),
    ] {
        assert_eq!(
            client_request(&app, "/oauth/register", body, None)
                .await
                .status,
            StatusCode::BAD_REQUEST
        );
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn registration_accepts_optional_nulls_and_replaces_unsupported_metadata(pool: PgPool) {
    let app = app(pool);
    for content_type in [
        None,
        Some("text/plain"),
        Some("application/json; charset=utf-8"),
    ] {
        for method in [
            Value::Null,
            json!("private_key_jwt"),
            json!("tls_client_auth"),
        ] {
            let registered = call(
                &app,
                "POST",
                "/oauth/register",
                None,
                None,
                None,
                content_type,
                Some(
                    json!({
                        "client_name": null,
                        "redirect_uris": ["https://client.example/callback"],
                        "grant_types": null,
                        "response_types": null,
                        "token_endpoint_auth_method": method,
                        "scope": null,
                        "contacts": null,
                        "unknown": null
                    })
                    .to_string()
                    .into_bytes(),
                ),
            )
            .await;
            assert_eq!(
                registered.status,
                StatusCode::CREATED,
                "{}",
                registered.body
            );
            assert_eq!(registered.body["client_name"], "MCP client");
            assert_eq!(
                registered.body["grant_types"],
                json!(["authorization_code", "refresh_token"])
            );
            assert_eq!(registered.body["response_types"], json!(["code"]));
            let expected = if method.is_null() {
                "none"
            } else {
                "client_secret_post"
            };
            assert_eq!(registered.body["token_endpoint_auth_method"], expected);
            assert_eq!(
                registered.body.get("client_secret").is_some(),
                !method.is_null()
            );
        }
    }
    for grants in [
        json!(["client_credentials", "implicit"]),
        json!(["authorization_code", "client_credentials"]),
        json!([]),
    ] {
        let registered = client_request(
            &app,
            "/oauth/register",
            json!({
                "redirect_uris": ["https://client.example/callback"],
                "grant_types": grants,
                "response_types": ["token", "id_token"],
                "scope": "unsupported:scope",
                "unknown": {"extra": true}
            }),
            None,
        )
        .await;
        assert_eq!(
            registered.status,
            StatusCode::CREATED,
            "{}",
            registered.body
        );
        assert_eq!(
            registered.body["grant_types"],
            json!(["authorization_code", "refresh_token"])
        );
        assert_eq!(registered.body["response_types"], json!(["code"]));
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn registration_rejections_use_rfc7591_errors(pool: PgPool) {
    let app = app(pool);
    for (body, error) in [
        (
            json!({"redirect_uris": ["http://evil.example/callback"]}).to_string(),
            "invalid_redirect_uri",
        ),
        (
            json!({"redirect_uris": []}).to_string(),
            "invalid_redirect_uri",
        ),
        ("{".to_owned(), "invalid_client_metadata"),
        ("".to_owned(), "invalid_client_metadata"),
        ("null".to_owned(), "invalid_client_metadata"),
        (
            json!({"redirect_uris": ["https://client.example"], "client_name": 42}).to_string(),
            "invalid_client_metadata",
        ),
    ] {
        let rejected = call(
            &app,
            "POST",
            "/oauth/register",
            None,
            None,
            None,
            None,
            Some(body.into_bytes()),
        )
        .await;
        assert_eq!(
            rejected.status,
            StatusCode::BAD_REQUEST,
            "{}",
            rejected.body
        );
        assert_eq!(rejected.body["error"], error);
        assert!(rejected.body["error_description"]
            .as_str()
            .is_some_and(|description| !description.is_empty()));
        assert_eq!(rejected.body.as_object().unwrap().len(), 2);
    }
}

async fn token_request(app: &axum::Router, form: &str) -> Reply {
    call(
        app,
        "POST",
        "/oauth/token",
        None,
        None,
        None,
        Some("application/x-www-form-urlencoded"),
        Some(form.as_bytes().to_vec()),
    )
    .await
}

#[sqlx::test(migrations = "./migrations")]
async fn long_authorization_state_survives_consent_and_token_exchange(pool: PgPool) {
    let app = app(pool);
    let redirect_uri = "https://api.devin.ai/mcp/oauth/callback";
    let resource = "https://rudder.dymini.com/mcp";
    let registered = client_request(
        &app,
        "/oauth/register",
        json!({
            "client_name": "Devin", "redirect_uris": [redirect_uri]
        }),
        None,
    )
    .await;
    assert_eq!(registered.status, StatusCode::CREATED);
    let id = registered.body["client_id"].as_str().unwrap();
    let scopes: Vec<&str> = rudder_core::MCP_SCOPES
        .iter()
        .map(|scope| scope.id)
        .collect();
    let scope = scopes.join(" ");
    let verifier = "v".repeat(64);
    let challenge = s256(&verifier);
    let long_state = base64::engine::general_purpose::STANDARD
        .encode(json!({"context": "x".repeat(740)}).to_string());
    assert!(long_state.len() >= 1000);
    let authorize_url = |state: &str| {
        format!(
        "/oauth/authorize?response_type=code&client_id={id}&redirect_uri={}&code_challenge={challenge}&code_challenge_method=S256&state={}&scope={}&resource={}",
        percent(redirect_uri), percent(state), percent(&scope), percent(resource)
    )
    };
    let consent_body = |state: &str| {
        json!({
            "client_id": id, "redirect_uri": redirect_uri, "resource": resource,
            "code_challenge": challenge, "code_challenge_method": "S256",
            "state": state, "template": "custom", "scopes": scopes
        })
    };
    let anonymous = call(
        &app,
        "GET",
        &authorize_url(&long_state),
        None,
        None,
        None,
        None,
        None,
    )
    .await;
    assert_eq!(anonymous.status, StatusCode::TEMPORARY_REDIRECT);
    let login = url::Url::parse(&format!(
        "https://panel.example{}",
        anonymous.headers["location"].to_str().unwrap()
    ))
    .unwrap();
    let next = login
        .query_pairs()
        .find(|(key, _)| key == "next")
        .unwrap()
        .1
        .into_owned();
    assert_eq!(next, authorize_url(&long_state));
    let (cookie, csrf) = owner(&app).await;
    for state in [long_state, "s".repeat(4096)] {
        let authorized = call(
            &app,
            "GET",
            &authorize_url(&state),
            Some(&cookie),
            None,
            None,
            None,
            None,
        )
        .await;
        assert_eq!(authorized.status, StatusCode::TEMPORARY_REDIRECT);
        let consent = url::Url::parse(&format!(
            "https://panel.example{}",
            authorized.headers["location"].to_str().unwrap()
        ))
        .unwrap();
        assert_eq!(consent.path(), "/mcp/consent");
        let params: HashMap<_, _> = consent.query_pairs().into_owned().collect();
        assert_eq!(params["state"], state);
        assert_eq!(params["scope"], scope);
        let approved = call(
            &app,
            "POST",
            "/api/v1/mcp/consent",
            Some(&cookie),
            Some(&csrf),
            None,
            Some("application/json"),
            Some(consent_body(&params["state"]).to_string().into_bytes()),
        )
        .await;
        assert_eq!(approved.status, StatusCode::OK, "{}", approved.body);
        let redirect = approved.body["redirect"].as_str().unwrap();
        let callback = url::Url::parse(redirect).unwrap();
        assert_eq!(
            callback
                .query_pairs()
                .find(|(key, _)| key == "state")
                .unwrap()
                .1,
            state
        );
        let issued = token_request(&app, &format!(
            "grant_type=authorization_code&client_id={id}&code={}&redirect_uri={}&code_verifier={verifier}",
            code_from(redirect), percent(redirect_uri)
        )).await;
        assert_eq!(issued.status, StatusCode::OK, "{}", issued.body);
        let mut granted = scopes.clone();
        granted.sort();
        assert_eq!(issued.body["scope"], granted.join(" "));
    }
    for (state, message) in [
        (String::new(), "state is required"),
        ("s".repeat(4097), "state is too long"),
    ] {
        let rejected = call(
            &app,
            "GET",
            &authorize_url(&state),
            Some(&cookie),
            None,
            None,
            None,
            None,
        )
        .await;
        assert_eq!(rejected.status, StatusCode::BAD_REQUEST);
        assert_eq!(rejected.body["error"]["message"], message);
        let rejected = call(
            &app,
            "POST",
            "/api/v1/mcp/consent",
            Some(&cookie),
            Some(&csrf),
            None,
            Some("application/json"),
            Some(consent_body(&state).to_string().into_bytes()),
        )
        .await;
        assert_eq!(rejected.status, StatusCode::BAD_REQUEST);
        assert_eq!(rejected.body["error"]["message"], message);
    }
}

async fn rpc(app: &axum::Router, bearer: &str, body: Value) -> Reply {
    call(
        app,
        "POST",
        "/mcp",
        None,
        None,
        if bearer.is_empty() {
            None
        } else {
            Some(bearer)
        },
        Some("application/json"),
        Some(body.to_string().into_bytes()),
    )
    .await
}
