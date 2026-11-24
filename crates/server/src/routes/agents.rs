//! Agent enrollment, join tokens, and the agent installer.

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Json;
use bytes::Bytes;
use chrono::{DateTime, Duration, Utc};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::agent::{self, pki, EnrollError, AGENT_PORT};
use crate::audit;
use crate::auth::{self, AuthUser};
use crate::error::ApiError;
use crate::routes::catalog::{bad, conflict, not_found};
use crate::state::AppState;

const JOIN_TOKEN_HOURS: i64 = 1;
const MAX_CSR_BYTES: usize = 8 * 1024;
const INSTALL_SCRIPT: &str = include_str!("../../../../deploy/install-agent.sh");

#[derive(Deserialize, ToSchema)]
pub struct EnrollRequest {
    pub join_token: String,
    /// PKCS#10 request for the agent's own key, PEM.
    pub csr_pem: String,
    #[serde(default)]
    pub hostname: String,
}

#[derive(Serialize, ToSchema)]
pub struct EnrollResponse {
    pub server_id: Uuid,
    pub client_cert_pem: String,
    pub ca_pem: String,
    /// Where the agent dials its gRPC session.
    pub grpc_url: String,
    /// TLS server name to verify on that connection.
    pub tls_name: String,
}

#[derive(Serialize, ToSchema)]
pub struct JoinToken {
    pub join_token: String,
    pub join_command: String,
    pub expires_at: DateTime<Utc>,
}

pub fn router() -> axum::Router<AppState> {
    axum::Router::new()
        .route("/api/v1/agent/enroll", post(enroll))
        .route("/api/v1/agent/tunnel", get(tunnel))
        .route("/api/v1/servers/{id}/join-token", post(rotate_join_token))
        .route("/install-agent.sh", get(install_script))
        .route("/downloads/{file}", get(download))
}

/// Base URL of the panel as the browser reached it, for commands shown to users.
pub fn panel_url(state: &AppState, headers: &HeaderMap) -> String {
    if let Some(url) = &state.config.public_url {
        return url.trim_end_matches('/').to_owned();
    }
    let host = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .filter(|h| valid_host(h))
        .unwrap_or("localhost:3000");
    let scheme = if state.config.trust_proxy {
        headers
            .get("x-forwarded-proto")
            .and_then(|v| v.to_str().ok())
            .filter(|p| *p == "https" || *p == "http")
            .unwrap_or("http")
    } else {
        "http"
    };
    format!("{scheme}://{host}")
}

fn valid_host(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 260
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b':' | b'[' | b']'))
}

/// The one-line installer shown after adding a server. The token is the only secret.
/// It pins the agent CA's fingerprint, so enrollment cannot be answered by anyone
/// else even if the connection to the panel is intercepted.
pub async fn join_command(
    state: &AppState,
    headers: &HeaderMap,
    token: &str,
) -> Result<String, ApiError> {
    let panel = panel_url(state, headers);
    let ca = pki::AgentCa::load_or_create(&state.pool, &state.cipher)
        .await
        .map_err(ca_unavailable)?;
    let ca_sha256 = pki::pem_fingerprint(&ca.cert_pem)
        .ok_or_else(|| ca_unavailable("the agent CA certificate is unreadable"))?;
    // /mcp is the machine-client path prefix: it stays reachable through
    // Cloudflare Access's agent bypass when the panel is Zero Trust locked.
    Ok(format!(
        "curl -fsSL {panel}/mcp/install-agent.sh | sudo sh -s -- --server {panel} --ca-sha256 {ca_sha256} --token {token}"
    ))
}

fn ca_unavailable(error: impl std::fmt::Display) -> ApiError {
    tracing::error!(%error, "agent CA unavailable");
    ApiError::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        "ca_unavailable",
        "the agent certificate authority is unavailable",
    )
}

fn grpc_url(state: &AppState, headers: &HeaderMap) -> String {
    if let Some(url) = &state.config.agent_url {
        return url.clone();
    }
    let panel = panel_url(state, headers);
    let authority = panel
        .split_once("://")
        .map_or(panel.as_str(), |(_, rest)| rest)
        .split('/')
        .next()
        .unwrap_or_default();
    let host = authority_host(authority);
    let port = state
        .config
        .agent_bind
        .map_or(AGENT_PORT, |bind| bind.port());
    format!("https://{host}:{port}")
}

/// Host part of `host`, `host:port`, `[v6]`, or `[v6]:port`, brackets kept.
fn authority_host(authority: &str) -> &str {
    if authority.starts_with('[') {
        return authority
            .find(']')
            .map_or(authority, |end| &authority[..=end]);
    }
    authority
        .split_once(':')
        .map_or(authority, |(host, _)| host)
}

async fn enroll(
    State(state): State<AppState>,
    auth::OptionalPeer(peer): auth::OptionalPeer,
    headers: HeaderMap,
    Json(body): Json<EnrollRequest>,
) -> Result<Json<EnrollResponse>, ApiError> {
    let ip = auth::client_ip(&headers, peer, &state.config.trusted_proxies);
    if state
        .login_limiter
        .check_key(&format!("enroll:{ip}"))
        .is_err()
    {
        return Err(ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limited",
            "too many enrollment attempts",
        ));
    }
    if body.csr_pem.len() > MAX_CSR_BYTES || body.join_token.len() > 200 {
        return Err(bad("invalid_request", "request is too large"));
    }
    let enrolled = agent::enroll(&state, &body.join_token, &body.csr_pem, &body.hostname)
        .await
        .map_err(|error| match error {
            EnrollError::BadToken => ApiError::new(
                StatusCode::UNAUTHORIZED,
                "invalid_join_token",
                error.to_string(),
            ),
            EnrollError::BadCsr => bad("invalid_csr", error.to_string()),
            EnrollError::Database(error) => ApiError::database(error),
            EnrollError::Pki(error) => ca_unavailable(error),
        })?;
    audit::record(
        &state.pool,
        None,
        "server.enroll",
        "server",
        Some(&enrolled.server_id.to_string()),
        json!({ "hostname": body.hostname.chars().take(253).collect::<String>() }),
        Some(&ip),
    )
    .await;
    Ok(Json(EnrollResponse {
        server_id: enrolled.server_id,
        client_cert_pem: enrolled.client_cert_pem,
        ca_pem: enrolled.ca_pem,
        grpc_url: grpc_url(&state, &headers),
        tls_name: pki::PANEL_TLS_NAME.to_owned(),
    }))
}

async fn rotate_join_token(
    State(state): State<AppState>,
    auth: AuthUser,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<JoinToken>, ApiError> {
    auth.require_write()?;
    let kind = sqlx::query_scalar!("SELECT kind FROM servers WHERE id = $1", id)
        .fetch_optional(&state.pool)
        .await
        .map_err(ApiError::database)?
        .ok_or_else(|| not_found("server not found"))?;
    if kind != "remote" {
        return Err(conflict(
            "local_server",
            "the local server does not use an agent",
        ));
    }
    let token = auth::random_token("join");
    let expires_at = Utc::now() + Duration::hours(JOIN_TOKEN_HOURS);
    sqlx::query!(
        "UPDATE servers SET join_token_hash = $2, join_token_expires_at = $3, updated_at = now() WHERE id = $1",
        id,
        auth::sha256_hex(&token),
        expires_at
    )
    .execute(&state.pool)
    .await
    .map_err(ApiError::database)?;
    audit::record(
        &state.pool,
        Some(auth.id),
        "server.join_token",
        "server",
        Some(&id.to_string()),
        json!({}),
        None,
    )
    .await;
    Ok(Json(JoinToken {
        join_command: join_command(&state, &headers, &token).await?,
        join_token: token,
        expires_at,
    }))
}

/// A byte pipe to the agent gRPC listener over a WebSocket upgrade. The
/// client still performs the mTLS handshake inside the socket, so enrollment
/// and sessions work when only the panel's HTTPS port is reachable — for
/// example behind a Cloudflare Tunnel with no inbound agent port. Upgrading
/// needs no panel login because the inner mTLS is the credential, just like a
/// direct connection to the listener.
async fn tunnel(
    State(state): State<AppState>,
    auth::OptionalPeer(peer): auth::OptionalPeer,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    let Some(bind) = state.config.agent_bind else {
        return ApiError::new(
            StatusCode::NOT_FOUND,
            "agent_listener_off",
            "the agent listener is disabled",
        )
        .into_response();
    };
    if !crate::security::ws_same_origin(&headers) {
        return ApiError::new(
            StatusCode::FORBIDDEN,
            "bad_origin",
            "cross-origin WebSocket requests are not allowed",
        )
        .into_response();
    }
    let ip = auth::client_ip(&headers, peer, &state.config.trusted_proxies);
    if state
        .login_limiter
        .check_key(&format!("agent-tunnel:{ip}"))
        .is_err()
    {
        return ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limited",
            "too many connection attempts",
        )
        .into_response();
    }
    upgrade.on_upgrade(move |socket| pipe_to_listener(socket, bind))
}

/// An unspecified bind address means the listener sits on this machine's
/// loopback as well, which is the only side this pipe may reach.
fn listener_target(bind: SocketAddr) -> SocketAddr {
    if !bind.ip().is_unspecified() {
        return bind;
    }
    let localhost = if bind.is_ipv4() {
        IpAddr::V4(Ipv4Addr::LOCALHOST)
    } else {
        IpAddr::V6(Ipv6Addr::LOCALHOST)
    };
    SocketAddr::new(localhost, bind.port())
}

/// Copy bytes between the WebSocket and the agent listener until either side
/// closes. Binary messages carry the opaque inner TLS stream.
async fn pipe_to_listener(socket: WebSocket, bind: SocketAddr) {
    let target = listener_target(bind);
    let tcp = match TcpStream::connect(target).await {
        Ok(tcp) => tcp,
        Err(error) => {
            tracing::warn!(%target, %error, "agent tunnel could not reach the listener");
            return;
        }
    };
    let _ = tcp.set_nodelay(true);
    let (mut tcp_read, mut tcp_write) = tcp.into_split();
    let (mut sink, mut incoming) = socket.split();
    let mut buf = [0u8; 16 * 1024];
    loop {
        tokio::select! {
            message = incoming.next() => match message {
                Some(Ok(Message::Binary(bytes))) => {
                    if tcp_write.write_all(&bytes).await.is_err() {
                        break;
                    }
                }
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                Some(Ok(_)) => {}
            },
            read = tcp_read.read(&mut buf) => match read {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if sink
                        .send(Message::Binary(Bytes::copy_from_slice(&buf[..n])))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            },
        }
    }
}

async fn install_script() -> Response {
    (
        [(header::CONTENT_TYPE, "text/x-shellscript; charset=utf-8")],
        INSTALL_SCRIPT,
    )
        .into_response()
}

/// Agent binaries bundled with the panel image, so servers can install without
/// reaching GitHub.
async fn download(State(state): State<AppState>, Path(file): Path<String>) -> Response {
    let allowed = [
        "rudder-agent-x86_64-unknown-linux-musl",
        "rudder-agent-aarch64-unknown-linux-musl",
    ];
    let name = file.strip_suffix(".sha256").unwrap_or(&file);
    if !allowed.contains(&name) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let path = state.config.dist_dir.join(&file);
    match tokio::fs::File::open(&path).await {
        Ok(handle) => {
            let length = handle.metadata().await.map(|m| m.len()).unwrap_or(0);
            let body = axum::body::Body::from_stream(tokio_util::io::ReaderStream::new(handle));
            (
                [
                    (header::CONTENT_TYPE, "application/octet-stream".to_owned()),
                    (header::CONTENT_LENGTH, length.to_string()),
                ],
                body,
            )
                .into_response()
        }
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::{authority_host, valid_host};

    #[test]
    fn authority_host_drops_ports() {
        assert_eq!(
            authority_host("panel.example.com:3000"),
            "panel.example.com"
        );
        assert_eq!(authority_host("panel.example.com"), "panel.example.com");
        assert_eq!(authority_host("[2001:db8::1]:3000"), "[2001:db8::1]");
        assert_eq!(authority_host("[2001:db8::1]"), "[2001:db8::1]");
    }

    #[test]
    fn host_headers_are_restricted_to_hostname_characters() {
        assert!(valid_host("panel.example.com:3000"));
        assert!(valid_host("[2001:db8::1]:3000"));
        assert!(!valid_host("evil.com/$(id)"));
        assert!(!valid_host("a b"));
        assert!(!valid_host(""));
    }
}
