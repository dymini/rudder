use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Json,
};
use axum_extra::extract::cookie::CookieJar;
use chrono::{DateTime, Utc};
use rand_core06::{OsRng, RngCore};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use utoipa::ToSchema;
use uuid::Uuid;
use webauthn_rs::prelude::*;

use crate::{
    audit,
    auth::{self, AuthUser},
    error::ApiError,
    mailer::MailConfig,
    state::AppState,
};

#[derive(Default, Deserialize, Serialize, ToSchema)]
pub struct FactorInput {
    pub totp_code: Option<String>,
    pub email_code: Option<String>,
    pub mfa_proof: Option<String>,
}
#[derive(Deserialize, ToSchema)]
pub struct MfaReauth {
    pub password: String,
    #[serde(flatten)]
    pub factor: FactorInput,
}
#[derive(Deserialize, ToSchema)]
pub struct PasskeyRegister {
    pub name: String,
    #[serde(flatten)]
    pub reauth: MfaReauth,
}
#[derive(Deserialize, ToSchema)]
pub struct PasskeyVerify {
    pub email: String,
    pub password: String,
}
#[derive(Deserialize, ToSchema)]
pub struct MfaFinish {
    pub challenge_id: String,
    pub credential: Value,
}
#[derive(Serialize, ToSchema)]
pub struct MfaChallenge {
    pub challenge_id: String,
    pub options: Value,
}
#[derive(Serialize, ToSchema)]
pub struct MfaProof {
    pub mfa_proof: String,
}
#[derive(Deserialize, ToSchema)]
pub struct PasskeyName {
    pub name: String,
}
#[derive(Serialize, ToSchema, sqlx::FromRow)]
pub struct PasskeyView {
    pub id: Uuid,
    pub name: String,
    pub counter: i64,
    pub created_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
}
#[derive(Serialize, ToSchema)]
pub struct MfaMethods {
    pub totp: bool,
    pub email: bool,
    pub passkeys: i64,
    pub email_available: bool,
    pub passkey_origin: Option<String>,
}
#[derive(Deserialize, ToSchema)]
pub struct EmailMfaSend {
    pub purpose: String,
    #[serde(flatten)]
    pub reauth: MfaReauth,
}
#[derive(Deserialize, ToSchema)]
pub struct EmailMfaEnable {
    pub password: String,
    pub code: String,
}

#[derive(sqlx::FromRow)]
struct MfaUser {
    id: Uuid,
    email: String,
    display_name: String,
    role: String,
    password_hash: String,
    totp_enabled: bool,
    totp_secret: Option<Vec<u8>>,
    recovery_hashes: Vec<String>,
    email_mfa_enabled: bool,
    is_owner: bool,
}
#[derive(sqlx::FromRow)]
struct KeyRow {
    id: Uuid,
    credential: sqlx::types::Json<Passkey>,
    counter: i64,
}
#[derive(Serialize, Deserialize)]
struct Saved<T> {
    origin: String,
    inner: T,
}

pub fn router() -> axum::Router<AppState> {
    axum::Router::new()
        .route("/api/v1/me/mfa", get(methods))
        .route("/api/v1/me/passkeys", get(list))
        .route(
            "/api/v1/me/passkeys/{id}",
            axum::routing::patch(rename).delete(remove),
        )
        .route("/api/v1/me/passkeys/register/start", post(register_start))
        .route("/api/v1/me/passkeys/register/finish", post(register_finish))
        .route("/api/v1/auth/passkeys/verify/start", post(verify_start))
        .route("/api/v1/auth/passkeys/verify/finish", post(verify_finish))
        .route("/api/v1/auth/passkeys/login/start", post(discover_start))
        .route("/api/v1/auth/passkeys/login/finish", post(discover_finish))
        .route("/api/v1/auth/mfa/email/send", post(email_login_send))
        .route("/api/v1/me/mfa/email/send", post(email_send))
        .route("/api/v1/me/mfa/email/enable", post(email_enable))
        .route("/api/v1/me/mfa/email/disable", post(email_disable))
}

fn invalid() -> ApiError {
    ApiError::new(
        StatusCode::UNAUTHORIZED,
        "invalid_mfa",
        "Password or current factor is invalid, expired, or already used",
    )
}
fn bad(message: &str) -> ApiError {
    ApiError::new(StatusCode::BAD_REQUEST, "invalid_mfa_request", message)
}
fn limited() -> ApiError {
    ApiError::new(
        StatusCode::TOO_MANY_REQUESTS,
        "rate_limited",
        "Too many MFA requests; wait before trying again",
    )
}
fn name(value: &str) -> Result<String, ApiError> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > 100 || value.chars().any(char::is_control) {
        return Err(bad(
            "Passkey names must contain 1–100 characters and no control characters",
        ));
    }
    Ok(value.into())
}
async fn user(state: &AppState, id: Uuid) -> Result<MfaUser, ApiError> {
    sqlx::query_as("SELECT id,email,display_name,role,password_hash,totp_enabled,totp_secret,recovery_hashes,email_mfa_enabled,is_owner FROM users WHERE id=$1")
        .bind(id).fetch_optional(&state.pool).await.map_err(ApiError::database)?.ok_or_else(invalid)
}
async fn password_user(state: &AppState, email: &str, password: &str) -> Result<MfaUser, ApiError> {
    let email = auth::normalize_email(email).unwrap_or_default();
    let id: Option<Uuid> = sqlx::query_scalar("SELECT id FROM users WHERE email=$1")
        .bind(email)
        .fetch_optional(&state.pool)
        .await
        .map_err(ApiError::database)?;
    let Some(id) = id else {
        auth::dummy_verify(password);
        return Err(invalid());
    };
    let u = user(state, id).await?;
    if !auth::verify_password(password, &u.password_hash) {
        return Err(invalid());
    }
    Ok(u)
}
pub(super) fn limit(
    state: &AppState,
    headers: &HeaderMap,
    peer: Option<std::net::SocketAddr>,
) -> Result<(), ApiError> {
    let ip = auth::client_ip(headers, peer, &state.config.trusted_proxies);
    state
        .login_limiter
        .check_key(&format!("mfa:{ip}"))
        .map_err(|_| limited())
}
async fn origin(state: &AppState) -> Result<String, ApiError> {
    let (domain, mode): (Option<String>, String) =
        sqlx::query_as("SELECT panel_domain,panel_ssl_mode FROM panel_settings WHERE id=TRUE")
            .fetch_one(&state.pool)
            .await
            .map_err(ApiError::database)?;
    let raw = if let Some(url) = &state.config.public_url {
        url.clone()
    } else if let Some(domain) = domain.filter(|d| !d.is_empty()) {
        format!(
            "{}://{domain}",
            if mode == "none" { "http" } else { "https" }
        )
    } else {
        return Err(bad(
            "Configure the panel domain or DYMINI_PANEL_PUBLIC_URL before using passkeys",
        ));
    };
    let url = Url::parse(&raw).map_err(|_| bad("The panel public URL is invalid"))?;
    if url.scheme() != "https" && !(url.scheme() == "http" && url.host_str() == Some("localhost")) {
        return Err(bad("Passkeys require HTTPS, except on localhost"));
    }
    Ok(url.origin().ascii_serialization())
}
async fn webauthn(state: &AppState) -> Result<(Webauthn, String), ApiError> {
    let origin = origin(state).await?;
    let url = Url::parse(&origin).map_err(|_| bad("The panel origin is invalid"))?;
    let rp = url
        .host_str()
        .ok_or_else(|| bad("The panel URL needs a hostname"))?;
    let webauthn = WebauthnBuilder::new(rp, &url)
        .map_err(|_| bad("The panel origin cannot be used for WebAuthn"))?
        .rp_name("Dymini Panel")
        .build()
        .map_err(|_| bad("Could not initialize WebAuthn"))?;
    Ok((webauthn, origin))
}
async fn save<T: Serialize>(
    state: &AppState,
    uid: Option<Uuid>,
    purpose: &str,
    inner: &T,
) -> Result<String, ApiError> {
    sqlx::query("DELETE FROM mfa_challenges WHERE expires_at<=now()")
        .execute(&state.pool)
        .await
        .map_err(ApiError::database)?;
    let bytes = serde_json::to_vec(inner).map_err(|_| bad("Could not save MFA state"))?;
    let encrypted = state
        .cipher
        .encrypt(&bytes)
        .map_err(|_| bad("Could not protect MFA state"))?;
    let token = auth::random_token("");
    sqlx::query(
        "INSERT INTO mfa_challenges(token_hash,user_id,purpose,state_enc) VALUES ($1,$2,$3,$4)",
    )
    .bind(auth::sha256_hex(&token))
    .bind(uid)
    .bind(purpose)
    .bind(encrypted)
    .execute(&state.pool)
    .await
    .map_err(ApiError::database)?;
    Ok(token)
}
async fn take<T: DeserializeOwned>(
    state: &AppState,
    token: &str,
    purpose: &str,
    uid: Option<Uuid>,
) -> Result<(Option<Uuid>, T), ApiError> {
    let row: Option<(Option<Uuid>,Vec<u8>)>=sqlx::query_as("DELETE FROM mfa_challenges WHERE token_hash=$1 AND purpose=$2 AND expires_at>now() AND ($3::uuid IS NULL OR user_id=$3) RETURNING user_id,state_enc").bind(auth::sha256_hex(token)).bind(purpose).bind(uid).fetch_optional(&state.pool).await.map_err(ApiError::database)?;
    let (id, bytes) = row.ok_or_else(invalid)?;
    let plaintext = state.cipher.decrypt(&bytes).map_err(|_| invalid())?;
    Ok((
        id,
        serde_json::from_slice(&plaintext).map_err(|_| invalid())?,
    ))
}
async fn keys(state: &AppState, id: Uuid) -> Result<Vec<Passkey>, ApiError> {
    let rows: Vec<sqlx::types::Json<Passkey>> =
        sqlx::query_scalar("SELECT credential FROM user_passkeys WHERE user_id=$1")
            .bind(id)
            .fetch_all(&state.pool)
            .await
            .map_err(ApiError::database)?;
    Ok(rows.into_iter().map(|p| p.0).collect())
}
pub(super) async fn require_factor(
    state: &AppState,
    id: Uuid,
    input: &FactorInput,
    purpose: &str,
) -> Result<(), ApiError> {
    let u = user(state, id).await?;
    if let Some(proof) = input.mfa_proof.as_deref() {
        let (_, credential): (_, Vec<u8>) = take(state, proof, "factor", Some(id)).await?;
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM user_passkeys WHERE user_id=$1 AND credential_id=$2)",
        )
        .bind(id)
        .bind(credential)
        .fetch_one(&state.pool)
        .await
        .map_err(ApiError::database)?;
        return if exists { Ok(()) } else { Err(invalid()) };
    }
    if let Some(code) = input.email_code.as_deref() {
        if u.email_mfa_enabled && consume_email(state, id, purpose, code).await? {
            return Ok(());
        }
        return Err(invalid());
    }
    if let Some(code) = input.totp_code.as_deref() {
        if u.totp_enabled {
            let secret = super::account::decrypt_secret(state, u.totp_secret.as_deref())?;
            if super::account::totp_matches(&secret, &u.email, code)?
                || super::account::consume_recovery(state, id, &u.recovery_hashes, code).await?
            {
                return Ok(());
            }
        }
        return Err(invalid());
    }
    if u.totp_enabled || u.email_mfa_enabled || !keys(state, id).await?.is_empty() {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            if u.totp_enabled && !u.email_mfa_enabled && keys(state, id).await?.is_empty() {
                "totp_required"
            } else {
                "mfa_required"
            },
            "Use an authenticator code, an email code, or a passkey",
        ));
    }
    Ok(())
}
pub(super) async fn reauth(state: &AppState, id: Uuid, body: &MfaReauth) -> Result<(), ApiError> {
    let u = user(state, id).await?;
    if !auth::verify_password(&body.password, &u.password_hash) {
        return Err(invalid());
    }
    require_factor(state, id, &body.factor, "reauth").await
}
fn require_current_factor(body: &MfaReauth) -> Result<(), ApiError> {
    if body.factor.mfa_proof.is_none()
        && body.factor.email_code.is_none()
        && body.factor.totp_code.is_none()
    {
        return Err(invalid());
    }
    Ok(())
}
async fn methods(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<MfaMethods>, ApiError> {
    let u = user(&state, auth.id).await?;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM user_passkeys WHERE user_id=$1")
        .bind(auth.id)
        .fetch_one(&state.pool)
        .await
        .map_err(ApiError::database)?;
    let ready:bool=sqlx::query_scalar("SELECT email_provider<>'none' AND email_config_enc IS NOT NULL AND email_from<>'' FROM access_settings WHERE id=TRUE").fetch_one(&state.pool).await.map_err(ApiError::database)?;
    Ok(Json(MfaMethods {
        totp: u.totp_enabled,
        email: u.email_mfa_enabled,
        passkeys: count,
        email_available: ready,
        passkey_origin: origin(&state).await.ok(),
    }))
}
async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Vec<PasskeyView>>, ApiError> {
    Ok(Json(sqlx::query_as("SELECT id,name,counter,created_at,last_used_at FROM user_passkeys WHERE user_id=$1 ORDER BY created_at,id").bind(auth.id).fetch_all(&state.pool).await.map_err(ApiError::database)?))
}
async fn rename(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(body): Json<PasskeyName>,
) -> Result<StatusCode, ApiError> {
    auth.require_write()?;
    let affected = sqlx::query("UPDATE user_passkeys SET name=$3 WHERE id=$1 AND user_id=$2")
        .bind(id)
        .bind(auth.id)
        .bind(name(&body.name)?)
        .execute(&state.pool)
        .await
        .map_err(ApiError::database)?
        .rows_affected();
    if affected == 0 {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "not_found",
            "Passkey not found",
        ));
    }
    Ok(StatusCode::NO_CONTENT)
}
async fn remove(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
    auth::OptionalPeer(peer): auth::OptionalPeer,
    Json(body): Json<MfaReauth>,
) -> Result<StatusCode, ApiError> {
    auth.require_write()?;
    limit(&state, &headers, peer)?;
    require_current_factor(&body)?;
    reauth(&state, auth.id, &body).await?;
    let affected = sqlx::query("DELETE FROM user_passkeys WHERE id=$1 AND user_id=$2")
        .bind(id)
        .bind(auth.id)
        .execute(&state.pool)
        .await
        .map_err(ApiError::database)?
        .rows_affected();
    if affected == 0 {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "not_found",
            "Passkey not found",
        ));
    }
    audit::record(
        &state.pool,
        Some(auth.id),
        "user.passkey_delete",
        "passkey",
        Some(&id.to_string()),
        json!({}),
        None,
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}
async fn register_start(
    State(state): State<AppState>,
    auth: AuthUser,
    headers: HeaderMap,
    auth::OptionalPeer(peer): auth::OptionalPeer,
    Json(body): Json<PasskeyRegister>,
) -> Result<Json<MfaChallenge>, ApiError> {
    auth.require_write()?;
    limit(&state, &headers, peer)?;
    reauth(&state, auth.id, &body.reauth).await?;
    let name = name(&body.name)?;
    let (w, origin) = webauthn(&state).await?;
    let excludes = keys(&state, auth.id)
        .await?
        .into_iter()
        .map(|p| p.cred_id().clone())
        .collect();
    let (options, inner) = w
        .start_passkey_registration(auth.id, &auth.email, &auth.display_name, Some(excludes))
        .map_err(|_| invalid())?;
    let challenge_id = save(
        &state,
        Some(auth.id),
        "register",
        &Saved {
            origin,
            inner: (inner, name),
        },
    )
    .await?;
    let mut options = serde_json::to_value(options).map_err(|_| invalid())?;
    options["publicKey"]["authenticatorSelection"]["residentKey"] = json!("required");
    options["publicKey"]["authenticatorSelection"]["requireResidentKey"] = json!(true);
    Ok(Json(MfaChallenge {
        challenge_id,
        options,
    }))
}
async fn register_finish(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<MfaFinish>,
) -> Result<StatusCode, ApiError> {
    auth.require_write()?;
    let (_, saved): (_, Saved<(PasskeyRegistration, String)>) =
        take(&state, &body.challenge_id, "register", Some(auth.id)).await?;
    let (w, origin) = webauthn(&state).await?;
    if saved.origin != origin {
        return Err(invalid());
    }
    let credential: RegisterPublicKeyCredential =
        serde_json::from_value(body.credential).map_err(|_| invalid())?;
    let key = w
        .finish_passkey_registration(&credential, &saved.inner.0)
        .map_err(|_| invalid())?;
    let counter = Credential::from(key.clone()).counter;
    let result=sqlx::query("INSERT INTO user_passkeys(user_id,name,credential_id,credential,counter) VALUES ($1,$2,$3,$4,$5) ON CONFLICT(credential_id) DO NOTHING").bind(auth.id).bind(saved.inner.1).bind(key.cred_id().as_ref()).bind(sqlx::types::Json(key.clone())).bind(i64::from(counter)).execute(&state.pool).await.map_err(ApiError::database)?;
    if result.rows_affected() == 0 {
        return Err(bad("This passkey is already registered"));
    }
    audit::record(
        &state.pool,
        Some(auth.id),
        "user.passkey_register",
        "user",
        Some(&auth.id.to_string()),
        json!({}),
        None,
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}
async fn verify_start(
    State(state): State<AppState>,
    headers: HeaderMap,
    auth::OptionalPeer(peer): auth::OptionalPeer,
    Json(body): Json<PasskeyVerify>,
) -> Result<Json<MfaChallenge>, ApiError> {
    limit(&state, &headers, peer)?;
    let u = password_user(&state, &body.email, &body.password).await?;
    let (w, origin) = webauthn(&state).await?;
    let (options, inner) = w
        .start_passkey_authentication(&keys(&state, u.id).await?)
        .map_err(|_| invalid())?;
    let challenge_id = save(&state, Some(u.id), "verify", &Saved { origin, inner }).await?;
    Ok(Json(MfaChallenge {
        challenge_id,
        options: serde_json::to_value(options).map_err(|_| invalid())?,
    }))
}
async fn update_key(
    state: &AppState,
    id: Uuid,
    result: AuthenticationResult,
) -> Result<(), ApiError> {
    let mut tx = state.pool.begin().await.map_err(ApiError::database)?;
    let mut row:KeyRow=sqlx::query_as("SELECT id,credential,counter FROM user_passkeys WHERE user_id=$1 AND credential_id=$2 FOR UPDATE").bind(id).bind(result.cred_id().as_ref()).fetch_optional(&mut *tx).await.map_err(ApiError::database)?.ok_or_else(invalid)?;
    if !result.user_verified() || (row.counter > 0 && i64::from(result.counter()) <= row.counter) {
        return Err(invalid());
    }
    row.credential.0.update_credential(&result);
    sqlx::query("UPDATE user_passkeys SET credential=$2,counter=$3,last_used_at=now() WHERE id=$1")
        .bind(row.id)
        .bind(row.credential)
        .bind(i64::from(result.counter()))
        .execute(&mut *tx)
        .await
        .map_err(ApiError::database)?;
    tx.commit().await.map_err(ApiError::database)
}
async fn verify_finish(
    State(state): State<AppState>,
    Json(body): Json<MfaFinish>,
) -> Result<Json<MfaProof>, ApiError> {
    let (id, saved): (_, Saved<PasskeyAuthentication>) =
        take(&state, &body.challenge_id, "verify", None).await?;
    let id = id.ok_or_else(invalid)?;
    let (w, origin) = webauthn(&state).await?;
    if saved.origin != origin {
        return Err(invalid());
    }
    let credential: PublicKeyCredential =
        serde_json::from_value(body.credential).map_err(|_| invalid())?;
    let result = w
        .finish_passkey_authentication(&credential, &saved.inner)
        .map_err(|_| invalid())?;
    let credential_id: Vec<u8> = result.cred_id().as_ref().to_vec();
    update_key(&state, id, result).await?;
    Ok(Json(MfaProof {
        mfa_proof: save(&state, Some(id), "factor", &credential_id).await?,
    }))
}
async fn discover_start(
    State(state): State<AppState>,
    headers: HeaderMap,
    auth::OptionalPeer(peer): auth::OptionalPeer,
) -> Result<Json<MfaChallenge>, ApiError> {
    limit(&state, &headers, peer)?;
    let (w, origin) = webauthn(&state).await?;
    let (mut options, inner) = w
        .start_discoverable_authentication()
        .map_err(|_| invalid())?;
    options.mediation = None;
    let challenge_id = save(&state, None, "discover", &Saved { origin, inner }).await?;
    Ok(Json(MfaChallenge {
        challenge_id,
        options: serde_json::to_value(options).map_err(|_| invalid())?,
    }))
}
async fn discover_finish(
    State(state): State<AppState>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(body): Json<MfaFinish>,
) -> Result<(CookieJar, Json<super::account::SessionResponse>), ApiError> {
    let (_, saved): (_, Saved<DiscoverableAuthentication>) =
        take(&state, &body.challenge_id, "discover", None).await?;
    let (w, origin) = webauthn(&state).await?;
    if saved.origin != origin {
        return Err(invalid());
    }
    let credential: PublicKeyCredential =
        serde_json::from_value(body.credential).map_err(|_| invalid())?;
    let (id, _) = w
        .identify_discoverable_authentication(&credential)
        .map_err(|_| invalid())?;
    let keys: Vec<DiscoverableKey> = keys(&state, id)
        .await?
        .into_iter()
        .map(Into::into)
        .collect();
    let result = w
        .finish_discoverable_authentication(&credential, saved.inner, &keys)
        .map_err(|_| invalid())?;
    update_key(&state, id, result).await?;
    let u = user(&state, id).await?;
    let auth = AuthUser {
        id: u.id,
        email: u.email,
        display_name: u.display_name,
        role: u.role,
        totp_enabled: u.totp_enabled,
        is_owner: u.is_owner,
        csrf_token: None,
        scopes: vec!["read".into(), "write".into()],
        via_token: false,
        via_mcp: false,
        mcp: None,
    };
    audit::record(
        &state.pool,
        Some(id),
        "session.passkey_login",
        "session",
        Some(&id.to_string()),
        json!({}),
        None,
    )
    .await;
    super::account::issue(&state, &auth, &headers, jar, None).await
}

fn email_hash(state: &AppState, id: Uuid, purpose: &str, code: &str) -> Result<String, ApiError> {
    crate::monitor::notify::sign(
        &hex::encode(state.config.master_key()),
        format!("mfa:{id}:{purpose}:{code}").as_bytes(),
    )
    .map_err(|_| invalid())
}
async fn send_email(state: &AppState, u: &MfaUser, purpose: &str) -> Result<(), ApiError> {
    let (from,encrypted):(String,Option<Vec<u8>>)=sqlx::query_as("SELECT email_from,email_config_enc FROM access_settings WHERE id=TRUE AND email_provider<>'none'").fetch_optional(&state.pool).await.map_err(ApiError::database)?.ok_or_else(||bad("Configure an email provider in Access settings first"))?;
    let bytes = state
        .cipher
        .decrypt(
            &encrypted
                .ok_or_else(|| bad("Configure an email provider in Access settings first"))?,
        )
        .map_err(|_| bad("Email configuration could not be decrypted"))?;
    let config: MailConfig =
        serde_json::from_slice(&bytes).map_err(|_| bad("Email configuration is invalid"))?;
    let number = loop {
        let n = OsRng.next_u32();
        if n < 4_294_000_000 {
            break n % 1_000_000;
        }
    };
    let code = format!("{number:06}");
    let hash = email_hash(state, u.id, purpose, &code)?;
    let mut tx = state.pool.begin().await.map_err(ApiError::database)?;
    sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
        .bind(u.id)
        .execute(&mut *tx)
        .await
        .map_err(ApiError::database)?;
    let recent:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM mfa_email_codes WHERE user_id=$1 AND created_at>now()-interval '1 minute')").bind(u.id).fetch_one(&mut *tx).await.map_err(ApiError::database)?;
    if recent {
        return Err(limited());
    }
    sqlx::query("INSERT INTO mfa_email_codes(user_id,purpose,code_hash) VALUES ($1,$2,$3) ON CONFLICT(user_id,purpose) DO UPDATE SET code_hash=$3,attempts=0,created_at=now(),expires_at=now()+interval '10 minutes'").bind(u.id).bind(purpose).bind(&hash).execute(&mut *tx).await.map_err(ApiError::database)?;
    tx.commit().await.map_err(ApiError::database)?;
    if let Err(error)=crate::mailer::send_text(&reqwest::Client::new(),&config,&from,&u.email,"Your Dymini Panel MFA code",&format!("Your Dymini Panel {purpose} code is {code}. It expires in 10 minutes. Do not share this code.")).await {
        sqlx::query("UPDATE mfa_email_codes SET attempts=5 WHERE user_id=$1 AND purpose=$2 AND code_hash=$3").bind(u.id).bind(purpose).bind(hash).execute(&state.pool).await.map_err(ApiError::database)?;
        return Err(error)
    }
    Ok(())
}
async fn consume_email(
    state: &AppState,
    id: Uuid,
    purpose: &str,
    code: &str,
) -> Result<bool, ApiError> {
    let expected = email_hash(state, id, purpose, code)?;
    let mut tx = state.pool.begin().await.map_err(ApiError::database)?;
    let hash:Option<String>=sqlx::query_scalar("UPDATE mfa_email_codes SET attempts=attempts+1 WHERE user_id=$1 AND purpose=$2 AND attempts<5 AND expires_at>now() RETURNING code_hash").bind(id).bind(purpose).fetch_optional(&mut *tx).await.map_err(ApiError::database)?;
    let matched = hash.is_some_and(|hash| auth::tokens_eq(&hash, &expected));
    if matched {
        sqlx::query("UPDATE mfa_email_codes SET attempts=5 WHERE user_id=$1 AND purpose=$2")
            .bind(id)
            .bind(purpose)
            .execute(&mut *tx)
            .await
            .map_err(ApiError::database)?;
    }
    tx.commit().await.map_err(ApiError::database)?;
    Ok(matched)
}
async fn email_login_send(
    State(state): State<AppState>,
    headers: HeaderMap,
    auth::OptionalPeer(peer): auth::OptionalPeer,
    Json(body): Json<PasskeyVerify>,
) -> Result<StatusCode, ApiError> {
    limit(&state, &headers, peer)?;
    let u = password_user(&state, &body.email, &body.password).await?;
    if !u.email_mfa_enabled {
        return Err(invalid());
    }
    send_email(&state, &u, "login").await?;
    Ok(StatusCode::NO_CONTENT)
}
async fn email_send(
    State(state): State<AppState>,
    auth: AuthUser,
    headers: HeaderMap,
    auth::OptionalPeer(peer): auth::OptionalPeer,
    Json(body): Json<EmailMfaSend>,
) -> Result<StatusCode, ApiError> {
    auth.require_write()?;
    limit(&state, &headers, peer)?;
    let u = user(&state, auth.id).await?;
    if !auth::verify_password(&body.reauth.password, &u.password_hash) {
        return Err(invalid());
    }
    match body.purpose.as_str() {
        "enable" if !u.email_mfa_enabled => reauth(&state, auth.id, &body.reauth).await?,
        "reauth" if u.email_mfa_enabled => (),
        _ => {
            return Err(bad(
                "Use enable for a new email factor, or reauth for an enabled email factor",
            ))
        }
    }
    send_email(&state, &u, &body.purpose).await?;
    Ok(StatusCode::NO_CONTENT)
}
async fn email_enable(
    State(state): State<AppState>,
    auth: AuthUser,
    headers: HeaderMap,
    auth::OptionalPeer(peer): auth::OptionalPeer,
    Json(body): Json<EmailMfaEnable>,
) -> Result<StatusCode, ApiError> {
    auth.require_write()?;
    limit(&state, &headers, peer)?;
    let u = user(&state, auth.id).await?;
    if !auth::verify_password(&body.password, &u.password_hash)
        || !consume_email(&state, auth.id, "enable", &body.code).await?
    {
        return Err(invalid());
    }
    sqlx::query("UPDATE users SET email_mfa_enabled=TRUE WHERE id=$1")
        .bind(auth.id)
        .execute(&state.pool)
        .await
        .map_err(ApiError::database)?;
    Ok(StatusCode::NO_CONTENT)
}
async fn email_disable(
    State(state): State<AppState>,
    auth: AuthUser,
    headers: HeaderMap,
    auth::OptionalPeer(peer): auth::OptionalPeer,
    Json(body): Json<MfaReauth>,
) -> Result<StatusCode, ApiError> {
    auth.require_write()?;
    limit(&state, &headers, peer)?;
    require_current_factor(&body)?;
    reauth(&state, auth.id, &body).await?;
    sqlx::query("UPDATE users SET email_mfa_enabled=FALSE WHERE id=$1")
        .bind(auth.id)
        .execute(&state.pool)
        .await
        .map_err(ApiError::database)?;
    sqlx::query("DELETE FROM mfa_email_codes WHERE user_id=$1")
        .bind(auth.id)
        .execute(&state.pool)
        .await
        .map_err(ApiError::database)?;
    Ok(StatusCode::NO_CONTENT)
}
