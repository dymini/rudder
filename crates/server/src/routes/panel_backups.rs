use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use futures_util::StreamExt;
use object_store::ObjectStoreExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::Row;
use utoipa::ToSchema;
use uuid::Uuid;

use super::catalog::{bad, conflict, core_error, not_found};
use crate::{audit, auth::AuthUser, backup, error::ApiError, s3, state::AppState};

#[derive(Serialize, Deserialize, ToSchema)]
pub struct PanelBackupHealth {
    pub configured: bool,
    pub last_success: Option<DateTime<Utc>>,
    pub age_seconds: Option<i64>,
    pub failing_streak: i64,
    pub stale: bool,
    pub warning: Option<String>,
}

#[derive(Serialize, ToSchema, sqlx::FromRow)]
pub struct PanelBackupConfig {
    pub id: Uuid,
    pub name: String,
    pub target_kind: String,
    pub schedule_cron: Option<String>,
    pub timezone: String,
    pub enabled: bool,
    pub destination_ids: Vec<Uuid>,
    pub retention: Value,
    pub encrypted: bool,
    pub includes_history: bool,
}

#[derive(Serialize, ToSchema, sqlx::FromRow)]
pub struct PanelBackupVerification {
    pub status: String,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub manifest: Option<Value>,
    pub error: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct PanelBackupObject {
    pub run_id: Uuid,
    pub job_id: Uuid,
    pub destination_id: Uuid,
    pub key: String,
    pub size_bytes: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub encrypted: bool,
    pub version: Option<String>,
    pub manifest: Option<Value>,
    pub presence: String,
    pub verification: Option<PanelBackupVerification>,
}

#[derive(Serialize, ToSchema)]
pub struct PanelBackupInventory {
    pub health: PanelBackupHealth,
    pub jobs: Vec<PanelBackupConfig>,
    pub objects: Vec<PanelBackupObject>,
}

#[derive(Deserialize, Default)]
pub struct InventoryQuery {
    pub limit: Option<i64>,
}
#[derive(Deserialize, ToSchema)]
pub struct VerifyPanelBackup {
    pub destination_id: Uuid,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/system/panel-backups", get(inventory))
        .route("/api/v1/system/panel-backups/health", get(health_route))
        .route("/api/v1/system/panel-backups/{id}/verify", post(verify))
        .route(
            "/api/v1/system/panel-backups/{id}/copies/{destination}",
            axum::routing::delete(delete_copy),
        )
        .route(
            "/api/v1/system/panel-backups/{id}/copies/{destination}/download",
            get(download_copy),
        )
}

async fn copy_source(
    state: &AppState,
    run: Uuid,
    destination: Uuid,
) -> Result<backup::SourceRun, ApiError> {
    let source = backup::load_source_run(state, run).await?;
    if !matches!(
        source.meta.target_kind.as_str(),
        "panel_db" | "panel_settings"
    ) || !source.destination_ids.contains(&destination)
    {
        return Err(not_found("panel backup copy not found"));
    }
    Ok(source)
}

async fn download_copy(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((run, destination)): Path<(Uuid, Uuid)>,
) -> Result<axum::response::Response, ApiError> {
    auth.require_admin()?;
    let source = copy_source(&state, run, destination).await?;
    let store = s3::load_destination(&state, destination).await?;
    let stream = s3::download(store.store, &source.key)
        .await
        .map_err(core_error)?;
    audit::record(
        &state.pool,
        Some(auth.id),
        "panel_backup.download",
        "backup_run",
        Some(&run.to_string()),
        json!({"destination_id":destination}),
        None,
    )
    .await;
    let filename: String = source
        .key
        .rsplit('/')
        .next()
        .unwrap_or("backup.enc")
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "-_.".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    axum::http::Response::builder()
        .header("content-type", "application/octet-stream")
        .header(
            "content-disposition",
            format!("attachment; filename=\"{filename}\""),
        )
        .body(axum::body::Body::from_stream(
            stream.map(|c| c.map_err(std::io::Error::other)),
        ))
        .map_err(|_| bad("invalid", "could not build the download"))
}

async fn delete_copy(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((run, destination)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    auth.require_admin()?;
    auth.require_write()?;
    let mut tx = state.pool.begin().await.map_err(ApiError::database)?;
    let acquired: bool = sqlx::query_scalar("SELECT pg_try_advisory_xact_lock(782644926)")
        .fetch_one(&mut *tx)
        .await
        .map_err(ApiError::database)?;
    if !acquired {
        return Err(conflict(
            "verification_busy",
            "wait for panel backup verification before deleting a copy",
        ));
    }
    let source = copy_source(&state, run, destination).await?;
    let store = s3::load_destination(&state, destination).await?;
    s3::delete(store.store.as_ref(), &source.key)
        .await
        .map_err(|_| {
            conflict(
                "delete_failed",
                "the remote backup copy could not be deleted",
            )
        })?;
    sqlx::query("UPDATE backup_runs SET destination_ids=array_remove(destination_ids,$2),status=CASE WHEN cardinality(array_remove(destination_ids,$2))=0 THEN 'expired' ELSE status END WHERE id=$1")
        .bind(run).bind(destination).execute(&mut *tx).await.map_err(ApiError::database)?;
    sqlx::query("DELETE FROM panel_backup_verifications WHERE run_id=$1 AND destination_id=$2")
        .bind(run)
        .bind(destination)
        .execute(&mut *tx)
        .await
        .map_err(ApiError::database)?;
    tx.commit().await.map_err(ApiError::database)?;
    audit::record(
        &state.pool,
        Some(auth.id),
        "panel_backup.delete",
        "backup_run",
        Some(&run.to_string()),
        json!({"destination_id":destination}),
        None,
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

pub fn stale_after(cron: &str, timezone: &str, anchor: DateTime<Utc>, now: DateTime<Utc>) -> bool {
    rudder_core::next_after(cron, timezone, anchor)
        .and_then(|first| rudder_core::next_after(cron, timezone, first))
        .map_or(true, |deadline| now > deadline)
}

pub async fn health(state: &AppState) -> Result<PanelBackupHealth, ApiError> {
    let rows = sqlx::query("SELECT j.id,j.schedule_cron,j.timezone,j.created_at,j.enabled, (SELECT max(finished_at) FROM backup_runs WHERE job_id=j.id AND kind='backup' AND status='succeeded' AND meta->>'target_kind'='panel_db') AS last_success, (SELECT count(*) FROM backup_runs r WHERE r.job_id=j.id AND r.kind='backup' AND r.status IN ('failed','partial') AND r.created_at > COALESCE((SELECT max(created_at) FROM backup_runs s WHERE s.job_id=j.id AND s.kind='backup' AND s.status='succeeded' AND s.meta->>'target_kind'='panel_db'),'-infinity')) AS streak FROM backup_jobs j WHERE j.target_kind='panel_db'")
        .fetch_all(&state.pool).await.map_err(ApiError::database)?;
    let now = Utc::now();
    let mut configured = false;
    let mut stale = false;
    let mut last: Option<DateTime<Utc>> = None;
    let mut streak = 0;
    for row in rows {
        let success: Option<DateTime<Utc>> =
            row.try_get("last_success").map_err(ApiError::database)?;
        last = last.max(success);
        let enabled: bool = row.try_get("enabled").map_err(ApiError::database)?;
        let cron: Option<String> = row.try_get("schedule_cron").map_err(ApiError::database)?;
        if let Some(cron) = cron.filter(|c| enabled && !c.is_empty()) {
            configured = true;
            let created: DateTime<Utc> = row.try_get("created_at").map_err(ApiError::database)?;
            let zone: String = row.try_get("timezone").map_err(ApiError::database)?;
            stale |= stale_after(&cron, &zone, success.unwrap_or(created), now);
            streak = streak.max(
                row.try_get::<i64, _>("streak")
                    .map_err(ApiError::database)?,
            );
        }
    }
    let warning = if !configured {
        Some("No enabled scheduled full-panel backup is configured.")
    } else if stale {
        Some("A full-panel backup is overdue by two scheduled occurrences.")
    } else if streak > 0 {
        Some("A scheduled full-panel backup is failing.")
    } else {
        None
    };
    Ok(PanelBackupHealth {
        configured,
        last_success: last,
        age_seconds: last.map(|d| (now - d).num_seconds().max(0)),
        failing_streak: streak,
        stale,
        warning: warning.map(str::to_owned),
    })
}

async fn health_route(
    State(state): State<AppState>,
    _auth: AuthUser,
) -> Result<Json<PanelBackupHealth>, ApiError> {
    Ok(Json(health(&state).await?))
}

async fn inventory(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(query): Query<InventoryQuery>,
) -> Result<Json<PanelBackupInventory>, ApiError> {
    auth.require_admin()?;
    let limit = query.limit.unwrap_or(50);
    if !(1..=200).contains(&limit) {
        return Err(bad("invalid", "limit must be between 1 and 200 runs"));
    }
    let jobs: Vec<PanelBackupConfig> = sqlx::query_as("SELECT id,name,target_kind,schedule_cron,timezone,enabled,destination_ids,retention_spec AS retention,encryption_passphrase_enc IS NOT NULL AS encrypted,COALESCE(target_spec->'include_history'='true'::jsonb,false) AS includes_history FROM backup_jobs WHERE target_kind IN ('panel_db','panel_settings') ORDER BY created_at DESC")
        .fetch_all(&state.pool).await.map_err(ApiError::database)?;
    let rows = sqlx::query("SELECT r.id,r.job_id,r.object_keys,r.destination_ids,r.size_bytes,r.finished_at,r.meta FROM backup_runs r WHERE r.kind='backup' AND r.status IN ('succeeded','partial') AND r.meta->>'target_kind' IN ('panel_db','panel_settings') ORDER BY r.created_at DESC LIMIT $1")
        .bind(limit).fetch_all(&state.pool).await.map_err(ApiError::database)?;
    let mut objects = Vec::new();
    for row in rows {
        let run_id: Uuid = row.try_get("id").map_err(ApiError::database)?;
        let meta: Value = row.try_get("meta").map_err(ApiError::database)?;
        let keys: Vec<String> = row.try_get("object_keys").map_err(ApiError::database)?;
        let ids: Vec<Uuid> = row.try_get("destination_ids").map_err(ApiError::database)?;
        for destination_id in ids {
            for key in &keys {
                let mut verification: Option<PanelBackupVerification> = sqlx::query_as("SELECT status,started_at,finished_at,manifest,error FROM panel_backup_verifications WHERE run_id=$1 AND destination_id=$2")
                    .bind(run_id).bind(destination_id).fetch_optional(&state.pool).await.map_err(ApiError::database)?;
                if let Some(v) = verification.as_mut().filter(|v| {
                    v.status == "running"
                        && Utc::now() - v.started_at > chrono::Duration::minutes(15)
                }) {
                    v.status = "failed".into();
                    v.error = Some("Verification was interrupted; run it again.".into());
                }
                let manifest = verification
                    .as_ref()
                    .and_then(|v| v.manifest.clone())
                    .or_else(|| meta.get("panel_manifest").filter(|m| !m.is_null()).cloned());
                objects.push(PanelBackupObject {
                    run_id,
                    job_id: row.try_get("job_id").map_err(ApiError::database)?,
                    destination_id,
                    key: key.clone(),
                    size_bytes: row.try_get("size_bytes").map_err(ApiError::database)?,
                    created_at: row.try_get("finished_at").map_err(ApiError::database)?,
                    encrypted: meta["encrypted"].as_bool().unwrap_or(false),
                    version: manifest
                        .as_ref()
                        .and_then(|m| m["rudder_version"].as_str())
                        .map(str::to_owned),
                    manifest,
                    presence: "unavailable".into(),
                    verification,
                });
            }
        }
    }
    let objects = futures_util::stream::iter(objects)
        .map(|mut object| {
            let state = state.clone();
            async move {
                if let Ok(destination) = s3::load_destination(&state, object.destination_id).await {
                    match tokio::time::timeout(
                        std::time::Duration::from_secs(5),
                        destination
                            .store
                            .head(&object_store::path::Path::from(object.key.clone())),
                    )
                    .await
                    {
                        Ok(Ok(meta)) => {
                            object.presence = "present".into();
                            object.size_bytes = i64::try_from(meta.size).ok();
                            object.created_at = meta.last_modified;
                        }
                        Ok(Err(object_store::Error::NotFound { .. })) => {
                            object.presence = "missing".into()
                        }
                        _ => {}
                    }
                }
                object
            }
        })
        .buffered(8)
        .collect()
        .await;
    Ok(Json(PanelBackupInventory {
        health: health(&state).await?,
        jobs,
        objects,
    }))
}

async fn verify(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(run): Path<Uuid>,
    Json(body): Json<VerifyPanelBackup>,
) -> Result<(StatusCode, Json<PanelBackupVerification>), ApiError> {
    auth.require_admin()?;
    auth.require_write()?;
    let source = backup::load_source_run(&state, run).await?;
    if !matches!(
        source.meta.target_kind.as_str(),
        "panel_db" | "panel_settings"
    ) || !source.destination_ids.contains(&body.destination_id)
    {
        return Err(not_found("panel backup copy not found"));
    }
    let job = backup::load_job(&state, source.job_id).await?;
    let expected: Option<String> =
        sqlx::query_scalar("SELECT checksum FROM backup_runs WHERE id=$1")
            .bind(run)
            .fetch_one(&state.pool)
            .await
            .map_err(ApiError::database)?;
    let checksum = expected.filter(|c| c.len() == 64).ok_or_else(|| {
        conflict(
            "missing_checksum",
            "this run has no valid stored SHA-256 checksum",
        )
    })?;
    if source.meta.encrypted && job.passphrase.is_none() {
        return Err(conflict(
            "missing_passphrase",
            "the job no longer has its backup passphrase",
        ));
    }
    if source.meta.target_kind == "panel_db" && !source.meta.encrypted {
        return Err(conflict(
            "unencrypted_panel_backup",
            "full-panel backups must be encrypted",
        ));
    }
    let destination = s3::load_destination(&state, body.destination_id).await?;
    let compression =
        rudder_core::Compression::parse(&source.meta.compression).map_err(core_error)?;
    let mut lock = state.pool.begin().await.map_err(ApiError::database)?;
    let acquired: bool = sqlx::query_scalar("SELECT pg_try_advisory_xact_lock(782644926)")
        .fetch_one(&mut *lock)
        .await
        .map_err(ApiError::database)?;
    if !acquired {
        return Err(conflict(
            "verification_busy",
            "a panel backup verification is already running",
        ));
    }
    let verification: PanelBackupVerification = sqlx::query_as("INSERT INTO panel_backup_verifications(run_id,destination_id,status) VALUES($1,$2,'running') ON CONFLICT(run_id,destination_id) DO UPDATE SET status='running',started_at=now(),finished_at=NULL,manifest=NULL,error=NULL RETURNING status,started_at,finished_at,manifest,error")
        .bind(run).bind(body.destination_id).fetch_one(&state.pool).await.map_err(ApiError::database)?;
    audit::record(
        &state.pool,
        Some(auth.id),
        "panel_backup.verify",
        "backup_run",
        Some(&run.to_string()),
        json!({"destination_id":body.destination_id}),
        None,
    )
    .await;
    tokio::spawn(async move {
        let result = tokio::time::timeout(std::time::Duration::from_secs(600), async {
            let stream = s3::download(destination.store, &source.key).await?;
            crate::panel::verify_backup(
                stream,
                &checksum,
                compression,
                if source.meta.encrypted {
                    job.passphrase.as_deref()
                } else {
                    None
                },
                source.meta.target_kind == "panel_settings",
                &state.config,
            )
            .await
            .map_err(|error| {
                let text = error.to_string().to_lowercase();
                rudder_core::Error::Validation(
                    if text.contains("checksum") {
                        "Stored SHA-256 does not match the downloaded object."
                    } else if text.contains("pg_restore") {
                        "pg_restore --list failed; check PostgreSQL client compatibility."
                    } else if text.contains("crypto") {
                        "Decryption/authentication failed; check the saved passphrase."
                    } else if text.contains("limit") || text.contains("exceeds") {
                        "The verification size limit was exceeded."
                    } else {
                        "The archive, manifest, master key or settings export is invalid."
                    }
                    .into(),
                )
            })
        })
        .await;
        let (status, manifest, error) = match result {
            Ok(Ok(manifest)) => ("succeeded", Some(manifest), None),
            Ok(Err(rudder_core::Error::Validation(message))) => ("failed", None, Some(message)),
            Ok(Err(_)) => (
                "failed",
                None,
                Some("The backup object could not be downloaded.".into()),
            ),
            Err(_) => (
                "failed",
                None,
                Some("Verification timed out after ten minutes.".into()),
            ),
        };
        let _ = sqlx::query("UPDATE panel_backup_verifications SET status=$3,finished_at=now(),manifest=$4,error=$5 WHERE run_id=$1 AND destination_id=$2")
            .bind(run).bind(body.destination_id).bind(status).bind(manifest).bind(&error).execute(&state.pool).await;
        if error.is_some() {
            crate::events::record(
                &state.pool,
                crate::events::Event::warning(
                    "backup.panel_verify_failed",
                    "Panel backup verification failed",
                )
                .payload(json!({"run_id":run,"destination_id":body.destination_id})),
            )
            .await;
        }
        let _ = lock.commit().await;
    });
    Ok((StatusCode::ACCEPTED, Json(verification)))
}

pub async fn monitor(state: &AppState) -> Result<(), sqlx::Error> {
    let health = health(state)
        .await
        .map_err(|e| sqlx::Error::Protocol(e.public_message()))?;
    let mut tx = state.pool.begin().await?;
    let row =
        sqlx::query("SELECT warning,alerted_at FROM panel_backup_health_state WHERE id FOR UPDATE")
            .fetch_one(&mut *tx)
            .await?;
    let old: Option<String> = row.try_get("warning")?;
    let at: Option<DateTime<Utc>> = row.try_get("alerted_at")?;
    let notify = health.warning.is_some()
        && (old != health.warning
            || at.is_none_or(|at| Utc::now() - at > chrono::Duration::hours(24)));
    if notify {
        sqlx::query("INSERT INTO system_events(kind,severity,title,message,payload) VALUES('backup.panel_health','warning','Panel backup needs attention',$1,'{}')")
            .bind(&health.warning).execute(&mut *tx).await?;
    }
    sqlx::query("UPDATE panel_backup_health_state SET warning=$1,alerted_at=CASE WHEN $2 THEN now() ELSE alerted_at END WHERE id")
        .bind(health.warning).bind(notify).execute(&mut *tx).await?;
    tx.commit().await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[sqlx::test(migrations = "./migrations")]
    async fn health_distinguishes_settings_exports_and_alerts_are_deduplicated(pool: sqlx::PgPool) {
        let config = crate::Config::from_map(
            &std::collections::HashMap::from([
                (
                    "DATABASE_URL".into(),
                    "postgres://rudder:rudder@localhost:5432/rudder".into(),
                ),
                ("RUDDER_MASTER_KEY".into(), "ab".repeat(32)),
            ]),
            |_| unreachable!(),
        )
        .unwrap();
        let state = AppState::new(pool.clone(), config);
        assert!(!health(&state).await.unwrap().configured);
        monitor(&state).await.unwrap();
        monitor(&state).await.unwrap();
        let events: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM system_events WHERE kind='backup.panel_health'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(events, 1);
        sqlx::query("INSERT INTO backup_jobs(name,target_kind,schedule_cron) VALUES('settings','panel_settings','@daily')").execute(&pool).await.unwrap();
        assert!(!health(&state).await.unwrap().configured);
        let job: Uuid=sqlx::query_scalar("INSERT INTO backup_jobs(name,target_kind,schedule_cron,created_at) VALUES('daily','panel_db','@daily',now()-interval '4 days') RETURNING id").fetch_one(&pool).await.unwrap();
        assert!(health(&state).await.unwrap().stale);
        for _ in 0..2 {
            sqlx::query("INSERT INTO backup_runs(job_id,status,kind,trigger,finished_at) VALUES($1,'failed','backup','test',now())").bind(job).execute(&pool).await.unwrap();
        }
        assert_eq!(health(&state).await.unwrap().failing_streak, 2);
        sqlx::query("INSERT INTO backup_runs(job_id,status,kind,trigger,finished_at,meta) VALUES($1,'succeeded','backup','test',now(),'{\"target_kind\":\"panel_db\"}')").bind(job).execute(&pool).await.unwrap();
        let result = health(&state).await.unwrap();
        assert!(result.configured);
        assert!(!result.stale);
        assert_eq!(result.failing_streak, 0);
        assert!(result.last_success.is_some());
        assert!(result.warning.is_none());
        monitor(&state).await.unwrap();
        let warning: Option<String> =
            sqlx::query_scalar("SELECT warning FROM panel_backup_health_state WHERE id")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(warning.is_none());
    }
    #[test]
    fn freshness_uses_two_calendar_occurrences_and_fails_closed() {
        let at = |s: &str| DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc);
        let anchor = at("2026-01-01T00:00:00Z");
        assert!(!stale_after(
            "0 0 * * *",
            "UTC",
            anchor,
            at("2026-01-02T23:59:59Z")
        ));
        assert!(stale_after(
            "0 0 * * *",
            "UTC",
            anchor,
            at("2026-01-03T00:00:01Z")
        ));
        assert!(!stale_after(
            "0 0 31 * *",
            "UTC",
            at("2026-01-31T00:00:00Z"),
            at("2026-05-30T23:59:59Z")
        ));
        assert!(stale_after(
            "0 0 31 * *",
            "UTC",
            at("2026-01-31T00:00:00Z"),
            at("2026-05-31T00:00:01Z")
        ));
        assert!(stale_after("invalid", "UTC", anchor, anchor));
        assert!(stale_after("@daily", "invalid", anchor, anchor));
        let anchor = at("2026-03-28T00:00:00Z");
        let first = rudder_core::next_after("0 9 * * *", "Europe/Berlin", anchor).unwrap();
        let second = rudder_core::next_after("0 9 * * *", "Europe/Berlin", first).unwrap();
        assert!(!stale_after("0 9 * * *", "Europe/Berlin", anchor, second));
        assert!(stale_after(
            "0 9 * * *",
            "Europe/Berlin",
            anchor,
            second + chrono::Duration::seconds(1)
        ));
    }
}
