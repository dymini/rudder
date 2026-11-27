use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{auth::AuthUser, error::ApiError, state::AppState};

#[derive(Serialize, ToSchema, sqlx::FromRow)]
pub struct McpCall {
    pub id: i64,
    pub grant_id: Option<Uuid>,
    pub client_id: Option<String>,
    pub tool: String,
    pub scope: String,
    pub status: String,
    pub http_status: i32,
    pub duration_ms: i64,
    pub error_message: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Default, Deserialize)]
pub struct UsageQuery {
    pub client_id: Option<String>,
    pub tool: Option<String>,
    pub status: Option<String>,
    pub days: Option<i32>,
    pub bucket: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

impl UsageQuery {
    fn validate(&self) -> Result<(), ApiError> {
        let invalid = |m| ApiError::new(StatusCode::BAD_REQUEST, "invalid_usage_filter", m);
        if !(1..=30).contains(&self.days.unwrap_or(7)) {
            return Err(invalid("days must be between 1 and 30."));
        }
        if !(1..=500).contains(&self.limit.unwrap_or(100)) {
            return Err(invalid("limit must be between 1 and 500."));
        }
        if !(0..=100_000).contains(&self.offset.unwrap_or(0)) {
            return Err(invalid("offset must be between 0 and 100000."));
        }
        if self
            .bucket
            .as_ref()
            .is_some_and(|s| s != "hour" && s != "day")
        {
            return Err(invalid("bucket must be hour or day."));
        }
        if self
            .status
            .as_ref()
            .is_some_and(|s| !["ok", "denied", "error"].contains(&s.as_str()))
        {
            return Err(invalid("status must be ok, denied, or error."));
        }
        if self.tool.as_ref().is_some_and(|s| s.len() > 200)
            || self.client_id.as_ref().is_some_and(|s| s.len() > 200)
        {
            return Err(invalid(
                "Tool and client filters must be at most 200 bytes.",
            ));
        }
        Ok(())
    }
}

pub async fn begin(state: &AppState, user: Option<&AuthUser>, name: &str) -> Result<i64, ApiError> {
    let authority = user.and_then(|u| u.mcp.as_ref());
    begin_for(
        state,
        authority.and_then(|a| a.grant_id),
        authority.and_then(|a| a.client_id.as_deref()),
        name,
    )
    .await
}

pub async fn begin_unauthorized(
    state: &AppState,
    token: Option<&str>,
    name: &str,
) -> Result<i64, ApiError> {
    let actor: Option<(Uuid, String)> = if let Some(token) = token {
        sqlx::query_as("SELECT g.id,g.client_id FROM mcp_access_tokens a JOIN mcp_grants g ON g.id=a.grant_id WHERE a.token_hash=$1").bind(crate::auth::sha256_hex(token)).fetch_optional(&state.pool).await.map_err(ApiError::database)?
    } else {
        None
    };
    begin_for(
        state,
        actor.as_ref().map(|a| a.0),
        actor.as_ref().map(|a| a.1.as_str()),
        name,
    )
    .await
}

async fn begin_for(
    state: &AppState,
    grant: Option<Uuid>,
    client: Option<&str>,
    name: &str,
) -> Result<i64, ApiError> {
    let known = super::tools().into_iter().find(|t| t.name == name);
    let (tool, scope) = match known {
        Some(t) => (t.name, t.scope),
        None if name == "dymini_panel_whoami" => ("dymini_panel_whoami".into(), "self".into()),
        None => ("unknown".into(), "unknown".into()),
    };
    sqlx::query_scalar("INSERT INTO mcp_calls(grant_id,client_id,tool,scope,status,http_status,duration_ms,error_message,finished_at) VALUES ($1,$2,$3,$4,'error',500,0,'Tool dispatch did not finish.',NULL) RETURNING id")
        .bind(grant).bind(client).bind(tool).bind(scope).fetch_one(&state.pool).await.map_err(ApiError::database)
}

pub async fn finish(
    state: &AppState,
    id: i64,
    status: StatusCode,
    failed: bool,
    duration_ms: i64,
) -> Result<(), ApiError> {
    let outcome = if [401, 403].contains(&status.as_u16()) {
        "denied"
    } else if failed || status.is_client_error() || status.is_server_error() {
        "error"
    } else {
        "ok"
    };
    let message = if outcome == "denied" {
        Some("Tool access was denied.")
    } else if outcome == "error" {
        Some("Tool execution failed; inspect the tool response for its reason.")
    } else {
        None
    };
    sqlx::query("UPDATE mcp_calls SET status=$2,http_status=$3,duration_ms=$4,error_message=$5,finished_at=clock_timestamp() WHERE id=$1")
        .bind(id).bind(outcome).bind(i32::from(status.as_u16())).bind(duration_ms).bind(message).execute(&state.pool).await.map_err(ApiError::database)?;
    Ok(())
}

pub async fn maintain(pool: &PgPool) -> Result<(), ApiError> {
    sqlx::query("DELETE FROM mcp_calls WHERE created_at < now() - interval '30 days'")
        .execute(pool)
        .await
        .map_err(ApiError::database)?;
    sqlx::query("UPDATE mcp_calls SET finished_at=clock_timestamp(), duration_ms=GREATEST(0, (extract(epoch FROM (clock_timestamp()-created_at))*1000)::bigint) WHERE finished_at IS NULL AND created_at < now() - interval '15 minutes'").execute(pool).await.map_err(ApiError::database)?;
    Ok(())
}

const FILTER:&str=" WHERE created_at >= now() - make_interval(days => $1) AND finished_at IS NOT NULL AND ($2::text IS NULL OR client_id=$2) AND ($3::text IS NULL OR tool=$3) AND ($4::text IS NULL OR status=$4) ";

pub async fn calls(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(query): Query<UsageQuery>,
) -> Result<Json<Vec<McpCall>>, ApiError> {
    auth.require_admin()?;
    query.validate()?;
    let rows = sqlx::query_as(&format!("SELECT id,grant_id,client_id,tool,scope,status,http_status,duration_ms,error_message,created_at FROM mcp_calls {FILTER} ORDER BY created_at DESC,id DESC LIMIT $5 OFFSET $6"))
        .bind(query.days.unwrap_or(7)).bind(&query.client_id).bind(&query.tool).bind(&query.status).bind(query.limit.unwrap_or(100)).bind(query.offset.unwrap_or(0)).fetch_all(&state.pool).await.map_err(ApiError::database)?;
    Ok(Json(rows))
}

#[derive(Serialize, ToSchema, sqlx::FromRow)]
pub struct McpUsageSummary {
    pub total: i64,
    pub error_rate: f64,
    pub denied_rate: f64,
    pub p50_ms: Option<f64>,
    pub p95_ms: Option<f64>,
}
#[derive(Serialize, ToSchema, sqlx::FromRow)]
pub struct McpUsageBucket {
    pub at: DateTime<Utc>,
    pub count: i64,
    pub ok: i64,
    pub denied: i64,
    pub error: i64,
}
#[derive(Serialize, ToSchema, sqlx::FromRow)]
pub struct McpToolUsage {
    pub tool: String,
    pub count: i64,
}
#[derive(Serialize, ToSchema, sqlx::FromRow)]
pub struct McpClientUsage {
    pub client_id: Option<String>,
    pub count: i64,
}
#[derive(Serialize, ToSchema, sqlx::FromRow)]
pub struct McpGrantUsage {
    pub grant_id: Uuid,
    pub count: i64,
    pub last_used_at: DateTime<Utc>,
}
#[derive(Serialize, ToSchema)]
pub struct McpUsageStats {
    pub summary: McpUsageSummary,
    pub buckets: Vec<McpUsageBucket>,
    pub tools: Vec<McpToolUsage>,
    pub clients: Vec<McpClientUsage>,
    pub grants: Vec<McpGrantUsage>,
}

pub async fn stats(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(query): Query<UsageQuery>,
) -> Result<Json<McpUsageStats>, ApiError> {
    auth.require_admin()?;
    query.validate()?;
    let days = query.days.unwrap_or(7);
    let summary=sqlx::query_as(&format!("SELECT count(*) AS total,coalesce(count(*) FILTER (WHERE status='error')::float8/nullif(count(*),0),0.0) AS error_rate,coalesce(count(*) FILTER (WHERE status='denied')::float8/nullif(count(*),0),0.0) AS denied_rate,percentile_cont(0.5) WITHIN GROUP (ORDER BY duration_ms) AS p50_ms,percentile_cont(0.95) WITHIN GROUP (ORDER BY duration_ms) AS p95_ms FROM mcp_calls {FILTER}"))
        .bind(days).bind(&query.client_id).bind(&query.tool).bind(&query.status).fetch_one(&state.pool).await.map_err(ApiError::database)?;
    let buckets=sqlx::query_as(&format!("WITH filtered AS (SELECT * FROM mcp_calls {FILTER}), series AS (SELECT generate_series(date_trunc($5,now()-make_interval(days=>$1),'UTC'),date_trunc($5,now(),'UTC'),CASE WHEN $5='hour' THEN interval '1 hour' ELSE interval '24 hours' END) AS at) SELECT series.at,count(f.id) AS count,count(f.id) FILTER (WHERE status='ok') AS ok,count(f.id) FILTER (WHERE status='denied') AS denied,count(f.id) FILTER (WHERE status='error') AS error FROM series LEFT JOIN filtered f ON date_trunc($5,f.created_at,'UTC')=series.at GROUP BY series.at ORDER BY series.at"))
        .bind(days).bind(&query.client_id).bind(&query.tool).bind(&query.status).bind(query.bucket.as_deref().unwrap_or("hour")).fetch_all(&state.pool).await.map_err(ApiError::database)?;
    let tools=sqlx::query_as(&format!("SELECT tool,count(*) AS count FROM mcp_calls {FILTER} GROUP BY tool ORDER BY count DESC,tool LIMIT 20"))
        .bind(days).bind(&query.client_id).bind(&query.tool).bind(&query.status).fetch_all(&state.pool).await.map_err(ApiError::database)?;
    let clients=sqlx::query_as(&format!("SELECT client_id,count(*) AS count FROM mcp_calls {FILTER} GROUP BY client_id ORDER BY count DESC,client_id"))
        .bind(days).bind(&query.client_id).bind(&query.tool).bind(&query.status).fetch_all(&state.pool).await.map_err(ApiError::database)?;
    let grants=sqlx::query_as(&format!("SELECT grant_id,count(*) AS count,max(created_at) AS last_used_at FROM mcp_calls {FILTER} AND grant_id IS NOT NULL GROUP BY grant_id ORDER BY last_used_at DESC"))
        .bind(days).bind(&query.client_id).bind(&query.tool).bind(&query.status).fetch_all(&state.pool).await.map_err(ApiError::database)?;
    Ok(Json(McpUsageStats {
        summary,
        buckets,
        tools,
        clients,
        grants,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[sqlx::test(migrations = "./migrations")]
    async fn retention_prunes_old_rows_and_finishes_interrupted_calls(pool: PgPool) {
        sqlx::query("INSERT INTO mcp_calls(tool,scope,status,http_status,duration_ms,created_at,finished_at,error_message) VALUES ('old','self','ok',200,1,now()-interval '31 days',now(),NULL),('interrupted','self','error',500,0,now()-interval '1 hour',NULL,'Tool dispatch did not finish.')").execute(&pool).await.unwrap();
        maintain(&pool).await.unwrap();
        let rows: Vec<(String, bool)> =
            sqlx::query_as("SELECT tool,finished_at IS NOT NULL FROM mcp_calls")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(rows, vec![("interrupted".into(), true)]);
    }
}
