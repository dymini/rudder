//! OAuth 2.1 endpoints and the MCP JSON-RPC endpoint.
//!
//! Agents discover the login server, send the person to the browser to approve
//! a template, then exchange the code for an access token and a refresh token.

use std::net::SocketAddr;

use axum::body::Body;
use axum::extract::{Query, Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::Json;
use base64::Engine;
use serde::Deserialize;
use serde_json::{json, Value};
use tower::ServiceExt;

use crate::audit;
use crate::auth::{self, AuthUser};
use crate::error::ApiError;
use crate::state::AppState;

use super::tokens::{self, ExchangeError};
use super::Tool;

const PROTOCOL: &str = "2025-03-26";
const MAX_BODY: usize = 1024 * 1024;
const MAX_TOOL_RESPONSE: usize = 256 * 1024;

pub fn router() -> axum::Router<AppState> {
    axum::Router::new()
        .route(
            "/.well-known/oauth-protected-resource",
            get(protected_resource),
        )
        .route(
            "/.well-known/oauth-authorization-server",
            get(authorization_server),
        )
        .route("/oauth/register", post(register))
        .route(
            "/.well-known/oauth-protected-resource/mcp",
            get(protected_resource),
        )
        .route(
            "/.well-known/oauth-authorization-server/mcp",
            get(authorization_server),
        )
        .route("/oauth/authorize", get(authorize))
        .route("/oauth/token", post(token))
        .route("/oauth/revoke", post(revoke))
        .route("/mcp", post(rpc).get(mcp_unused).delete(mcp_end))
}

fn no_store(response: Response) -> Response {
    let mut response = response;
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    response
}

/// The address agents should use. The panel domain saved under **Certificates**
/// wins, so changing it there also moves the sign-in address. Otherwise an
/// `https` public address wins. A plain `http` one (the install default before
/// a panel domain exists) gives way to the HTTPS address the request actually
/// came in on, because agents in the cloud reject `http` endpoints and cannot
/// reach the install's IP address.
async fn origin(state: &AppState, headers: &HeaderMap) -> Result<String, ApiError> {
    if let Some(url) = panel_origin(state).await {
        return Ok(url);
    }
    let configured = state
        .config
        .public_url
        .as_deref()
        .map(|url| url.trim_end_matches('/').to_owned());
    if let Some(url) = &configured {
        if url.starts_with("https://") {
            return Ok(url.clone());
        }
        return match request_origin(state, headers) {
            Ok(seen) if seen.starts_with("https://") => Ok(seen),
            _ => Ok(url.clone()),
        };
    }
    request_origin(state, headers)
}

async fn panel_origin(state: &AppState) -> Option<String> {
    let row: Option<(Option<String>, String)> =
        sqlx::query_as("SELECT panel_domain, panel_ssl_mode FROM panel_settings WHERE id = TRUE")
            .fetch_optional(&state.pool)
            .await
            .ok()?;
    let (domain, mode) = row?;
    let domain = domain.filter(|domain| !domain.is_empty())?;
    let scheme = if mode == "none" { "http" } else { "https" };
    Some(format!("{scheme}://{domain}"))
}

fn request_origin(state: &AppState, headers: &HeaderMap) -> Result<String, ApiError> {
    let host = headers
        .get(if state.config.trust_proxy {
            "x-forwarded-host"
        } else {
            "host"
        })
        .or_else(|| headers.get(header::HOST))
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .split(',')
        .next()
        .unwrap_or("")
        .trim();
    if host.is_empty()
        || host.contains('/')
        || host.contains(' ')
        || host.contains('@')
        || host.len() > 255
    {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_host",
            "set DYMINI_PANEL_PUBLIC_URL so agents know the panel address",
        ));
    }
    let proto = if state.config.trust_proxy {
        headers
            .get("x-forwarded-proto")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(',').next())
            .map(str::trim)
            .filter(|value| *value == "http" || *value == "https")
            .unwrap_or(if state.config.cookie_secure {
                "https"
            } else {
                "http"
            })
    } else if state.config.cookie_secure {
        "https"
    } else {
        "http"
    };
    Ok(format!("{proto}://{host}"))
}

fn scopes_supported() -> Vec<&'static str> {
    dymini_panel_core::MCP_SCOPES
        .iter()
        .map(|scope| scope.id)
        .collect()
}

async fn protected_resource(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let origin = origin(&state, &headers).await?;
    Ok(no_store(
        Json(json!({
            "resource": format!("{origin}/mcp"),
            "authorization_servers": [origin],
            "scopes_supported": scopes_supported(),
            "bearer_methods_supported": ["header"],
            "resource_name": "Dymini Panel",
        }))
        .into_response(),
    ))
}

async fn authorization_server(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let origin = origin(&state, &headers).await?;
    Ok(no_store(
        Json(json!({
            "issuer": origin,
            "authorization_endpoint": format!("{origin}/oauth/authorize"),
            "token_endpoint": format!("{origin}/oauth/token"),
            "registration_endpoint": format!("{origin}/oauth/register"),
            "revocation_endpoint": format!("{origin}/oauth/revoke"),
            "response_types_supported": ["code"],
            "grant_types_supported": ["authorization_code", "refresh_token"],
            "code_challenge_methods_supported": ["S256"],
            "token_endpoint_auth_methods_supported": ["none", "client_secret_post", "client_secret_basic"],
            "revocation_endpoint_auth_methods_supported": ["none", "client_secret_post", "client_secret_basic"],
            "scopes_supported": scopes_supported(),
        }))
        .into_response(),
    ))
}

#[derive(Deserialize)]
struct RegisterRequest {
    client_name: Option<String>,
    redirect_uris: Vec<String>,
    #[serde(default)]
    token_endpoint_auth_method: Option<String>,
}

async fn register(
    State(state): State<AppState>,
    headers: HeaderMap,
    auth::OptionalPeer(peer): auth::OptionalPeer,
    raw: Result<axum::body::Bytes, axum::extract::rejection::BytesRejection>,
) -> Response {
    let metadata = raw
        .as_ref()
        .ok()
        .and_then(|raw| serde_json::from_slice::<Value>(raw).ok());
    let reject = |status, code, reason| {
        tracing::warn!(
            reason,
            client_name = ?metadata.as_ref().and_then(|value| value.get("client_name")),
            redirect_uris = ?metadata.as_ref().and_then(|value| value.get("redirect_uris")),
            "OAuth client registration rejected"
        );
        oauth_error(status, code, reason)
    };
    if rate_limited(&state.oauth_limiter, &client_ip(&state, &headers, peer)) {
        return reject(
            StatusCode::TOO_MANY_REQUESTS,
            "invalid_client_metadata",
            "too many requests; try again in a minute",
        );
    }
    if raw.is_err() {
        return reject(
            StatusCode::BAD_REQUEST,
            "invalid_client_metadata",
            "could not read registration body or body is too large",
        );
    }
    let Some(metadata) = metadata.as_ref() else {
        return reject(
            StatusCode::BAD_REQUEST,
            "invalid_client_metadata",
            "invalid JSON",
        );
    };
    let body: RegisterRequest = match serde_json::from_value(metadata.clone()) {
        Ok(body) => body,
        Err(_) => return reject(StatusCode::BAD_REQUEST, "invalid_client_metadata", "provide client metadata with redirect_uris as an array of URLs and optional string client_name and token_endpoint_auth_method"),
    };
    let Some(name) = clean_name(body.client_name.as_deref().unwrap_or("MCP client")) else {
        return reject(
            StatusCode::BAD_REQUEST,
            "invalid_client_metadata",
            "Client name must be non-blank, at most 80 UTF-8 bytes after trimming, and contain no control characters.",
        );
    };
    if body.redirect_uris.is_empty() || body.redirect_uris.len() > 8 {
        return reject(
            StatusCode::BAD_REQUEST,
            "invalid_redirect_uri",
            "provide 1 to 8 redirect URLs",
        );
    }
    if body.redirect_uris.iter().any(|uri| !valid_redirect(uri)) {
        return reject(
            StatusCode::BAD_REQUEST,
            "invalid_redirect_uri",
            "redirect URLs must be https, a loopback http URL, or a private app scheme",
        );
    }
    let method = match body.token_endpoint_auth_method.as_deref().unwrap_or("none") {
        method @ ("none" | "client_secret_post" | "client_secret_basic") => method,
        _ => "client_secret_post",
    };
    let count = match sqlx::query_scalar!(r#"SELECT count(*) AS "count!" FROM oauth_clients"#)
        .fetch_one(&state.pool)
        .await
    {
        Ok(count) => count,
        Err(_) => {
            return reject(
                StatusCode::INTERNAL_SERVER_ERROR,
                "invalid_client_metadata",
                "could not register client",
            )
        }
    };
    if count >= 200 {
        return reject(
            StatusCode::TOO_MANY_REQUESTS,
            "invalid_client_metadata",
            "too many agent clients are registered; unused ones are removed after 7 days",
        );
    }
    let client_id = auth::random_token("rdr_cli");
    let secret = (method != "none").then(|| auth::random_token("rdr_cs"));
    let secret_hash = secret.as_deref().map(auth::sha256_hex);
    if sqlx::query!(
        r#"
        INSERT INTO oauth_clients (client_id, client_name, redirect_uris, token_endpoint_auth_method, client_secret_hash)
        VALUES ($1, $2, $3, $4, $5)
        "#,
        client_id,
        name,
        &body.redirect_uris,
        method,
        secret_hash,
    )
    .execute(&state.pool)
    .await.is_err() {
        return reject(StatusCode::INTERNAL_SERVER_ERROR, "invalid_client_metadata", "could not register client");
    }
    let mut registration = json!({
        "client_id": client_id,
        "client_name": name,
        "redirect_uris": body.redirect_uris,
        "grant_types": ["authorization_code", "refresh_token"],
        "response_types": ["code"],
        "token_endpoint_auth_method": method,
        "client_id_issued_at": chrono::Utc::now().timestamp(),
    });
    if let Some(secret) = secret {
        registration["client_secret"] = json!(secret);
        registration["client_secret_expires_at"] = json!(0);
    }
    no_store((StatusCode::CREATED, Json(registration)).into_response())
}

#[derive(Deserialize)]
struct AuthorizeQuery {
    response_type: String,
    client_id: String,
    redirect_uri: String,
    code_challenge: String,
    code_challenge_method: String,
    state: String,
    #[serde(default)]
    resource: Option<String>,
}

async fn authorize(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<AuthorizeQuery>,
    request: Request,
) -> Result<Response, ApiError> {
    validate_login(
        &query.response_type,
        &query.code_challenge_method,
        &query.code_challenge,
        &query.state,
        query.resource.as_deref(),
    )?;
    load_client_redirect(&state, &query.client_id, &query.redirect_uri).await?;
    let raw = request.uri().query().unwrap_or("");
    if auth::session_user(&state, &headers).await.is_none() {
        let back = format!("/oauth/authorize?{raw}");
        let next = percent(&back);
        return Ok(Redirect::temporary(&format!("/login?next={next}")).into_response());
    }
    Ok(Redirect::temporary(&format!("/mcp/consent?{raw}")).into_response())
}

pub(crate) fn validate_login(
    response_type: &str,
    method: &str,
    challenge: &str,
    state: &str,
    resource: Option<&str>,
) -> Result<(), ApiError> {
    if response_type != "code" {
        return Err(bad("response_type must be code"));
    }
    if method != "S256" {
        return Err(bad("code_challenge_method must be S256"));
    }
    if !(43..=128).contains(&challenge.len())
        || !challenge
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(bad("code_challenge must be the S256 value"));
    }
    if state.is_empty() {
        return Err(bad("state is required"));
    }
    if state.len() > 4096 {
        return Err(bad("state is too long"));
    }
    if resource.is_some_and(|resource| !resource.is_empty() && !resource.ends_with("/mcp")) {
        return Err(bad("resource must be the panel MCP address"));
    }
    Ok(())
}

#[derive(Deserialize)]
struct TokenForm {
    grant_type: String,
    #[serde(default)]
    code: String,
    #[serde(default)]
    redirect_uri: String,
    #[serde(default)]
    client_id: String,
    #[serde(default)]
    client_secret: Option<String>,
    #[serde(default)]
    code_verifier: String,
    #[serde(default)]
    refresh_token: String,
}

async fn authenticate_client(
    state: &AppState,
    headers: &HeaderMap,
    body_id: &str,
    body_secret: Option<&str>,
) -> Result<String, Box<Response>> {
    let invalid = || {
        let mut response = oauth_error(
            StatusCode::UNAUTHORIZED,
            "invalid_client",
            "invalid client credentials",
        );
        response.headers_mut().insert(
            header::WWW_AUTHENTICATE,
            "Basic realm=\"oauth\"".parse().unwrap(),
        );
        Box::new(response)
    };
    let basic = if let Some(header) = headers.get(header::AUTHORIZATION) {
        let value = header.to_str().map_err(|_| invalid())?;
        let (scheme, encoded) = value.split_once(' ').ok_or_else(invalid)?;
        if !scheme.eq_ignore_ascii_case("Basic") || body_secret.is_some() {
            return Err(invalid());
        }
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|_| invalid())?;
        let decoded = String::from_utf8(decoded).map_err(|_| invalid())?;
        let (id, secret) = decoded.split_once(':').ok_or_else(invalid)?;
        let decode = |value: &str| -> Result<String, Box<Response>> {
            percent_encoding::percent_decode_str(&value.replace('+', " "))
                .decode_utf8()
                .map(|value| value.into_owned())
                .map_err(|_| invalid())
        };
        Some((decode(id)?, decode(secret)?))
    } else {
        None
    };
    let (id, secret) = match &basic {
        Some((id, secret)) => {
            if !body_id.is_empty() && body_id != id {
                return Err(invalid());
            }
            (id.as_str(), Some(secret.as_str()))
        }
        None => (body_id, body_secret),
    };
    let row: Option<(String, Option<String>)> = sqlx::query_as(
        "SELECT token_endpoint_auth_method, client_secret_hash FROM oauth_clients WHERE client_id = $1",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|_| Box::new(oauth_error(StatusCode::INTERNAL_SERVER_ERROR, "server_error", "could not authenticate client")))?;
    let Some((registered_method, hash)) = row else {
        return Err(invalid());
    };
    if registered_method == "none" {
        if basic.is_some() || body_secret.is_some() {
            return Err(invalid());
        }
    } else if !secret
        .zip(hash.as_deref())
        .is_some_and(|(secret, hash)| auth::tokens_eq(&auth::sha256_hex(secret), hash))
    {
        return Err(invalid());
    }
    Ok(id.to_owned())
}

async fn token(
    State(state): State<AppState>,
    headers: HeaderMap,
    auth::OptionalPeer(peer): auth::OptionalPeer,
    body: axum::body::Bytes,
) -> Response {
    if rate_limited(&state.oauth_limiter, &client_ip(&state, &headers, peer)) {
        return oauth_error(
            StatusCode::TOO_MANY_REQUESTS,
            "temporarily_unavailable",
            "too many token requests",
        );
    }
    if body.len() > 64 * 1024 {
        return oauth_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "body is too large",
        );
    }
    let content = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    let form: TokenForm = if content.starts_with("application/json") {
        match serde_json::from_slice::<TokenForm>(&body) {
            Ok(form) => form,
            Err(_) => {
                return oauth_error(StatusCode::BAD_REQUEST, "invalid_request", "invalid JSON")
            }
        }
    } else {
        match serde_urlencoded::from_bytes(&body) {
            Ok(form) => form,
            Err(_) => {
                return oauth_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_request",
                    "invalid form body",
                )
            }
        }
    };
    let client_id = match authenticate_client(
        &state,
        &headers,
        &form.client_id,
        form.client_secret.as_deref(),
    )
    .await
    {
        Ok(id) => id,
        Err(response) => return *response,
    };
    let issued = match form.grant_type.as_str() {
        "authorization_code" => {
            tokens::exchange_code(
                &state,
                &client_id,
                &form.code,
                &form.redirect_uri,
                &form.code_verifier,
            )
            .await
        }
        "refresh_token" => tokens::exchange_refresh(&state, &client_id, &form.refresh_token).await,
        _ => {
            return oauth_error(
                StatusCode::BAD_REQUEST,
                "unsupported_grant_type",
                "supported grants are authorization_code and refresh_token",
            )
        }
    };
    match issued {
        Ok(issued) => no_store(
            Json(json!({
                "access_token": issued.access_token,
                "token_type": "Bearer",
                "expires_in": issued.expires_in,
                "refresh_token": issued.refresh_token,
                "scope": issued.scope,
            }))
            .into_response(),
        ),
        Err(ExchangeError::Invalid(message)) => {
            oauth_error(StatusCode::BAD_REQUEST, "invalid_grant", message)
        }
        Err(ExchangeError::Server) => oauth_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "server_error",
            "could not issue a token",
        ),
    }
}

#[derive(Deserialize)]
struct RevokeForm {
    token: String,
    #[serde(default)]
    client_id: String,
    #[serde(default)]
    client_secret: Option<String>,
}

async fn revoke(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let content = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    let form: RevokeForm = if content.starts_with("application/json") {
        serde_json::from_slice(&body).unwrap_or(RevokeForm {
            token: String::new(),
            client_id: String::new(),
            client_secret: None,
        })
    } else {
        serde_urlencoded::from_bytes(&body).unwrap_or(RevokeForm {
            token: String::new(),
            client_id: String::new(),
            client_secret: None,
        })
    };
    let mut id = form.client_id;
    if id.is_empty() && !headers.contains_key(header::AUTHORIZATION) && form.client_secret.is_none()
    {
        let hash = auth::sha256_hex(&form.token);
        let found: Result<Option<String>, _> = sqlx::query_scalar(
            "SELECT g.client_id FROM mcp_grants g JOIN mcp_access_tokens t ON t.grant_id = g.id WHERE t.token_hash = $1 UNION ALL SELECT g.client_id FROM mcp_grants g JOIN mcp_refresh_tokens t ON t.grant_id = g.id WHERE t.token_hash = $1 LIMIT 1",
        ).bind(hash).fetch_optional(&state.pool).await;
        match found {
            Ok(Some(found)) => id = found,
            Ok(None) => return no_store(StatusCode::OK.into_response()),
            Err(_) => {
                return oauth_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "server_error",
                    "could not authenticate client",
                )
            }
        }
    }
    let client_id =
        match authenticate_client(&state, &headers, &id, form.client_secret.as_deref()).await {
            Ok(id) => id,
            Err(response) => return *response,
        };
    if !form.token.is_empty() {
        let _ = tokens::revoke(&state, &form.token, &client_id).await;
    }
    no_store(StatusCode::OK.into_response())
}

async fn mcp_unused() -> Response {
    let mut response = StatusCode::METHOD_NOT_ALLOWED.into_response();
    response
        .headers_mut()
        .insert(header::ALLOW, "POST, DELETE".parse().unwrap());
    no_store(response)
}

async fn mcp_end() -> Response {
    no_store(StatusCode::NO_CONTENT.into_response())
}

async fn rpc(
    State(state): State<AppState>,
    headers: HeaderMap,
    auth::OptionalPeer(peer): auth::OptionalPeer,
    body: axum::body::Bytes,
) -> Response {
    let origin = origin(&state, &headers).await.unwrap_or_default();
    if !origin_ok(&headers, &origin) {
        return no_store(
            (
                StatusCode::FORBIDDEN,
                Json(json!({"error": "cross-origin MCP requests are not allowed"})),
            )
                .into_response(),
        );
    }
    if rate_limited(&state.mcp_limiter, &client_ip(&state, &headers, peer)) {
        return no_store(
            (
                StatusCode::TOO_MANY_REQUESTS,
                [(header::RETRY_AFTER, "5")],
                Json(rpc_error(Value::Null, -32000, "too many agent requests")),
            )
                .into_response(),
        );
    }
    if body.len() > MAX_BODY {
        return no_store(
            (
                StatusCode::PAYLOAD_TOO_LARGE,
                Json(rpc_error(Value::Null, -32600, "request is too large")),
            )
                .into_response(),
        );
    }
    let message: Value = match serde_json::from_slice::<Value>(&body) {
        Ok(value) if value.is_object() => value,
        _ => {
            return no_store(
                (
                    StatusCode::BAD_REQUEST,
                    Json(rpc_error(Value::Null, -32700, "expected a JSON-RPC object")),
                )
                    .into_response(),
            )
        }
    };
    let id = message.get("id").cloned().unwrap_or(Value::Null);
    let method = message
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if message.get("id").is_none() {
        return no_store(StatusCode::ACCEPTED.into_response());
    }
    let token = bearer(&headers);
    let call_started = std::time::Instant::now();
    let params = message.get("params").cloned().unwrap_or_else(|| json!({}));
    let Some(token) = token else {
        if method == "tools/call" {
            record_unauthorized(&state, None, &params, call_started).await;
        }
        return unauthorized_mcp(&origin, id);
    };
    let user = match tokens::load_access(&state, &token).await {
        Ok(user) => user,
        Err(_) => {
            if method == "tools/call" {
                record_unauthorized(&state, Some(&token), &params, call_started).await;
            }
            return unauthorized_mcp(&origin, id);
        }
    };
    let result = match method {
        "initialize" => initialize(&params),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(tools_list(&user)),
        "tools/call" => match tools_call(&state, &token, &user, &params, call_started).await {
            Ok(result) => Ok(result),
            Err(message) => Err(rpc_error(
                id.clone(),
                if tool_error_status(message).is_server_error() {
                    -32603
                } else {
                    -32602
                },
                message,
            )),
        },
        "resources/list" => Ok(json!({"resources": []})),
        "prompts/list" => Ok(json!({"prompts": []})),
        _ => Err(rpc_error(id.clone(), -32601, "method not found")),
    };
    let body = match result {
        Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
        Err(error) => error,
    };
    no_store(Json(body).into_response())
}

fn initialize(params: &Value) -> Result<Value, Value> {
    let requested = params
        .get("protocolVersion")
        .and_then(Value::as_str)
        .unwrap_or(PROTOCOL);
    let version = if requested == PROTOCOL || requested == "2025-06-18" {
        requested
    } else {
        PROTOCOL
    };
    Ok(json!({
        "protocolVersion": version,
        "capabilities": {"tools": {"listChanged": false}},
        "serverInfo": {"name": "dymini-panel", "version": dymini_panel_core::VERSION},
        "instructions": "Call dymini_panel_whoami to see this approval. Other tools are the panel API. Full control can read and change every setting and all data. Pass path parameters, query parameters, and a JSON body.",
    }))
}

fn tools_list(user: &AuthUser) -> Value {
    let mut tools: Vec<Value> = super::tools()
        .into_iter()
        .filter(|tool| super::permissions::allows(user, &tool.name, &tool.scope))
        .map(tool_json)
        .collect();
    tools.insert(
        0,
        json!({
            "name": "dymini_panel_whoami",
            "description": "Who this agent is, which permission template was approved, and which scopes it has.",
            "inputSchema": {"type": "object", "properties": {}, "additionalProperties": false},
        }),
    );
    json!({"tools": tools})
}

fn tool_json(tool: Tool) -> Value {
    let mut properties = serde_json::Map::new();
    let mut required = Vec::new();
    let names = path_params(tool.path);
    if !names.is_empty() {
        let mut path_props = serde_json::Map::new();
        for name in &names {
            path_props.insert((*name).to_owned(), json!({"type": "string"}));
        }
        properties.insert(
            "path".into(),
            json!({
                "type": "object",
                "properties": path_props,
                "required": names,
                "additionalProperties": false,
            }),
        );
        required.push("path");
    }
    properties.insert(
        "query".into(),
        json!({
            "type": "object",
            "description": "Query parameters. Values are strings, numbers, or booleans.",
            "additionalProperties": {"type": "string"},
        }),
    );
    if tool.body_schema.is_some() {
        properties.insert(
            "body".into(),
            json!({
                "description": format!("JSON body ({})", tool.body_schema.unwrap_or("object")),
            }),
        );
    }
    json!({
        "name": tool.name,
        "description": tool.description,
        "inputSchema": {
            "type": "object",
            "properties": properties,
            "required": required,
            "additionalProperties": false,
        },
    })
}

async fn tools_call(
    state: &AppState,
    token: &str,
    user: &AuthUser,
    params: &Value,
    started: std::time::Instant,
) -> Result<Value, &'static str> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let id = super::usage::begin(state, Some(user), name)
        .await
        .map_err(|_| "could not record the tool call; no action was performed")?;
    let result = execute_tool(state, token, user, params).await;
    let (status, failed) = match &result {
        Ok((status, body)) => (*status, body["isError"].as_bool().unwrap_or(false)),
        Err(message) => (tool_error_status(message), true),
    };
    if let Err(error) = super::usage::finish(
        state,
        id,
        status,
        failed,
        started.elapsed().as_millis().min(i64::MAX as u128) as i64,
    )
    .await
    {
        tracing::warn!(message=%error.public_message(),"MCP call finalization failed");
    }
    result.map(|(_, body)| body)
}

fn tool_error_status(message: &str) -> StatusCode {
    match message {
        "unknown tool"
        | "unknown path parameter"
        | "path parameters must be strings"
        | "invalid path parameter"
        | "missing path parameter"
        | "query values must be strings, numbers, or booleans" => StatusCode::BAD_REQUEST,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

async fn record_unauthorized(
    state: &AppState,
    token: Option<&str>,
    params: &Value,
    started: std::time::Instant,
) {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let recorded = async {
        let id = super::usage::begin_unauthorized(state, token, name).await?;
        super::usage::finish(
            state,
            id,
            StatusCode::UNAUTHORIZED,
            true,
            started.elapsed().as_millis().min(i64::MAX as u128) as i64,
        )
        .await
    }
    .await;
    if let Err(error) = recorded {
        tracing::warn!(message=%error.public_message(),"MCP denial recording failed");
    }
}

async fn execute_tool(
    state: &AppState,
    token: &str,
    user: &AuthUser,
    params: &Value,
) -> Result<(StatusCode, Value), &'static str> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if name == "dymini_panel_whoami" {
        let described = tokens::describe(state, token)
            .await
            .map_err(|_| "could not read this approval")?;
        return Ok((
            StatusCode::OK,
            tool_text(StatusCode::OK, &described.to_string(), false),
        ));
    }
    let Some(tool) = super::tools().into_iter().find(|tool| tool.name == name) else {
        return Err("unknown tool");
    };
    if !super::permissions::allows(user, &tool.name, &tool.scope) {
        return Ok((
            StatusCode::FORBIDDEN,
            tool_text(
                StatusCode::FORBIDDEN,
                &format!("this approval does not include {}", tool.scope),
                false,
            ),
        ));
    }
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let path = fill_path(tool.path, &arguments)?;
    let mut builder = axum::http::Request::builder()
        .method(tool.method)
        .uri(path)
        .header(header::AUTHORIZATION, format!("Bearer {token}"));
    let body = arguments.get("body").filter(|body| !body.is_null());
    if body.is_some() {
        builder = builder.header(header::CONTENT_TYPE, "application/json");
    }
    let request = builder
        .body(match body {
            Some(body) => Body::from(body.to_string()),
            None => Body::empty(),
        })
        .map_err(|_| "could not build the request")?;
    let response = crate::router(state.clone())
        .oneshot(request)
        .await
        .map_err(|_| "the panel did not answer")?;
    let status = response.status();
    let bytes = match axum::body::to_bytes(response.into_body(), MAX_TOOL_RESPONSE).await {
        Ok(bytes) => (bytes.to_vec(), false),
        Err(_) => (b"response truncated; narrow the query".to_vec(), true),
    };
    let text = String::from_utf8_lossy(&bytes.0);
    Ok((
        status,
        tool_text(
            status,
            &format!("HTTP {}\n{text}", status.as_u16()),
            bytes.1,
        ),
    ))
}

fn fill_path(pattern: &str, arguments: &Value) -> Result<String, &'static str> {
    let provided = arguments
        .get("path")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut path = pattern.to_owned();
    for (key, value) in &provided {
        let needle = format!("{{{key}}}");
        if !path.contains(&needle) {
            return Err("unknown path parameter");
        }
        let text = match value {
            Value::String(text) => text.as_str(),
            _ => return Err("path parameters must be strings"),
        };
        if text.is_empty() || text.contains('/') {
            return Err("invalid path parameter");
        }
        let encoded =
            percent_encoding::utf8_percent_encode(text, percent_encoding::NON_ALPHANUMERIC)
                .to_string();
        path = path.replace(&needle, &encoded);
    }
    if path.contains('{') {
        return Err("missing path parameter");
    }
    if let Some(query) = arguments.get("query").and_then(Value::as_object) {
        let mut pairs = Vec::new();
        for (key, value) in query {
            let text = match value {
                Value::String(text) => text.clone(),
                Value::Number(number) => number.to_string(),
                Value::Bool(flag) => flag.to_string(),
                Value::Null => continue,
                _ => return Err("query values must be strings, numbers, or booleans"),
            };
            pairs.push(format!("{}={}", percent(key), percent(&text)));
        }
        if !pairs.is_empty() {
            path.push('?');
            path.push_str(&pairs.join("&"));
        }
    }
    Ok(path)
}

fn path_params(pattern: &str) -> Vec<&str> {
    pattern
        .split('/')
        .filter_map(|segment| {
            segment
                .strip_prefix('{')
                .and_then(|rest| rest.strip_suffix('}'))
        })
        .collect()
}

fn tool_text(status: StatusCode, text: &str, truncated: bool) -> Value {
    json!({
        "content": [{"type": "text", "text": text}],
        "isError": truncated || status.as_u16() >= 400,
    })
}

fn rpc_error(id: Value, code: i32, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {"code": code, "message": message},
    })
}

fn unauthorized_mcp(origin: &str, id: Value) -> Response {
    let metadata = format!("{origin}/.well-known/oauth-protected-resource");
    let mut response = (
        StatusCode::UNAUTHORIZED,
        Json(rpc_error(id, -32001, "sign in required")),
    )
        .into_response();
    if let Ok(value) =
        format!("Bearer realm=\"dymini-panel\", resource_metadata=\"{metadata}\"").parse()
    {
        response
            .headers_mut()
            .insert(header::WWW_AUTHENTICATE, value);
    }
    no_store(response)
}

fn oauth_error(status: StatusCode, code: &str, description: &str) -> Response {
    no_store(
        (
            status,
            Json(json!({"error": code, "error_description": description})),
        )
            .into_response(),
    )
}

fn origin_ok(headers: &HeaderMap, expected: &str) -> bool {
    let Some(origin) = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
    else {
        return true;
    };
    if expected.is_empty() || origin == "null" {
        return false;
    }
    origin.trim_end_matches('/') == expected.trim_end_matches('/')
}

fn rate_limited(limiter: &crate::state::LoginLimiter, ip: &str) -> bool {
    limiter.check_key(&ip.to_owned()).is_err()
}

fn client_ip(state: &AppState, headers: &HeaderMap, peer: Option<SocketAddr>) -> String {
    auth::client_ip(headers, peer, &state.config.trusted_proxies)
}

fn bad(message: &'static str) -> ApiError {
    ApiError::new(StatusCode::BAD_REQUEST, "invalid_request", message)
}

fn percent(value: &str) -> String {
    percent_encoding::utf8_percent_encode(value, percent_encoding::NON_ALPHANUMERIC).to_string()
}

fn clean_name(name: &str) -> Option<String> {
    let name = name.trim();
    if (1..=80).contains(&name.len()) && !name.chars().any(|c| c.is_control()) {
        Some(name.to_owned())
    } else {
        None
    }
}

pub(crate) fn valid_redirect(uri: &str) -> bool {
    if uri.len() > 500 || uri.contains('#') || uri.contains('\\') || uri.contains('@') {
        return false;
    }
    let Ok(url) = url::Url::parse(uri) else {
        return false;
    };
    match url.scheme() {
        "https" => url.host_str().is_some(),
        "http" => matches!(
            url.host_str(),
            Some("localhost") | Some("127.0.0.1") | Some("[::1]") | Some("::1")
        ),
        "javascript" | "data" | "file" | "vbscript" => false,
        scheme
            if (2..=40).contains(&scheme.len())
                && scheme
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '.' || c == '-') =>
        {
            true
        }
        _ => false,
    }
}

fn bearer(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| {
            value
                .strip_prefix("Bearer ")
                .or_else(|| value.strip_prefix("bearer "))
        })
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(str::to_owned)
}

pub async fn load_client_redirect(
    state: &AppState,
    client_id: &str,
    redirect_uri: &str,
) -> Result<(String, String), ApiError> {
    let row = sqlx::query!(
        r#"SELECT client_name, redirect_uris FROM oauth_clients WHERE client_id = $1"#,
        client_id,
    )
    .fetch_optional(&state.pool)
    .await
    .map_err(ApiError::database)?;
    let Some(row) = row else {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_client",
            "unknown agent client",
        ));
    };
    if !row.redirect_uris.iter().any(|uri| uri == redirect_uri) {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_redirect",
            "redirect URL is not registered for this client",
        ));
    }
    Ok((row.client_name, redirect_uri.to_owned()))
}

pub fn redirect_with(redirect_uri: &str, code: &str, state: &str) -> Result<String, ApiError> {
    let mut url = url::Url::parse(redirect_uri).map_err(|_| bad("redirect URL is invalid"))?;
    url.query_pairs_mut()
        .append_pair("code", code)
        .append_pair("state", state);
    Ok(url.to_string())
}

pub async fn record_grant(state: &AppState, user: &AuthUser, client_id: &str, template: &str) {
    audit::record(
        &state.pool,
        Some(user.id),
        "mcp.grant",
        "mcp_grant",
        Some(client_id),
        json!({"template": template}),
        None,
    )
    .await;
}
