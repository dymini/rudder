//! Connector telemetry and cached upstream versions. Credentials never enter these views.

use std::time::Duration;

use axum::{extract::State, Json};
use chrono::{DateTime, Utc};
use dymini_panel_core::{parse_version, GithubRelease, NodeDriver};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{Postgres, Transaction};
use utoipa::ToSchema;

use crate::{
    auth::AuthUser,
    cloudflare as cf,
    error::ApiError,
    routes::catalog::{bad, conflict},
    state::AppState,
};

#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct TunnelConnection {
    pub connector_id: String,
    pub connection_id: Option<String>,
    pub version: Option<String>,
    pub origin_ip: Option<String>,
    pub colo: Option<String>,
    pub opened_at: Option<String>,
    pub healthy: bool,
}

pub fn parse_connections(value: &Value) -> Vec<TunnelConnection> {
    let list = value.as_array().or_else(|| {
        value
            .as_object()
            .and_then(|map| map.values().find_map(Value::as_array))
    });
    let text = |v: &Value, key: &str| v.get(key).and_then(Value::as_str).map(str::to_owned);
    let mut result = Vec::new();
    for connector in list.into_iter().flatten() {
        if !connector.is_object() {
            continue;
        }
        let conns = connector.get("conns").and_then(Value::as_array);
        for conn in conns
            .map(|v| v.iter().collect::<Vec<_>>())
            .unwrap_or_else(|| vec![connector])
        {
            if !conn.is_object() {
                continue;
            }
            result.push(TunnelConnection {
                connector_id: text(connector, "client_id")
                    .or_else(|| text(connector, "id"))
                    .unwrap_or_default(),
                connection_id: text(conn, "id"),
                version: text(connector, "version").or_else(|| text(connector, "client_version")),
                origin_ip: text(conn, "origin_ip").or_else(|| text(connector, "origin_ip")),
                colo: text(conn, "colo_name").or_else(|| text(conn, "colo")),
                opened_at: text(conn, "opened_at"),
                healthy: conn.get("is_pending_reconnect").and_then(Value::as_bool) != Some(true),
            });
        }
    }
    result
}

#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct CloudflaredRelease {
    pub version: String,
    pub published_at: Option<String>,
    pub html_url: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct TunnelUpdateProgress {
    pub status: String,
    pub version: String,
    pub image: String,
    pub replica: Option<u8>,
    pub completed: u8,
    pub total: u8,
    pub message: Option<String>,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

#[derive(sqlx::FromRow)]
pub(crate) struct Saved {
    pub cloudflared_version: Option<String>,
    pub cloudflared_release: Option<Value>,
    pub cloudflared_checked_at: Option<DateTime<Utc>>,
    pub cloudflared_check_error: Option<String>,
    pub cloudflared_update: Option<Value>,
}

pub(crate) async fn saved(state: &AppState) -> Result<Saved, ApiError> {
    sqlx::query_as("SELECT cloudflared_version, cloudflared_release, cloudflared_checked_at, cloudflared_check_error, cloudflared_update FROM cloudflare_settings WHERE id = TRUE")
        .fetch_one(&state.pool).await.map_err(ApiError::database)
}

/// Serialize tunnel/credential mutations, including across panel processes.
pub(crate) struct TunnelLock {
    _local: tokio::sync::OwnedMutexGuard<()>,
    _database: Transaction<'static, Postgres>,
}

pub(crate) async fn lock(state: &AppState) -> Result<TunnelLock, ApiError> {
    let local = state.cloudflare.tunnel_lock.clone().try_lock_owned()
        .map_err(|_| conflict("tunnel_busy", "A tunnel update or configuration change is already running. Wait until it finishes."))?;
    let mut tx = state.pool.begin().await.map_err(ApiError::database)?;
    let acquired: bool = sqlx::query_scalar("SELECT pg_try_advisory_xact_lock(77302461)")
        .fetch_one(&mut *tx)
        .await
        .map_err(ApiError::database)?;
    if !acquired {
        return Err(conflict(
            "tunnel_busy",
            "A tunnel update or configuration change is already running. Wait until it finishes.",
        ));
    }
    Ok(TunnelLock {
        _local: local,
        _database: tx,
    })
}

fn stable_version(input: &str) -> Option<String> {
    let version = parse_version(input).ok()?;
    (version.pre.is_empty() && version.build.is_empty()).then(|| version.to_string())
}

pub(crate) fn version(input: &str) -> Result<String, ApiError> {
    stable_version(input).ok_or_else(|| bad("invalid_version", "cloudflared version must be a stable three-part version, such as 2026.11.0 (an optional v prefix is allowed)."))
}

pub(crate) fn newer(latest: &str, current: &str) -> bool {
    match (parse_version(latest), parse_version(current)) {
        (Ok(latest), Ok(current)) => latest > current,
        _ => false,
    }
}

fn newest(releases: Vec<GithubRelease>) -> Option<CloudflaredRelease> {
    releases
        .into_iter()
        .filter(|release| !release.draft && !release.prerelease)
        .filter_map(|release| {
            Some((
                parse_version(&stable_version(&release.tag_name)?).ok()?,
                release,
            ))
        })
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|(version, release)| CloudflaredRelease {
            version: version.to_string(),
            published_at: release.published_at,
            html_url: release.html_url,
        })
}

pub(crate) async fn check_release(state: &AppState, force: bool) -> Result<Saved, ApiError> {
    let cached = saved(state).await?;
    if !force
        && cached
            .cloudflared_checked_at
            .is_some_and(|at| (Utc::now() - at).num_seconds() < 6 * 60 * 60)
    {
        return Ok(cached);
    }
    let fetched = async {
        let client = crate::update::http_client()?;
        let bytes = crate::update::fetch(
            &client,
            &format!(
                "{}/repos/cloudflare/cloudflared/releases?per_page=30",
                state.config.github_api_url
            ),
        )
        .await?;
        let releases: Vec<GithubRelease> = serde_json::from_slice(&bytes).map_err(|_| {
            dymini_panel_core::Error::Node(
                "GitHub returned invalid cloudflared release data".into(),
            )
        })?;
        newest(releases).ok_or_else(|| {
            dymini_panel_core::Error::Node("GitHub returned no stable cloudflared release".into())
        })
    }
    .await;
    let (release, error) = match fetched {
        Ok(release) => (
            Some(
                serde_json::to_value(release)
                    .map_err(|_| bad("release", "Could not cache the release"))?,
            ),
            None,
        ),
        Err(error) => (None, Some(error.to_string())),
    };
    sqlx::query("UPDATE cloudflare_settings SET cloudflared_release = COALESCE($1, cloudflared_release), cloudflared_checked_at = now(), cloudflared_check_error = $2 WHERE id = TRUE")
        .bind(release).bind(error).execute(&state.pool).await.map_err(ApiError::database)?;
    saved(state).await
}

pub(crate) async fn effective_image(state: &AppState) -> Result<String, ApiError> {
    if state.config.cloudflared_image_overridden {
        return Ok(state.config.cloudflared_image.clone());
    }
    let mut pin = saved(state).await?.cloudflared_version;
    if pin.is_none() {
        let cached = check_release(state, false).await?;
        let latest: CloudflaredRelease = cached.cloudflared_release.and_then(|v| serde_json::from_value(v).ok())
            .ok_or_else(|| bad("no_cloudflared_release", "No cloudflared version is available. Check GitHub connectivity and check for updates, then retry."))?;
        sqlx::query("UPDATE cloudflare_settings SET cloudflared_version = $1 WHERE id = TRUE")
            .bind(&latest.version)
            .execute(&state.pool)
            .await
            .map_err(ApiError::database)?;
        pin = Some(latest.version);
    }
    Ok(format!(
        "cloudflare/cloudflared:{}",
        version(pin.as_deref().unwrap_or_default())?
    ))
}

#[derive(Serialize, ToSchema)]
pub struct TunnelReplica {
    pub id: String,
    pub name: String,
    pub image: String,
    pub digest: Option<String>,
    pub version: Option<String>,
    pub state: String,
    pub uptime_seconds: Option<i64>,
    pub restarts: i64,
}

#[derive(Serialize, ToSchema)]
pub struct TunnelStatus {
    pub tunnel_id: Option<String>,
    pub connections: Vec<TunnelConnection>,
    pub connection_error: Option<String>,
    pub replicas: Vec<TunnelReplica>,
    pub replica_error: Option<String>,
    pub pinned_version: Option<String>,
    pub latest_release: Option<CloudflaredRelease>,
    pub checked_at: Option<DateTime<Utc>>,
    pub check_error: Option<String>,
    pub image_overridden: bool,
    pub update_available: bool,
    pub update: Option<TunnelUpdateProgress>,
}

pub(crate) fn running_version(output: &str) -> Option<String> {
    output.split_whitespace().find_map(stable_version)
}

pub(crate) async fn progress(
    state: &AppState,
    update: &TunnelUpdateProgress,
) -> Result<(), ApiError> {
    let json = serde_json::to_value(update)
        .map_err(|_| bad("update", "Could not record tunnel progress"))?;
    sqlx::query("UPDATE cloudflare_settings SET cloudflared_update = $1 WHERE id = TRUE")
        .bind(json)
        .execute(&state.pool)
        .await
        .map_err(ApiError::database)?;
    Ok(())
}

pub(crate) async fn build_status(state: &AppState) -> Result<TunnelStatus, ApiError> {
    let settings = cf::load(&state.pool).await?;
    let cached = check_release(state, false).await?;
    let mut update: Option<TunnelUpdateProgress> = cached
        .cloudflared_update
        .and_then(|v| serde_json::from_value(v).ok());
    if update.as_ref().is_some_and(|job| job.status == "running") {
        if let Ok(_guard) = lock(state).await {
            update = saved(state)
                .await?
                .cloudflared_update
                .and_then(|v| serde_json::from_value(v).ok());
            if let Some(job) = update.as_mut().filter(|job| job.status == "running") {
                job.status = "interrupted".into();
                job.message = Some("The panel stopped during this update. Connectors were left intact; retry the update to recover.".into());
                job.finished_at = Some(Utc::now());
                progress(state, job).await?;
            }
        }
    }
    let mut connection_error = None;
    let connections = if let Some(id) = &settings.tunnel_id {
        match async {
            cf::client(state, &settings)?
                .connection_details(&settings.account_id, id)
                .await
        }
        .await
        {
            Ok(connections) => connections,
            Err(error) => {
                connection_error = Some(error.public_message());
                Vec::new()
            }
        }
    } else {
        Vec::new()
    };
    let mut replica_error = None;
    let mut replicas = Vec::new();
    match tokio::time::timeout(
        Duration::from_secs(8),
        state.local_driver.tunnel_containers(),
    )
    .await
    {
        Ok(Ok(containers)) => {
            for container in containers {
                let output = if container.running {
                    tokio::time::timeout(
                        Duration::from_secs(3),
                        state.local_driver.exec(
                            &container.id,
                            vec!["cloudflared".into(), "--version".into()],
                        ),
                    )
                    .await
                    .ok()
                    .and_then(Result::ok)
                    .filter(|r| r.exit_code == 0)
                } else {
                    None
                };
                let uptime = container
                    .started_at
                    .as_deref()
                    .and_then(|at| DateTime::parse_from_rfc3339(at).ok())
                    .map(|at| (Utc::now().timestamp() - at.timestamp()).max(0));
                replicas.push(TunnelReplica {
                    id: container.id,
                    name: container.name,
                    image: container.image,
                    digest: container.digest,
                    version: output.and_then(|r| running_version(&r.output)),
                    state: container.state,
                    uptime_seconds: container.running.then_some(uptime).flatten(),
                    restarts: container.restarts,
                });
            }
        }
        Ok(Err(error)) => replica_error = Some(error.to_string()),
        Err(_) => replica_error = Some("Docker did not answer within eight seconds".into()),
    }
    let latest: Option<CloudflaredRelease> = cached
        .cloudflared_release
        .and_then(|v| serde_json::from_value(v).ok());
    let update_available = settings.tunnel_id.is_some()
        && !state.config.cloudflared_image_overridden
        && latest.as_ref().is_some_and(|latest| {
            replicas.iter().any(|r| {
                r.version
                    .as_deref()
                    .is_none_or(|v| newer(&latest.version, v))
            }) || cached
                .cloudflared_version
                .as_deref()
                .is_none_or(|v| newer(&latest.version, v))
        });
    Ok(TunnelStatus {
        tunnel_id: settings.tunnel_id,
        connections,
        connection_error,
        replicas,
        replica_error,
        pinned_version: cached.cloudflared_version,
        latest_release: latest,
        checked_at: cached.cloudflared_checked_at,
        check_error: cached.cloudflared_check_error,
        image_overridden: state.config.cloudflared_image_overridden,
        update_available,
        update,
    })
}

pub async fn status(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<TunnelStatus>, ApiError> {
    auth.require_admin()?;
    build_status(&state).await.map(Json)
}

pub async fn check(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<TunnelStatus>, ApiError> {
    auth.require_write()?;
    check_release(&state, true).await?;
    build_status(&state).await.map(Json)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_nested_flat_empty_and_pending_connections() {
        let nested = parse_connections(
            &json!([{"id":"connector", "version":"2026.10.0", "conns":[{"id":"c1","colo_name":"SIN","origin_ip":"203.0.113.1","opened_at":"2026-11-22T01:00:00Z"},{"id":"c2","is_pending_reconnect":true}]}]),
        );
        assert_eq!(nested.len(), 2);
        assert_eq!(nested[0].connector_id, "connector");
        assert_eq!(nested[0].colo.as_deref(), Some("SIN"));
        assert_eq!(nested[0].version.as_deref(), Some("2026.10.0"));
        assert!(!nested[1].healthy);
        let flat = parse_connections(
            &json!({"connections":[{"id":"conn","client_id":"client","client_version":"2026.11.0"}]}),
        );
        assert_eq!(flat[0].connector_id, "client");
        assert!(parse_connections(&json!(null)).is_empty());
        assert!(parse_connections(&json!([])).is_empty());
        assert!(parse_connections(&json!([{"id":"empty", "conns":[]}])).is_empty());
    }

    #[test]
    fn explicit_image_override_is_distinct_from_the_legacy_default() {
        use std::collections::HashMap;
        let mut vars = HashMap::from([
            ("DATABASE_URL".into(), "postgres://localhost/test".into()),
            ("DYMINI_PANEL_MASTER_KEY".into(), "aa".repeat(32)),
        ]);
        let normal = crate::Config::from_map(&vars, |_| unreachable!()).expect("config");
        assert!(!normal.cloudflared_image_overridden);
        vars.insert(
            "DYMINI_PANEL_CLOUDFLARED_IMAGE".into(),
            dymini_panel_core::DEFAULT_CLOUDFLARED_IMAGE.into(),
        );
        let overridden = crate::Config::from_map(&vars, |_| unreachable!()).expect("config");
        assert!(overridden.cloudflared_image_overridden);
        vars.insert("DYMINI_PANEL_CLOUDFLARED_IMAGE".into(), String::new());
        let empty = crate::Config::from_map(&vars, |_| unreachable!()).expect("config");
        assert!(!empty.cloudflared_image_overridden);
    }

    #[test]
    fn versions_sort_numerically_and_ignore_unstable_tags() {
        assert!(newer("2026.11.0", "2026.9.9"));
        assert!(!newer("2026.9.0", "2026.11.0"));
        assert!(!newer("2026.11.0", "v2026.11.0"));
        assert!(!newer("invalid", "2026.11.0"));
        assert!(version("latest").is_err());
        assert!(version("2026.11.0-rc.1").is_err());
        let releases = serde_json::from_value(json!([{ "tag_name":"2026.9.0" },{ "tag_name":"2026.11.0" },{ "tag_name":"2027.1.0", "draft":true },{ "tag_name":"2027.2.0-rc.1" }])).expect("fixture");
        assert_eq!(
            newest(releases).expect("stable release").version,
            "2026.11.0"
        );
        assert!(newest(Vec::new()).is_none());
        assert_eq!(
            running_version("cloudflared version 2026.11.0 (built today)").as_deref(),
            Some("2026.11.0")
        );
    }
}
