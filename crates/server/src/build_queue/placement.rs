use chrono::{DateTime, Utc};
use sqlx::FromRow;
use uuid::Uuid;

use super::*;

#[derive(FromRow)]
struct Queued {
    id: Uuid,
    deployment_id: Uuid,
    service_id: Uuid,
    created_at: DateTime<Utc>,
    cancel_requested: bool,
    spec: Value,
    build_placement: String,
    build_builder_id: Option<Uuid>,
    build_fallback: String,
    target_kind: String,
    target_arch: Option<String>,
}

#[derive(FromRow)]
struct Candidate {
    id: Uuid,
    name: String,
}

pub(super) async fn claim(state: &AppState) -> Result<Option<(Uuid, Uuid)>, ApiError> {
    let mut tx = state.pool.begin().await.map_err(ApiError::database)?;
    // Capacity counts and reservations must share a transaction across panel processes.
    sqlx::query("SELECT pg_advisory_xact_lock(746321028)")
        .execute(&mut *tx)
        .await
        .map_err(ApiError::database)?;
    let limit: i32 =
        sqlx::query_scalar("SELECT build_concurrency_default FROM panel_settings WHERE id = TRUE")
            .fetch_one(&mut *tx)
            .await
            .map_err(ApiError::database)?;
    let panel_running: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM build_queue WHERE status = 'running' AND builder_id IS NULL",
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(ApiError::database)?;
    let rows = sqlx::query_as::<_, Queued>(
        "SELECT q.id, q.deployment_id, d.service_id, q.created_at, q.cancel_requested,
                s.spec, s.build_placement, s.build_builder_id, s.build_fallback,
                target.kind AS target_kind, target.arch AS target_arch
         FROM build_queue q JOIN deployments d ON d.id = q.deployment_id
         JOIN services s ON s.id = d.service_id JOIN servers target ON target.id = s.server_id
         WHERE q.status = 'queued' ORDER BY q.created_at, q.id
         FOR UPDATE OF q SKIP LOCKED",
    )
    .fetch_all(&mut *tx)
    .await
    .map_err(ApiError::database)?;
    for row in rows {
        if row.cancel_requested {
            sqlx::query(
                "UPDATE build_queue SET status = 'cancelled', finished_at = now() WHERE id = $1",
            )
            .bind(row.id)
            .execute(&mut *tx)
            .await
            .map_err(ApiError::database)?;
            sqlx::query("UPDATE deployments SET status = 'cancelled', error = 'build cancelled', finished_at = now() WHERE id = $1")
                .bind(row.deployment_id).execute(&mut *tx).await.map_err(ApiError::database)?;
            continue;
        }
        let running: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM build_queue q JOIN deployments d ON d.id = q.deployment_id WHERE d.service_id = $1 AND q.status = 'running')"
        ).bind(row.service_id).fetch_one(&mut *tx).await.map_err(ApiError::database)?;
        let waited = (Utc::now() - row.created_at).to_std().unwrap_or_default();
        let hold = state.guard.hold(waited);
        if running || hold == Some("paused") {
            waiting(
                &mut tx,
                row.deployment_id,
                if running {
                    "another build for this service is running"
                } else {
                    "builds are paused"
                },
            )
            .await?;
            continue;
        }
        let arch = if row.target_kind == "local" {
            Some(std::env::consts::ARCH)
        } else {
            row.target_arch.as_deref()
        };
        let nixpacks =
            row.spec.pointer("/git/strategy").and_then(Value::as_str) == Some("nixpacks");
        let candidate = if row.build_placement == "panel" {
            None
        } else {
            sqlx::query_as::<_, Candidate>(
                "SELECT b.id, b.name FROM servers b
                 WHERE b.builder_enabled AND NOT b.builder_paused AND NOT b.builder_draining
                   AND b.id = ANY($1) AND b.builder_buildx AND (NOT $2 OR b.builder_nixpacks)
                   AND b.arch = $3 AND ($4::uuid IS NULL OR b.id = $4)
                   AND (SELECT count(*) FROM build_queue q WHERE q.builder_id = b.id AND q.status = 'running') < b.builder_max_jobs
                 ORDER BY EXISTS(SELECT 1 FROM build_cache_entries c WHERE c.builder_id = b.id AND c.service_id = $5) DESC,
                   (SELECT count(*) FROM build_queue q WHERE q.builder_id = b.id AND q.status = 'running'), b.name, b.id LIMIT 1 FOR UPDATE OF b"
            ).bind(state.agents.connected()).bind(nixpacks).bind(arch)
                .bind(if row.build_placement == "builder" { row.build_builder_id } else { None })
                .bind(row.service_id).fetch_optional(&mut *tx).await.map_err(ApiError::database)?
                .filter(|_| row.build_placement != "builder" || row.build_builder_id.is_some())
        };
        let (builder, reason) = if let Some(candidate) = candidate {
            (Some(candidate.id), format!("Building on {}: {} placement; eligible architecture, capabilities and free slot\n", candidate.name, row.build_placement))
        } else {
            if row.build_placement != "panel" && row.build_fallback == "wait" {
                waiting(&mut tx, row.deployment_id, "no eligible builder; waiting for a connected, compatible builder with a free slot").await?;
                continue;
            }
            if let Some(hold) = hold {
                waiting(
                    &mut tx,
                    row.deployment_id,
                    &format!("panel load guard: {hold}"),
                )
                .await?;
                continue;
            }
            if !can_start(panel_running, i64::from(limit)) {
                waiting(&mut tx, row.deployment_id, "panel build concurrency limit").await?;
                continue;
            }
            (
                None,
                if row.build_placement == "panel" {
                    "Building on panel: panel placement\n".into()
                } else {
                    "Building on panel: no eligible builder; panel fallback\n".into()
                },
            )
        };
        sqlx::query("UPDATE build_queue SET status = 'running', started_at = now(), builder_id = $2 WHERE id = $1")
            .bind(row.id).bind(builder).execute(&mut *tx).await.map_err(ApiError::database)?;
        sqlx::query("UPDATE deployments SET status = 'running', started_at = COALESCE(started_at, now()), error = NULL, logs = COALESCE(logs, '') || $2 WHERE id = $1")
            .bind(row.deployment_id).bind(reason).execute(&mut *tx).await.map_err(ApiError::database)?;
        tx.commit().await.map_err(ApiError::database)?;
        return Ok(Some((row.id, row.deployment_id)));
    }
    tx.commit().await.map_err(ApiError::database)?;
    Ok(None)
}

async fn waiting(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    deployment: Uuid,
    reason: &str,
) -> Result<(), ApiError> {
    let reason = format!("Waiting: {reason}\n");
    sqlx::query("UPDATE deployments SET logs = CASE WHEN right(COALESCE(logs, ''), length($2)) = $2 THEN logs ELSE COALESCE(logs, '') || $2 END WHERE id = $1")
        .bind(deployment).bind(reason).execute(&mut **tx).await.map_err(ApiError::database)?;
    Ok(())
}
