use axum::{
    extract::{Query, State},
    Json,
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    auth::AuthUser,
    error::ApiError,
    logs::retention::{self, MetricsRetention},
    state::AppState,
};

#[derive(Deserialize, Default)]
pub struct StatsQuery {
    pub range: Option<String>,
}

#[derive(Deserialize, Serialize, ToSchema)]
pub struct TrafficStats {
    pub rx_bytes: Option<f64>,
    pub tx_bytes: Option<f64>,
    pub requests: f64,
    pub http_5xx: f64,
    pub http_5xx_rate: f64,
    pub cpu_avg: Option<f64>,
    pub cpu_peak: Option<f64>,
    pub memory_avg: Option<f64>,
    pub memory_peak: Option<f64>,
}
#[derive(Deserialize, Serialize, ToSchema)]
pub struct StatsPoint {
    pub at: DateTime<Utc>,
    pub rx_bytes: Option<f64>,
    pub tx_bytes: Option<f64>,
    pub requests: f64,
    pub http_5xx: f64,
    pub http_5xx_rate: f64,
    pub cpu_avg: Option<f64>,
    pub cpu_peak: Option<f64>,
    pub memory_avg: Option<f64>,
    pub memory_peak: Option<f64>,
}
#[derive(Deserialize, Serialize, ToSchema)]
pub struct TopService {
    pub service_id: Uuid,
    pub name: String,
    pub server_id: Uuid,
    pub network_bytes: Option<f64>,
    pub requests: Option<f64>,
    pub http_5xx: Option<f64>,
    pub cpu_percent: Option<f64>,
    pub memory_bytes: Option<f64>,
}
#[derive(Deserialize, Serialize, ToSchema)]
pub struct ServerDisk {
    pub server_id: Uuid,
    pub name: String,
    pub disk_percent: Option<f64>,
    pub disk_used_bytes: Option<f64>,
    pub sampled_at: Option<DateTime<Utc>>,
}
#[derive(Deserialize, Serialize, ToSchema)]
pub struct StatsMetrics {
    pub totals: TrafficStats,
    pub series: Vec<StatsPoint>,
    pub top_network: Vec<TopService>,
    pub top_requests: Vec<TopService>,
    pub top_5xx: Vec<TopService>,
    pub top_cpu: Vec<TopService>,
    pub top_memory: Vec<TopService>,
    /// Observed positive changes in sampled service restart counters, not an exact event count.
    pub container_restarts: f64,
    pub disks: Vec<ServerDisk>,
}
#[derive(Deserialize, Serialize, ToSchema)]
pub struct StatsCounts {
    pub servers: i64,
    pub services: i64,
    pub domains: i64,
    pub plugins: i64,
}
#[derive(Deserialize, Serialize, ToSchema)]
pub struct JobStats {
    pub job_id: Uuid,
    pub name: String,
    pub runs: i64,
    pub failures: i64,
    pub last_success: Option<DateTime<Utc>>,
}
#[derive(Deserialize, Serialize, ToSchema)]
pub struct StatsOperations {
    pub deploys: i64,
    pub deploy_success_rate: Option<f64>,
    pub deploy_avg_duration_seconds: Option<f64>,
    pub backup_runs: i64,
    pub backup_failures: i64,
    pub backup_jobs: Vec<JobStats>,
    pub alerts_fired: i64,
    pub mcp_calls: i64,
    pub mcp_error_rate: f64,
    pub mcp_denied: i64,
    pub counts: StatsCounts,
}
#[derive(Serialize, ToSchema)]
pub struct StatsOverview {
    pub since: DateTime<Utc>,
    pub until: DateTime<Utc>,
    pub resolution: String,
    pub retention: MetricsRetention,
    pub metrics: StatsMetrics,
    pub operations: StatsOperations,
}

pub async fn overview(
    State(state): State<AppState>,
    _auth: AuthUser,
    Query(query): Query<StatsQuery>,
) -> Result<Json<StatsOverview>, ApiError> {
    let (seconds, step, resolution) = match query.range.as_deref().unwrap_or("1h") {
        "1h" => (3600, 60.0, "1m"),
        "24h" => (86400, 300.0, "5m"),
        "7d" => (7 * 86400, 3600.0, "1h"),
        "30d" => (30 * 86400, 3600.0, "1h"),
        _ => {
            return Err(super::catalog::bad(
                "invalid_range",
                "range must be 1h, 24h, 7d, or 30d.",
            ))
        }
    };
    let until = Utc::now();
    let since = until - Duration::seconds(seconds);
    let retention = retention::load(&state.pool)
        .await
        .map_err(ApiError::database)?
        .metrics;
    let mut tx = state.pool.begin().await.map_err(ApiError::database)?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
        .execute(&mut *tx)
        .await
        .map_err(ApiError::database)?;
    let sqlx::types::Json(metrics) =
        sqlx::query_scalar::<_, sqlx::types::Json<StatsMetrics>>(include_str!("stats_metrics.sql"))
            .bind(since)
            .bind(until)
            .bind(step)
            .bind(resolution)
            .fetch_one(&mut *tx)
            .await
            .map_err(ApiError::database)?;
    let sqlx::types::Json(operations) =
        sqlx::query_scalar::<_, sqlx::types::Json<StatsOperations>>(include_str!(
            "stats_operations.sql"
        ))
        .bind(since)
        .bind(until)
        .fetch_one(&mut *tx)
        .await
        .map_err(ApiError::database)?;
    tx.commit().await.map_err(ApiError::database)?;
    Ok(Json(StatsOverview {
        since,
        until,
        resolution: resolution.into(),
        retention,
        metrics,
        operations,
    }))
}
