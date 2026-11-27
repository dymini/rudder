//! Builder inventory and cache management. A builder may also host workloads.

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, patch, post};
use axum::Json;
use chrono::{DateTime, Duration, Utc};
use dymini_panel_node::ops::{BuildCachePrune, BuildCachePruned, Request};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::FromRow;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::audit;
use crate::auth::{self, AuthUser};
use crate::error::ApiError;
use crate::routes::catalog::{bad, conflict, core_error, is_unique, not_found};
use crate::AppState;

#[derive(Serialize, FromRow, ToSchema)]
pub struct BuilderView {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub max_jobs: i32,
    pub paused: bool,
    pub draining: bool,
    pub cache_limit_bytes: i64,
    pub arch: Option<String>,
    pub buildx: bool,
    pub nixpacks: bool,
    pub version: Option<String>,
    pub last_seen: Option<DateTime<Utc>>,
    #[sqlx(default)]
    pub online: bool,
    pub active_jobs: i64,
    /// Queued jobs explicitly targeting this builder. Auto jobs are not reserved yet.
    pub queued_jobs: i64,
    /// Last reported cache catalog, also available while the builder is offline.
    pub cache_bytes: i64,
}

#[derive(Serialize, ToSchema)]
pub struct BuilderInventory {
    /// Remote servers, including disabled candidates that can be enabled with PATCH.
    pub builders: Vec<BuilderView>,
    /// Unclaimed jobs; auto placement has no builder assignment until it is claimed.
    pub waiting_jobs: Vec<BuilderJob>,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateBuilder {
    pub name: String,
    pub description: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct CreatedBuilder {
    pub server: BuilderView,
    pub join_token: String,
    pub join_command: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Default, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateBuilder {
    pub enabled: Option<bool>,
    pub max_jobs: Option<i32>,
    pub paused: Option<bool>,
    pub draining: Option<bool>,
    pub cache_limit_bytes: Option<i64>,
}

#[derive(Serialize, FromRow, ToSchema)]
pub struct BuilderCacheEntry {
    pub service_id: Uuid,
    pub service_name: String,
    pub cache_key: String,
    pub size_bytes: i64,
    pub last_used_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

#[derive(Default, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct BuilderCacheFilter {
    pub service_id: Option<Uuid>,
    pub key: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct BuilderCachePruned {
    pub reclaimed_bytes: u64,
    pub remaining_bytes: u64,
    pub entries: Vec<BuilderCacheEntry>,
}

#[derive(Serialize, FromRow, ToSchema)]
pub struct BuilderJob {
    pub id: Uuid,
    pub deployment_id: Uuid,
    pub service_id: Uuid,
    pub service_name: String,
    pub builder_id: Option<Uuid>,
    pub status: String,
    pub cancel_requested: bool,
    pub disconnect_retries: i16,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
}

const BUILDERS: &str = "SELECT s.id, s.name, s.description, s.builder_enabled AS enabled,
    s.builder_max_jobs AS max_jobs, s.builder_paused AS paused, s.builder_draining AS draining,
    s.builder_cache_limit_bytes AS cache_limit_bytes, s.arch, s.builder_buildx AS buildx,
    s.builder_nixpacks AS nixpacks, s.agent_version AS version, s.last_heartbeat_at AS last_seen,
    (SELECT count(*) FROM build_queue q WHERE q.builder_id = s.id AND q.status = 'running') AS active_jobs,
    (SELECT count(*) FROM build_queue q JOIN deployments d ON d.id = q.deployment_id
     JOIN services app ON app.id = d.service_id WHERE q.status = 'queued'
       AND app.build_placement = 'builder' AND app.build_builder_id = s.id) AS queued_jobs,
    COALESCE((SELECT sum(c.size_bytes)::bigint FROM build_cache_entries c WHERE c.builder_id = s.id), 0) AS cache_bytes
    FROM servers s WHERE s.kind = 'remote'";
const JOBS: &str = "SELECT q.id, q.deployment_id, d.service_id, s.name AS service_name, q.builder_id,
    q.status, q.cancel_requested, q.disconnect_retries, q.created_at, q.started_at, q.finished_at
    FROM build_queue q JOIN deployments d ON d.id = q.deployment_id JOIN services s ON s.id = d.service_id";

pub fn router() -> axum::Router<AppState> {
    axum::Router::new()
        .route("/api/v1/builders", get(inventory).post(create))
        .route("/api/v1/builders/{id}", patch(update))
        .route(
            "/api/v1/builders/{id}/cache",
            get(cache).delete(clear_cache),
        )
        .route("/api/v1/builders/{id}/cache/prune", post(prune_cache))
        .route("/api/v1/builders/{id}/jobs", get(jobs))
}

async fn builder(state: &AppState, id: Uuid) -> Result<BuilderView, ApiError> {
    let mut view = sqlx::query_as::<_, BuilderView>(&format!("{BUILDERS} AND s.id = $1"))
        .bind(id)
        .fetch_optional(&state.pool)
        .await
        .map_err(ApiError::database)?
        .ok_or_else(|| {
            not_found("remote server not found; builders must be enrolled remote servers")
        })?;
    view.online = state.agents.session(id).is_some();
    Ok(view)
}

async fn inventory(
    State(state): State<AppState>,
    _auth: AuthUser,
) -> Result<Json<BuilderInventory>, ApiError> {
    let mut builders =
        sqlx::query_as::<_, BuilderView>(&format!("{BUILDERS} ORDER BY s.name, s.id"))
            .fetch_all(&state.pool)
            .await
            .map_err(ApiError::database)?;
    for view in &mut builders {
        view.online = state.agents.session(view.id).is_some();
    }
    let waiting_jobs = sqlx::query_as(&format!("{JOBS} WHERE q.status = 'queued' AND s.build_placement <> 'panel' ORDER BY q.created_at, q.id LIMIT 200"))
        .fetch_all(&state.pool).await.map_err(ApiError::database)?;
    Ok(Json(BuilderInventory {
        builders,
        waiting_jobs,
    }))
}

async fn create(
    State(state): State<AppState>,
    auth: AuthUser,
    headers: HeaderMap,
    Json(body): Json<CreateBuilder>,
) -> Result<(StatusCode, Json<CreatedBuilder>), ApiError> {
    auth.require_write()?;
    dymini_panel_core::validate_name(&body.name).map_err(core_error)?;
    let description = body.description.unwrap_or_default();
    if description.len() > 4096 {
        return Err(bad(
            "invalid_description",
            "description must be at most 4096 UTF-8 bytes",
        ));
    }
    let token = auth::random_token("join");
    let command = format!(
        "{} --builder --no-edge",
        super::agents::join_command(&state, &headers, &token).await?
    );
    let expires_at = Utc::now() + Duration::hours(1);
    let inserted = sqlx::query_scalar::<_, Uuid>("INSERT INTO servers (name, kind, status, description, builder_enabled, join_token_hash, join_token_expires_at) VALUES ($1, 'remote', 'pending', $2, TRUE, $3, $4) RETURNING id")
        .bind(body.name).bind(description).bind(auth::sha256_hex(&token)).bind(expires_at).fetch_one(&state.pool).await;
    let id = match inserted {
        Ok(id) => id,
        Err(error) if is_unique(&error) => {
            return Err(conflict("name_taken", "a server with that name exists"))
        }
        Err(error) => return Err(ApiError::database(error)),
    };
    audit::record(
        &state.pool,
        Some(auth.id),
        "builder.create",
        "server",
        Some(&id.to_string()),
        json!({}),
        None,
    )
    .await;
    Ok((
        StatusCode::CREATED,
        Json(CreatedBuilder {
            server: builder(&state, id).await?,
            join_token: token,
            join_command: command,
            expires_at,
        }),
    ))
}

async fn update(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateBuilder>,
) -> Result<Json<BuilderView>, ApiError> {
    auth.require_write()?;
    builder(&state, id).await?;
    if body.max_jobs.is_some_and(|jobs| !(1..=32).contains(&jobs)) {
        return Err(bad(
            "invalid_max_jobs",
            "max_jobs must be an integer from 1 to 32",
        ));
    }
    if body.cache_limit_bytes.is_some_and(|bytes| bytes < 0) {
        return Err(bad(
            "invalid_cache_limit",
            "cache_limit_bytes must be a non-negative integer number of bytes",
        ));
    }
    sqlx::query("UPDATE servers SET builder_enabled = COALESCE($2, builder_enabled), builder_max_jobs = COALESCE($3, builder_max_jobs), builder_paused = COALESCE($4, builder_paused), builder_draining = COALESCE($5, builder_draining), builder_cache_limit_bytes = COALESCE($6, builder_cache_limit_bytes), updated_at = now() WHERE id = $1")
        .bind(id).bind(body.enabled).bind(body.max_jobs).bind(body.paused).bind(body.draining).bind(body.cache_limit_bytes).execute(&state.pool).await.map_err(ApiError::database)?;
    audit::record(&state.pool, Some(auth.id), "builder.update", "server", Some(&id.to_string()), json!({ "enabled": body.enabled, "max_jobs": body.max_jobs, "paused": body.paused, "draining": body.draining, "cache_limit_bytes": body.cache_limit_bytes }), None).await;
    Ok(Json(builder(&state, id).await?))
}

async fn cache_entries(state: &AppState, id: Uuid) -> Result<Vec<BuilderCacheEntry>, ApiError> {
    sqlx::query_as("SELECT c.service_id, s.name AS service_name, c.cache_key, c.size_bytes, c.last_used_at, c.created_at FROM build_cache_entries c JOIN services s ON s.id = c.service_id WHERE c.builder_id = $1 ORDER BY s.name, c.service_id, c.cache_key")
        .bind(id).fetch_all(&state.pool).await.map_err(ApiError::database)
}

async fn cache(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<BuilderCacheEntry>>, ApiError> {
    builder(&state, id).await?;
    Ok(Json(cache_entries(&state, id).await?))
}

async fn clear_cache(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Query(filter): Query<BuilderCacheFilter>,
) -> Result<Json<BuilderCachePruned>, ApiError> {
    auth.require_write()?;
    if let Some(key) = &filter.key {
        if key.is_empty()
            || key.len() > 128
            || !key
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-'))
        {
            return Err(bad(
                "invalid_cache_key",
                "key must contain 1 to 128 ASCII letters, digits, underscores or hyphens",
            ));
        }
    }
    mutate_cache(
        &state,
        &auth,
        id,
        BuildCachePrune {
            service_id: filter.service_id.map(|id| id.to_string()),
            cache_key: filter.key,
            limit_bytes: None,
        },
    )
    .await
    .map(Json)
}

async fn prune_cache(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<BuilderCachePruned>, ApiError> {
    auth.require_write()?;
    let view = builder(&state, id).await?;
    mutate_cache(
        &state,
        &auth,
        id,
        BuildCachePrune {
            limit_bytes: Some(view.cache_limit_bytes as u64),
            ..Default::default()
        },
    )
    .await
    .map(Json)
}

async fn mutate_cache(
    state: &AppState,
    auth: &AuthUser,
    id: Uuid,
    request: BuildCachePrune,
) -> Result<BuilderCachePruned, ApiError> {
    builder(state, id).await?;
    let session = state.agents.session(id).ok_or_else(|| {
        conflict(
            "builder_offline",
            "connect the builder before clearing or pruning its cache",
        )
    })?;
    let mut tx = state.pool.begin().await.map_err(ApiError::database)?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(format!("build-cache:{id}"))
        .execute(&mut *tx)
        .await
        .map_err(ApiError::database)?;
    let result: BuildCachePruned = session
        .call(Request::BuildCachePrune { request })
        .await
        .map_err(core_error)?;
    crate::build_queue::remote::persist_catalog(&mut tx, id, result.entries).await
        .map_err(|_| ApiError::new(StatusCode::BAD_GATEWAY, "invalid_cache_inventory", "builder cache changed but its returned inventory could not be stored; reconnect or run prune again"))?;
    tx.commit().await.map_err(ApiError::database)?;
    audit::record(
        &state.pool,
        Some(auth.id),
        "builder.cache_prune",
        "server",
        Some(&id.to_string()),
        json!({"reclaimed_bytes": result.reclaimed_bytes}),
        None,
    )
    .await;
    Ok(BuilderCachePruned {
        reclaimed_bytes: result.reclaimed_bytes,
        remaining_bytes: result.remaining_bytes,
        entries: cache_entries(state, id).await?,
    })
}

async fn jobs(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<BuilderJob>>, ApiError> {
    builder(&state, id).await?;
    let rows = sqlx::query_as(&format!("{JOBS} WHERE q.builder_id = $1 OR (q.status = 'queued' AND s.build_placement = 'builder' AND s.build_builder_id = $1) ORDER BY (q.status = 'running') DESC, q.created_at DESC, q.id LIMIT 100"))
        .bind(id).fetch_all(&state.pool).await.map_err(ApiError::database)?;
    Ok(Json(rows))
}
