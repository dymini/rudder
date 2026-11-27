use std::{collections::HashMap, num::NonZeroU32};

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use dymini_panel_server::AppState;
use governor::{Quota, RateLimiter};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sqlx::PgPool;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
    sync::mpsc,
};
use tower::ServiceExt;
use uuid::Uuid;
use webauthn_authenticator_rs::{softpasskey::SoftPasskey, WebauthnAuthenticator};
use webauthn_rs::prelude::*;

const PASSWORD: &str = "correct-horse-battery";
const EMAIL: &str = "owner@example.test";
const ORIGIN: &str = "https://panel.example.test";

struct Fixture {
    app: axum::Router,
    state: AppState,
    cookie: String,
    csrf: String,
    id: Uuid,
}
struct Reply {
    status: StatusCode,
    body: Value,
    cookie: Option<String>,
}

fn config() -> dymini_panel_server::Config {
    dymini_panel_server::Config::from_map(
        &HashMap::from([
            (
                "DATABASE_URL".into(),
                std::env::var("DATABASE_URL").unwrap(),
            ),
            ("DYMINI_PANEL_MASTER_KEY".into(), "bb".repeat(32)),
            ("DYMINI_PANEL_SETUP_TOKEN".into(), "mfa-test-setup".into()),
            ("DYMINI_PANEL_COOKIE_SECURE".into(), "false".into()),
            ("DYMINI_PANEL_PUBLIC_URL".into(), ORIGIN.into()),
        ]),
        |_| unreachable!(),
    )
    .unwrap()
}
async fn request(
    app: &axum::Router,
    method: &str,
    path: &str,
    cookie: Option<(&str, &str)>,
    body: Option<Value>,
) -> Reply {
    let mut builder = Request::builder().method(method).uri(path);
    if let Some((cookie, csrf)) = cookie {
        builder = builder
            .header("cookie", cookie)
            .header("x-csrf-token", csrf);
    }
    let body = if let Some(body) = body {
        builder = builder.header("content-type", "application/json");
        Body::from(body.to_string())
    } else {
        Body::empty()
    };
    let response = app
        .clone()
        .oneshot(builder.body(body).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let cookie = response
        .headers()
        .get("set-cookie")
        .map(|c| c.to_str().unwrap().split(';').next().unwrap().to_owned());
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    Reply {
        status,
        body: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        cookie,
    }
}
impl Fixture {
    async fn call(&self, method: &str, path: &str, body: Option<Value>) -> Reply {
        request(
            &self.app,
            method,
            path,
            Some((&self.cookie, &self.csrf)),
            body,
        )
        .await
    }
    async fn public(&self, path: &str, body: Option<Value>) -> Reply {
        request(&self.app, "POST", path, None, body).await
    }
}
async fn fixture(pool: &PgPool) -> Fixture {
    let mut state = AppState::new(pool.clone(), config());
    state.login_limiter = std::sync::Arc::new(RateLimiter::keyed(Quota::per_minute(
        NonZeroU32::new(1000).unwrap(),
    )));
    let app = dymini_panel_server::router(state.clone());
    let reply = request(
        &app,
        "POST",
        "/api/v1/setup",
        None,
        Some(json!({"setup_token":"mfa-test-setup","email":EMAIL,"password":PASSWORD})),
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    Fixture {
        app,
        state,
        cookie: reply.cookie.unwrap(),
        csrf: reply.body["csrf_token"].as_str().unwrap().into(),
        id: reply.body["user"]["id"].as_str().unwrap().parse().unwrap(),
    }
}
async fn register(
    f: &Fixture,
    client: &mut WebauthnAuthenticator<SoftPasskey>,
    proof: Option<&str>,
) -> Value {
    let start = f
        .call(
            "POST",
            "/api/v1/me/passkeys/register/start",
            Some(json!({"password":PASSWORD,"name":"My device","mfa_proof":proof})),
        )
        .await;
    assert_eq!(start.status, StatusCode::OK, "{}", start.body);
    assert_eq!(
        start.body["options"]["publicKey"]["authenticatorSelection"]["residentKey"],
        "required"
    );
    let mut options: CreationChallengeResponse =
        serde_json::from_value(start.body["options"].clone()).unwrap();
    // SoftPasskey lacks resident storage; emulate the browser's resident-key selection.
    options
        .public_key
        .authenticator_selection
        .as_mut()
        .unwrap()
        .require_resident_key = false;
    let credential = client
        .do_registration(Url::parse(ORIGIN).unwrap(), options)
        .unwrap();
    let finish = json!({"challenge_id":start.body["challenge_id"],"credential":credential});
    let result = f
        .call(
            "POST",
            "/api/v1/me/passkeys/register/finish",
            Some(finish.clone()),
        )
        .await;
    assert_eq!(result.status, StatusCode::NO_CONTENT, "{}", result.body);
    assert_eq!(
        f.call("POST", "/api/v1/me/passkeys/register/finish", Some(finish))
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    serde_json::to_value(credential).unwrap()
}
async fn assertion(f: &Fixture, client: &mut WebauthnAuthenticator<SoftPasskey>) -> Value {
    let start = f
        .public(
            "/api/v1/auth/passkeys/verify/start",
            Some(json!({"email":EMAIL,"password":PASSWORD})),
        )
        .await;
    assert_eq!(start.status, StatusCode::OK, "{}", start.body);
    let options: RequestChallengeResponse =
        serde_json::from_value(start.body["options"].clone()).unwrap();
    let credential = client
        .do_authentication(Url::parse(ORIGIN).unwrap(), options)
        .unwrap();
    json!({"challenge_id":start.body["challenge_id"],"credential":credential})
}
async fn proof(f: &Fixture, client: &mut WebauthnAuthenticator<SoftPasskey>) -> String {
    let body = assertion(f, client).await;
    let finish = f
        .public("/api/v1/auth/passkeys/verify/finish", Some(body.clone()))
        .await;
    assert_eq!(finish.status, StatusCode::OK, "{}", finish.body);
    assert_eq!(
        f.public("/api/v1/auth/passkeys/verify/finish", Some(body))
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    finish.body["mfa_proof"].as_str().unwrap().into()
}

#[sqlx::test(migrations = "./migrations")]
async fn registration_second_factor_management_and_replay(pool: PgPool) {
    let f = fixture(&pool).await;
    let missing = format!("/api/v1/me/passkeys/{}", Uuid::new_v4());
    assert_eq!(
        f.call("DELETE", &missing, Some(json!({"password":PASSWORD})))
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        f.call("GET", "/api/v1/me/passkeys", None).await.body,
        json!([])
    );
    let mut client = WebauthnAuthenticator::new(SoftPasskey::new(true));
    let mut second = WebauthnAuthenticator::new(SoftPasskey::new(true));
    register(&f, &mut client, None).await;
    let denied = f
        .public(
            "/api/v1/login",
            Some(json!({"email":EMAIL,"password":PASSWORD})),
        )
        .await;
    assert_eq!(denied.status, StatusCode::UNAUTHORIZED);
    assert_eq!(denied.body["error"]["code"], "mfa_required");
    assert_eq!(
        f.call("POST", "/api/v1/me/totp/setup", None).await.status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        f.call(
            "POST",
            "/api/v1/me/passkeys/register/start",
            Some(json!({"name":"unverified","password":PASSWORD}))
        )
        .await
        .status,
        StatusCode::UNAUTHORIZED
    );
    let token = proof(&f, &mut client).await;
    let login = json!({"email":EMAIL,"password":PASSWORD,"mfa_proof":token});
    assert_eq!(
        f.public("/api/v1/login", Some(login.clone())).await.status,
        StatusCode::OK
    );
    assert_eq!(
        f.public("/api/v1/login", Some(login)).await.status,
        StatusCode::UNAUTHORIZED
    );
    let token = proof(&f, &mut client).await;
    register(&f, &mut second, Some(&token)).await;
    let rows = f.call("GET", "/api/v1/me/passkeys", None).await.body;
    assert_eq!(rows.as_array().unwrap().len(), 2);
    assert!(rows[0]["counter"].as_i64().unwrap() > 0);
    assert!(rows[0]["last_used_at"].is_string());
    let id = rows[0]["id"].as_str().unwrap();
    let path = format!("/api/v1/me/passkeys/{id}");
    assert_eq!(
        f.call("PATCH", &path, Some(json!({"name":""})))
            .await
            .status,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        f.call("PATCH", &path, Some(json!({"name":"Laptop"})))
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        f.call("GET", "/api/v1/me/passkeys", None).await.body[0]["name"],
        "Laptop"
    );
    assert_eq!(
        f.call("DELETE", &path, Some(json!({"password":PASSWORD})))
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    let pending = assertion(&f, &mut client).await;
    let token = proof(&f, &mut second).await;
    assert_eq!(
        f.call(
            "DELETE",
            &path,
            Some(json!({"password":PASSWORD,"mfa_proof":token}))
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        f.public("/api/v1/auth/passkeys/verify/finish", Some(pending))
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    let id = rows[1]["id"].as_str().unwrap();
    let path = format!("/api/v1/me/passkeys/{id}");
    assert_eq!(
        f.call("DELETE", &path, Some(json!({"password":PASSWORD})))
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    let token = proof(&f, &mut second).await;
    assert_eq!(
        f.call(
            "DELETE",
            &path,
            Some(json!({"password":"wrong","mfa_proof":token}))
        )
        .await
        .status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        f.call(
            "DELETE",
            &path,
            Some(json!({"password":PASSWORD,"mfa_proof":token}))
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        f.public(
            "/api/v1/login",
            Some(json!({"email":EMAIL,"password":PASSWORD}))
        )
        .await
        .status,
        StatusCode::OK
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn discoverable_signin_expiry_origin_and_counter_protection(pool: PgPool) {
    let f = fixture(&pool).await;
    let mut client = WebauthnAuthenticator::new(SoftPasskey::new(true));
    let registered = register(&f, &mut client, None).await;
    let start = f.public("/api/v1/auth/passkeys/login/start", None).await;
    assert_eq!(start.status, StatusCode::OK);
    let mut options = start.body["options"].clone();
    assert!(options["publicKey"]["allowCredentials"]
        .as_array()
        .unwrap()
        .is_empty());
    // Emulate resident discovery, which the software authenticator does not implement.
    options["publicKey"]["allowCredentials"] =
        json!([{"type":"public-key","id":registered["rawId"]}]);
    let mut credential = client
        .do_authentication(
            Url::parse(ORIGIN).unwrap(),
            serde_json::from_value(options).unwrap(),
        )
        .unwrap();
    credential.response.user_handle = Some(f.id.as_bytes().to_vec().into());
    let body = json!({"challenge_id":start.body["challenge_id"],"credential":credential});
    let finish = f
        .public("/api/v1/auth/passkeys/login/finish", Some(body.clone()))
        .await;
    assert_eq!(finish.status, StatusCode::OK, "{}", finish.body);
    assert!(finish.cookie.is_some());
    assert_eq!(finish.body["user"]["id"], f.id.to_string());
    assert_eq!(
        f.public("/api/v1/auth/passkeys/login/finish", Some(body))
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    let expired = assertion(&f, &mut client).await;
    sqlx::query("UPDATE mfa_challenges SET expires_at=now()-interval '1 second'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        f.public("/api/v1/auth/passkeys/verify/finish", Some(expired))
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    let wrong_origin = assertion(&f, &mut client).await;
    let mut other_config = config();
    other_config.public_url = Some("https://different.example.test".into());
    let other = dymini_panel_server::router(AppState::new(pool.clone(), other_config));
    assert_eq!(
        request(
            &other,
            "POST",
            "/api/v1/auth/passkeys/verify/finish",
            None,
            Some(wrong_origin)
        )
        .await
        .status,
        StatusCode::UNAUTHORIZED
    );
    let cloned = assertion(&f, &mut client).await;
    sqlx::query("UPDATE user_passkeys SET counter=99999")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        f.public("/api/v1/auth/passkeys/verify/finish", Some(cloned))
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
}

async fn smtp() -> (u16, mpsc::UnboundedReceiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        while let Ok((socket, _)) = listener.accept().await {
            let tx = tx.clone();
            tokio::spawn(async move {
                let (read, mut write) = socket.into_split();
                let mut reader = BufReader::new(read);
                write.write_all(b"220 fake SMTP\r\n").await.unwrap();
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).await.unwrap() == 0 {
                        break;
                    }
                    if line.starts_with("DATA") {
                        write.write_all(b"354 send data\r\n").await.unwrap();
                        let mut message = String::new();
                        loop {
                            let mut line = String::new();
                            if reader.read_line(&mut line).await.unwrap() == 0 || line == ".\r\n" {
                                break;
                            }
                            message.push_str(&line);
                        }
                        let _ = tx.send(message);
                        write.write_all(b"250 queued\r\n").await.unwrap();
                    } else if line.starts_with("QUIT") {
                        let _ = write.write_all(b"221 bye\r\n").await;
                        break;
                    } else {
                        write.write_all(b"250 ok\r\n").await.unwrap();
                    }
                }
            });
        }
    });
    (port, rx)
}
async fn delivered(rx: &mut mpsc::UnboundedReceiver<String>) -> String {
    let mail = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
        .await
        .unwrap()
        .unwrap();
    let code = mail
        .split("code is ")
        .nth(1)
        .unwrap()
        .chars()
        .take(6)
        .collect::<String>();
    assert!(code.chars().all(|c| c.is_ascii_digit()));
    code
}
async fn allow_send(pool: &PgPool) {
    sqlx::query("UPDATE mfa_email_codes SET created_at=now()-interval '2 minutes'")
        .execute(pool)
        .await
        .unwrap();
}
async fn login_email(f: &Fixture, code: &str) -> StatusCode {
    f.public(
        "/api/v1/login",
        Some(json!({"email":EMAIL,"password":PASSWORD,"email_code":code})),
    )
    .await
    .status
}

#[sqlx::test(migrations = "./migrations")]
async fn email_delivery_enrollment_expiry_reuse_attempts_and_disable(pool: PgPool) {
    let f = fixture(&pool).await;
    assert_eq!(
        f.call(
            "POST",
            "/api/v1/me/mfa/email/disable",
            Some(json!({"password":PASSWORD}))
        )
        .await
        .status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        f.call("GET", "/api/v1/me/mfa", None).await.body["email_available"],
        false
    );
    assert_eq!(
        f.call(
            "POST",
            "/api/v1/me/mfa/email/send",
            Some(json!({"password":PASSWORD,"purpose":"enable"}))
        )
        .await
        .status,
        StatusCode::BAD_REQUEST
    );
    let (port, mut rx) = smtp().await;
    let encrypted=f.state.cipher.encrypt(&serde_json::to_vec(&json!({"kind":"smtp","host":"127.0.0.1","port":port,"security":"none","username":null,"password":null})).unwrap()).unwrap();
    sqlx::query("UPDATE access_settings SET email_provider='smtp',email_from='Dymini Panel <no-reply@example.test>',email_config_enc=$1").bind(encrypted).execute(&pool).await.unwrap();
    let send = json!({"password":PASSWORD,"purpose":"enable"});
    let sent = f
        .call("POST", "/api/v1/me/mfa/email/send", Some(send.clone()))
        .await;
    assert_eq!(sent.status, StatusCode::NO_CONTENT, "{}", sent.body);
    let code = delivered(&mut rx).await;
    assert_eq!(
        f.call("POST", "/api/v1/me/mfa/email/send", Some(send))
            .await
            .status,
        StatusCode::TOO_MANY_REQUESTS
    );
    assert_eq!(
        f.call(
            "POST",
            "/api/v1/me/mfa/email/enable",
            Some(json!({"password":"wrong","code":code}))
        )
        .await
        .status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        f.call(
            "POST",
            "/api/v1/me/mfa/email/enable",
            Some(json!({"password":PASSWORD,"code":code}))
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        f.call(
            "POST",
            "/api/v1/me/mfa/email/enable",
            Some(json!({"password":PASSWORD,"code":code}))
        )
        .await
        .status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        f.public(
            "/api/v1/login",
            Some(json!({"email":EMAIL,"password":PASSWORD}))
        )
        .await
        .status,
        StatusCode::UNAUTHORIZED
    );
    let body = json!({"email":EMAIL,"password":PASSWORD});
    allow_send(&pool).await;
    assert_eq!(
        f.public("/api/v1/auth/mfa/email/send", Some(body.clone()))
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    let code = delivered(&mut rx).await;
    assert_eq!(login_email(&f, &code).await, StatusCode::OK);
    assert_eq!(login_email(&f, &code).await, StatusCode::UNAUTHORIZED);
    allow_send(&pool).await;
    assert_eq!(
        f.public("/api/v1/auth/mfa/email/send", Some(body.clone()))
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    let code = delivered(&mut rx).await;
    sqlx::query(
        "UPDATE mfa_email_codes SET expires_at=now()-interval '1 second' WHERE purpose='login'",
    )
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(login_email(&f, &code).await, StatusCode::UNAUTHORIZED);
    allow_send(&pool).await;
    assert_eq!(
        f.public("/api/v1/auth/mfa/email/send", Some(body))
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    let code = delivered(&mut rx).await;
    for _ in 0..5 {
        assert_eq!(
            login_email(&f, "not-a-code").await,
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(login_email(&f, &code).await, StatusCode::UNAUTHORIZED);
    let attempts: i32 =
        sqlx::query_scalar("SELECT attempts FROM mfa_email_codes WHERE purpose='login'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(attempts, 5);
    assert_eq!(
        f.call(
            "POST",
            "/api/v1/me/mfa/email/disable",
            Some(json!({"password":PASSWORD}))
        )
        .await
        .status,
        StatusCode::UNAUTHORIZED
    );
    allow_send(&pool).await;
    assert_eq!(
        f.call(
            "POST",
            "/api/v1/me/mfa/email/send",
            Some(json!({"password":PASSWORD,"purpose":"reauth"}))
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    let code = delivered(&mut rx).await;
    assert_eq!(
        f.call(
            "POST",
            "/api/v1/me/mfa/email/disable",
            Some(json!({"password":PASSWORD,"email_code":code}))
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        f.call("GET", "/api/v1/me/mfa", None).await.body["email"],
        false
    );
    assert_eq!(
        f.public(
            "/api/v1/login",
            Some(json!({"email":EMAIL,"password":PASSWORD}))
        )
        .await
        .status,
        StatusCode::OK
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn challenge_requests_are_ip_rate_limited(pool: PgPool) {
    let state = AppState::new(pool, config());
    let app = dymini_panel_server::router(state);
    for _ in 0..10 {
        assert_eq!(
            request(
                &app,
                "POST",
                "/api/v1/auth/passkeys/login/start",
                None,
                None
            )
            .await
            .status,
            StatusCode::OK
        );
    }
    assert_eq!(
        request(
            &app,
            "POST",
            "/api/v1/auth/passkeys/login/start",
            None,
            None
        )
        .await
        .status,
        StatusCode::TOO_MANY_REQUESTS
    );
}
