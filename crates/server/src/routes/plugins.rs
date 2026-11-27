//! Plugin catalog, release checks, installs and protected upgrades.

#[path = "plugin_restore.rs"]
mod restore;
pub use restore::PluginRestore;

use super::{
    backups::{create_job, SaveBackupJob},
    catalog::{bad, conflict, core_error, not_found},
    stacks::{deploy_from_template, template_view, DeployTemplate, TemplateDeployed, TemplateView},
};
use crate::{
    audit,
    auth::AuthUser,
    error::ApiError,
    plugins::{self, PluginCheck, PluginRelease},
    stacks::{self, StackAction},
    state::AppState,
};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use dymini_panel_core::{parse_version, Consistency, Plugin, RetentionSpec};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::Row;
use std::collections::HashMap;
use std::time::Duration;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Serialize, Deserialize, ToSchema)]
pub struct PluginInstall {
    pub id: Uuid,
    pub plugin: String,
    pub service_id: Uuid,
    pub service_name: String,
    pub environment_id: Uuid,
    pub environment_name: String,
    pub server_id: Uuid,
    pub server_name: String,
    pub installed_version: String,
    pub latest_version: Option<String>,
    pub update_available: bool,
    pub pending_releases: Vec<PluginRelease>,
    pub image: String,
    pub backup_job_id: Option<Uuid>,
    pub access_enabled: bool,
    pub access_bypass_paths: Vec<String>,
    pub safety_run_id: Option<Uuid>,
    pub status: String,
    pub last_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Serialize, ToSchema)]
pub struct PluginView {
    pub manifest: TemplateView,
    pub repo: String,
    pub descriptor_asset: String,
    pub image_variable: String,
    pub data_volume: String,
    pub health_path: String,
    pub compose: String,
    pub installs: Vec<PluginInstall>,
    pub last_check: Option<PluginCheck>,
}

#[derive(Deserialize, ToSchema)]
pub struct PluginBackup {
    pub destination_ids: Vec<Uuid>,
    pub schedule: Option<String>,
    pub keep_last: Option<u32>,
}

#[derive(Deserialize, ToSchema)]
pub struct InstallPlugin {
    pub name: String,
    pub server_id: Uuid,
    #[serde(default)]
    pub values: HashMap<String, String>,
    pub start: Option<bool>,
    pub version: Option<String>,
    pub backup: Option<PluginBackup>,
    pub access_enabled: Option<bool>,
}

#[derive(Deserialize, ToSchema)]
pub struct PluginAccessSettings {
    pub enabled: bool,
}

#[derive(Deserialize, ToSchema)]
pub struct UpdatePlugin {
    pub version: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct PluginInstalled {
    #[serde(flatten)]
    pub deployed: TemplateDeployed,
    pub install: PluginInstall,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/plugins", get(catalog))
        .route("/api/v1/plugins/{slug}", get(detail))
        .route("/api/v1/plugins/{slug}/check", post(check))
        .route(
            "/api/v1/environments/{id}/plugins/{slug}/install",
            post(install),
        )
        .route("/api/v1/plugin-installs/{id}/update", post(update))
        .route("/api/v1/plugin-installs/{id}/backup", post(backup))
        .route(
            "/api/v1/plugin-installs/{id}/access",
            axum::routing::put(access_settings),
        )
        .route(
            "/api/v1/plugin-installs/{id}/restore",
            post(restore::restore),
        )
}

async fn cached(state: &AppState, slug: &str) -> Result<Option<PluginCheck>, ApiError> {
    let row = sqlx::query!("SELECT checked_at, latest_version, releases, error FROM plugin_release_checks WHERE plugin = $1", slug)
        .fetch_optional(&state.pool).await.map_err(ApiError::database)?;
    row.map(|row| {
        Ok(PluginCheck {
            checked_at: row.checked_at,
            latest_version: row.latest_version,
            releases: serde_json::from_value(row.releases)
                .map_err(|_| bad("invalid_cache", "invalid plugin release cache"))?,
            error: row.error,
        })
    })
    .transpose()
}

fn manifest(slug: &str) -> Result<&'static Plugin, ApiError> {
    plugins::get(slug).ok_or_else(|| not_found("plugin not found"))
}

async fn installs(state: &AppState, slug: &str) -> Result<Vec<PluginInstall>, ApiError> {
    let rows = sqlx::query(r#"SELECT i.*, s.name AS service_name, s.environment_id, e.name AS environment_name,
        s.server_id, n.name AS server_name FROM plugin_installs i JOIN services s ON s.id = i.service_id
        JOIN environments e ON e.id = s.environment_id JOIN servers n ON n.id = s.server_id
        WHERE i.plugin = $1 ORDER BY i.created_at"#)
        .bind(slug).fetch_all(&state.pool).await.map_err(ApiError::database)?;
    let check = cached(state, slug).await?;
    let latest = check
        .as_ref()
        .and_then(|check| check.latest_version.clone());
    rows.into_iter()
        .map(|row| {
            let installed_version: String = row
                .try_get("installed_version")
                .map_err(ApiError::database)?;
            let update_available = latest
                .as_ref()
                .is_some_and(|latest| is_newer(latest, &installed_version));
            Ok(PluginInstall {
                id: row.try_get("id").map_err(ApiError::database)?,
                plugin: slug.into(),
                service_id: row.try_get("service_id").map_err(ApiError::database)?,
                service_name: row.try_get("service_name").map_err(ApiError::database)?,
                environment_id: row.try_get("environment_id").map_err(ApiError::database)?,
                environment_name: row
                    .try_get("environment_name")
                    .map_err(ApiError::database)?,
                server_id: row.try_get("server_id").map_err(ApiError::database)?,
                server_name: row.try_get("server_name").map_err(ApiError::database)?,
                pending_releases: check
                    .as_ref()
                    .map(|c| {
                        c.releases
                            .iter()
                            .filter(|r| is_newer(&r.version, &installed_version))
                            .cloned()
                            .collect()
                    })
                    .unwrap_or_default(),
                installed_version,
                latest_version: latest.clone(),
                update_available,
                image: row.try_get("image").map_err(ApiError::database)?,
                backup_job_id: row.try_get("backup_job_id").map_err(ApiError::database)?,
                access_enabled: row.try_get("access_enabled").map_err(ApiError::database)?,
                access_bypass_paths: manifest(slug)?.access_paths(
                    &row.try_get::<String, _>("installed_version")
                        .map_err(ApiError::database)?,
                ),
                safety_run_id: row.try_get("safety_run_id").map_err(ApiError::database)?,
                status: row.try_get("status").map_err(ApiError::database)?,
                last_error: row.try_get("last_error").map_err(ApiError::database)?,
                created_at: row.try_get("created_at").map_err(ApiError::database)?,
                updated_at: row.try_get("updated_at").map_err(ApiError::database)?,
            })
        })
        .collect()
}

fn is_newer(target: &str, current: &str) -> bool {
    matches!((parse_version(target), parse_version(current)), (Ok(target), Ok(current)) if target > current)
}

async fn load_install(state: &AppState, id: Uuid) -> Result<PluginInstall, ApiError> {
    let slug: String = sqlx::query_scalar("SELECT plugin FROM plugin_installs WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.pool)
        .await
        .map_err(ApiError::database)?
        .ok_or_else(|| not_found("plugin install not found"))?;
    installs(state, &slug)
        .await?
        .into_iter()
        .find(|i| i.id == id)
        .ok_or_else(|| not_found("plugin install not found"))
}

async fn view(state: &AppState, plugin: &Plugin) -> Result<PluginView, ApiError> {
    let mut template = template_view(&plugin.template);
    template
        .variables
        .retain(|v| v.name != plugin.image_variable);
    Ok(PluginView {
        manifest: template,
        repo: plugin.repo.clone(),
        descriptor_asset: plugin.descriptor_asset.clone(),
        image_variable: plugin.image_variable.clone(),
        data_volume: plugin.data_volume.clone(),
        health_path: plugin.health_path.clone(),
        compose: plugin.template.compose.clone(),
        installs: installs(state, &plugin.template.slug).await?,
        last_check: cached(state, &plugin.template.slug).await?,
    })
}

async fn catalog(
    State(state): State<AppState>,
    _auth: AuthUser,
) -> Result<Json<Vec<PluginView>>, ApiError> {
    let mut result = Vec::new();
    for plugin in plugins::all() {
        result.push(view(&state, plugin).await?);
    }
    Ok(Json(result))
}

async fn detail(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(slug): Path<String>,
) -> Result<Json<PluginView>, ApiError> {
    Ok(Json(view(&state, manifest(&slug)?).await?))
}

async fn check(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(slug): Path<String>,
) -> Result<Json<PluginCheck>, ApiError> {
    auth.require_write()?;
    let plugin = manifest(&slug)?;
    let (releases, error) = match plugins::discover(plugin).await {
        Ok(releases) => (releases, None),
        Err(error) => (
            cached(&state, &slug)
                .await?
                .map(|c| c.releases)
                .unwrap_or_default(),
            Some(error.to_string()),
        ),
    };
    let result = PluginCheck {
        checked_at: Utc::now(),
        latest_version: releases.first().map(|r| r.version.clone()),
        releases,
        error,
    };
    sqlx::query("INSERT INTO plugin_release_checks (plugin, checked_at, latest_version, releases, error) VALUES ($1,$2,$3,$4,$5) ON CONFLICT (plugin) DO UPDATE SET checked_at=$2, latest_version=$3, releases=$4, error=$5")
        .bind(&slug).bind(result.checked_at).bind(&result.latest_version).bind(json!(result.releases)).bind(&result.error)
        .execute(&state.pool).await.map_err(ApiError::database)?;
    audit::record(
        &state.pool,
        Some(auth.id),
        "plugin.check",
        "plugin",
        Some(&slug),
        json!({"error": result.error}),
        None,
    )
    .await;
    Ok(Json(result))
}

async fn select_release(
    state: &AppState,
    plugin: &Plugin,
    version: Option<&str>,
) -> Result<PluginRelease, ApiError> {
    let check = cached(state, &plugin.template.slug).await?.ok_or_else(|| {
        conflict(
            "no_release",
            "check for updates first; no verified plugin release is cached",
        )
    })?;
    let release = check
        .releases
        .into_iter()
        .find(|r| version.is_none_or(|v| v == r.version))
        .ok_or_else(|| {
            conflict(
                "no_release",
                "no verified plugin release matches this version",
            )
        })?;
    // Revalidate stored descriptors before using a cached image.
    dymini_panel_core::PluginDescriptor {
        schema: 1,
        plugin: plugin.template.slug.clone(),
        version: release.version.clone(),
        image: release.image.clone(),
        health_path: plugin.health_path.clone(),
        port: plugin.template.port,
    }
    .verify(plugin, &release.version)
    .map_err(core_error)?;
    Ok(release)
}

async fn attach_backup(
    state: &AppState,
    auth: AuthUser,
    install: &PluginInstall,
    request: PluginBackup,
) -> Result<Uuid, ApiError> {
    if request.destination_ids.is_empty() || request.keep_last == Some(0) {
        return Err(bad(
            "invalid_backup",
            "choose at least one destination and keep at least one backup",
        ));
    }
    let plugin = manifest(&install.plugin)?;
    let stack = stacks::load(state, install.service_id).await?;
    let (_, Json(job)) = create_job(
        State(state.clone()),
        auth,
        Json(SaveBackupJob {
            name: Some(format!("{} plugin data", install.service_name)),
            server_id: Some(install.server_id),
            service_id: Some(install.service_id),
            target_kind: Some("volume".into()),
            target_spec: Some(
                json!({"volume": format!("{}_{}", stack.spec.project, plugin.data_volume)}),
            ),
            schedule_cron: Some(request.schedule.unwrap_or_else(|| "@daily".into())),
            destination_ids: Some(request.destination_ids),
            retention: Some(RetentionSpec {
                keep_last: Some(request.keep_last.unwrap_or(7)),
                ..Default::default()
            }),
            consistency: Some(Consistency {
                stop_service: true,
                pre_hook: plugin.backup_hook(&install.installed_version),
                ..Default::default()
            }),
            ..Default::default()
        }),
    )
    .await?;
    sqlx::query("UPDATE plugin_installs SET backup_job_id=$2, updated_at=now() WHERE id=$1")
        .bind(install.id)
        .bind(job.id)
        .execute(&state.pool)
        .await
        .map_err(ApiError::database)?;
    // Replacing the dedicated plugin job also disables its old schedule.
    if let Some(old) = install.backup_job_id {
        sqlx::query("UPDATE backup_jobs SET enabled=FALSE WHERE id=$1")
            .bind(old)
            .execute(&state.pool)
            .await
            .map_err(ApiError::database)?;
    }
    Ok(job.id)
}

async fn install(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((environment, slug)): Path<(Uuid, String)>,
    Json(mut body): Json<InstallPlugin>,
) -> Result<(StatusCode, Json<PluginInstalled>), ApiError> {
    auth.require_write()?;
    let plugin = manifest(&slug)?;
    if body.values.contains_key(&plugin.image_variable) {
        return Err(bad(
            "reserved_variable",
            "the plugin image is managed by Dymini Panel",
        ));
    }
    let release = select_release(&state, plugin, body.version.as_deref()).await?;
    if body.start.unwrap_or(true) {
        crate::nodes::driver_for(&state, body.server_id).await?;
    }
    body.values
        .insert(plugin.image_variable.clone(), release.image.clone());
    let (_, Json(mut deployed)) = deploy_from_template(
        &state,
        &auth,
        environment,
        &plugin.template,
        DeployTemplate {
            name: body.name,
            server_id: body.server_id,
            values: body.values,
            start: Some(false),
        },
        "plugin.install",
    )
    .await?;
    let service_id = deployed.service.id;
    let setup = async {
        let id: Uuid = sqlx::query_scalar("INSERT INTO plugin_installs (plugin,service_id,installed_version,image,status) VALUES ($1,$2,$3,$4,$5) RETURNING id")
            .bind(&slug).bind(service_id).bind(&release.version).bind(&release.image).bind(if body.start.unwrap_or(true) { "installing" } else { "installed" }).fetch_one(&state.pool).await.map_err(ApiError::database)?;
        let installed = load_install(&state, id).await?;
        configure_access(&state, &installed, body.access_enabled.unwrap_or(true)).await?;
        let published = crate::publish::publish_for_service(&state, service_id).await;
        if body.start.unwrap_or(true) { published?; }
        if let Some(backup) = body.backup { attach_backup(&state, auth.clone(), &installed, backup).await?; }
        Ok::<_, ApiError>(id)
    }.await;
    let id = match setup {
        Ok(id) => id,
        Err(error) => {
            sqlx::query("DELETE FROM services WHERE id=$1")
                .bind(service_id)
                .execute(&state.pool)
                .await
                .map_err(ApiError::database)?;
            let _ = crate::publish::publish_server(&state, body.server_id).await;
            return Err(error);
        }
    };
    if body.start.unwrap_or(true) {
        match stacks::start_action(&state, service_id, StackAction::Up, "plugin.install").await {
            Ok(deployment) => {
                let deployment_id = deployment.id;
                deployed.deployment = Some(deployment);
                set_status(&state, id, "installing", None).await?;
                let state = state.clone();
                tokio::spawn(async move {
                    let result = async {
                        wait_result(
                            &state,
                            "deployments",
                            deployment_id,
                            Duration::from_secs(2400),
                        )
                        .await?;
                        wait_healthy(&state, service_id, plugin).await
                    }
                    .await;
                    let (status, error) = match result {
                        Ok(()) => ("installed", None),
                        Err(error) => ("failed", Some(error.public_message())),
                    };
                    let _ = set_status(&state, id, status, error.as_deref()).await;
                });
            }
            Err(error) => {
                set_status(&state, id, "failed", Some(&error.public_message())).await?;
            }
        }
    }
    Ok((
        StatusCode::CREATED,
        Json(PluginInstalled {
            deployed,
            install: load_install(&state, id).await?,
        }),
    ))
}

async fn backup(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(body): Json<PluginBackup>,
) -> Result<Json<PluginInstall>, ApiError> {
    auth.require_write()?;
    claim(&state, id).await?;
    let result = async {
        let install = load_install(&state, id).await?;
        attach_backup(&state, auth.clone(), &install, body).await?;
        audit::record(
            &state.pool,
            Some(auth.id),
            "plugin.backup",
            "plugin_install",
            Some(&id.to_string()),
            json!({}),
            None,
        )
        .await;
        Ok::<_, ApiError>(())
    }
    .await;
    set_status(
        &state,
        id,
        "installed",
        result.as_ref().err().map(|e| e.public_message()).as_deref(),
    )
    .await?;
    result?;
    Ok(Json(load_install(&state, id).await?))
}

async fn configure_access(
    state: &AppState,
    install: &PluginInstall,
    enabled: bool,
) -> Result<(), ApiError> {
    let plugin = manifest(&install.plugin)?;
    if plugin.access.is_none() {
        return Ok(());
    }
    let mut tx = state.pool.begin().await.map_err(ApiError::database)?;
    let created: bool = sqlx::query_scalar("INSERT INTO access_policies (service_id,enabled,bypass_paths) VALUES ($1,$2,$3) ON CONFLICT (service_id) DO UPDATE SET enabled=$2,bypass_paths=$3,updated_at=now() RETURNING (xmax=0)")
        .bind(install.service_id).bind(enabled).bind(plugin.access_paths(&install.installed_version))
        .fetch_one(&mut *tx).await.map_err(ApiError::database)?;
    if created {
        sqlx::query("INSERT INTO access_identities (email,display_name) SELECT email,display_name FROM users WHERE is_owner ON CONFLICT (email) DO NOTHING")
            .execute(&mut *tx).await.map_err(ApiError::database)?;
        sqlx::query("INSERT INTO access_rules (service_id,kind,email) SELECT $1,'email',email FROM users WHERE is_owner")
            .bind(install.service_id).execute(&mut *tx).await.map_err(ApiError::database)?;
    }
    sqlx::query("UPDATE plugin_installs SET access_enabled=$2,updated_at=now() WHERE id=$1")
        .bind(install.id)
        .bind(enabled)
        .execute(&mut *tx)
        .await
        .map_err(ApiError::database)?;
    tx.commit().await.map_err(ApiError::database)?;
    Ok(())
}

pub(crate) async fn reconcile_access(state: &AppState, server: Uuid) -> Result<(), ApiError> {
    let ids: Vec<Uuid> = sqlx::query_scalar("SELECT i.id FROM plugin_installs i JOIN services s ON s.id=i.service_id WHERE s.server_id=$1")
        .bind(server).fetch_all(&state.pool).await.map_err(ApiError::database)?;
    for id in ids {
        let install = load_install(state, id).await?;
        configure_access(state, &install, install.access_enabled).await?;
    }
    Ok(())
}

pub(crate) async fn access_publish_result(
    state: &AppState,
    server: Uuid,
    result: &Result<bool, ApiError>,
) -> Result<(), ApiError> {
    let error = result.as_ref().err().map(|error| {
        format!(
            "Access route publication failed: {}",
            error.public_message()
        )
    });
    sqlx::query("UPDATE plugin_installs i SET last_error=$2 FROM services s WHERE s.id=i.service_id AND s.server_id=$1 AND ($2 IS NOT NULL OR i.last_error LIKE 'Access route publication failed:%')")
        .bind(server).bind(error).execute(&state.pool).await.map_err(ApiError::database)?;
    Ok(())
}

pub(crate) async fn protect_stack_action(
    state: &AppState,
    service: Uuid,
    trigger: &str,
) -> Result<(), ApiError> {
    let id: Option<Uuid> = sqlx::query_scalar("SELECT id FROM plugin_installs WHERE service_id=$1")
        .bind(service)
        .fetch_optional(&state.pool)
        .await
        .map_err(ApiError::database)?;
    let Some(id) = id else {
        return Ok(());
    };
    let install = load_install(state, id).await?;
    let active: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM backup_runs r JOIN backup_jobs j ON j.id=r.job_id WHERE j.service_id=$1 AND r.status IN ('queued','running'))")
        .bind(service).fetch_one(&state.pool).await.map_err(ApiError::database)?;
    if active
        || install.status == "restoring"
        || (install.status == "installing" && trigger != "plugin.install")
        || (install.status == "updating" && trigger != "plugin.update")
    {
        return Err(conflict(
            "plugin_busy",
            "a plugin operation is already running",
        ));
    }
    configure_access(state, &install, install.access_enabled).await?;
    crate::publish::publish_for_service(state, service).await
}

async fn access_settings(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(body): Json<PluginAccessSettings>,
) -> Result<Json<PluginInstall>, ApiError> {
    auth.require_write()?;
    claim(&state, id).await?;
    let result = async {
        let install = load_install(&state, id).await?;
        configure_access(&state, &install, body.enabled).await?;
        crate::publish::publish_for_service(&state, install.service_id).await?;
        audit::record(
            &state.pool,
            Some(auth.id),
            "plugin.access",
            "plugin_install",
            Some(&id.to_string()),
            json!({"enabled":body.enabled}),
            None,
        )
        .await;
        Ok::<_, ApiError>(())
    }
    .await;
    set_status(
        &state,
        id,
        "installed",
        result.as_ref().err().map(|e| e.public_message()).as_deref(),
    )
    .await?;
    result?;
    Ok(Json(load_install(&state, id).await?))
}

async fn claim(state: &AppState, id: Uuid) -> Result<(), ApiError> {
    let mut tx = state.pool.begin().await.map_err(ApiError::database)?;
    let (status, service): (String, Uuid) =
        sqlx::query_as("SELECT status,service_id FROM plugin_installs WHERE id=$1 FOR UPDATE")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(ApiError::database)?
            .ok_or_else(|| not_found("plugin install not found"))?;
    let backing_up: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM backup_runs r JOIN backup_jobs j ON j.id=r.job_id WHERE j.service_id=$1 AND r.status IN ('queued','running')) OR EXISTS(SELECT 1 FROM deployments WHERE service_id=$1 AND status IN ('queued','running'))")
        .bind(service).fetch_one(&mut *tx).await.map_err(ApiError::database)?;
    if matches!(status.as_str(), "updating" | "installing" | "restoring") || backing_up {
        return Err(conflict(
            "plugin_busy",
            "an operation is already running for this install",
        ));
    }
    sqlx::query(
        "UPDATE plugin_installs SET status='updating',last_error=NULL,updated_at=now() WHERE id=$1",
    )
    .bind(id)
    .execute(&mut *tx)
    .await
    .map_err(ApiError::database)?;
    tx.commit().await.map_err(ApiError::database)?;
    Ok(())
}

async fn set_status(
    state: &AppState,
    id: Uuid,
    status: &str,
    error: Option<&str>,
) -> Result<(), ApiError> {
    sqlx::query("UPDATE plugin_installs SET status=$2,last_error=$3,updated_at=now() WHERE id=$1")
        .bind(id)
        .bind(status)
        .bind(error)
        .execute(&state.pool)
        .await
        .map_err(ApiError::database)?;
    Ok(())
}

async fn update(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdatePlugin>,
) -> Result<(StatusCode, Json<PluginInstall>), ApiError> {
    auth.require_write()?;
    let installed = load_install(&state, id).await?;
    let plugin = manifest(&installed.plugin)?;
    let release = select_release(&state, plugin, body.version.as_deref()).await?;
    if body.version.is_none() && !is_newer(&release.version, &installed.installed_version) {
        return Err(conflict(
            "not_newer",
            "no newer release; specify a version explicitly to reinstall or downgrade",
        ));
    }
    claim(&state, id).await?;
    audit::record(
        &state.pool,
        Some(auth.id),
        "plugin.update",
        "plugin_install",
        Some(&id.to_string()),
        json!({"from":installed.installed_version,"to":release.version}),
        None,
    )
    .await;
    let task_state = state.clone();
    tokio::spawn(async move {
        if let Err(error) = update_install(&task_state, &installed, plugin, &release).await {
            let _ = set_status(&task_state, id, "failed", Some(&error.public_message())).await;
        }
    });
    Ok((StatusCode::ACCEPTED, Json(load_install(&state, id).await?)))
}

async fn wait_result(
    state: &AppState,
    table: &str,
    id: Uuid,
    timeout: Duration,
) -> Result<(), ApiError> {
    let query = match table {
        "backup_runs" => "SELECT status, error FROM backup_runs WHERE id=$1",
        _ => "SELECT status, error FROM deployments WHERE id=$1",
    };
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let row = sqlx::query(query)
            .bind(id)
            .fetch_one(&state.pool)
            .await
            .map_err(ApiError::database)?;
        let status: String = row.try_get("status").map_err(ApiError::database)?;
        if !matches!(status.as_str(), "queued" | "running") {
            if status == "succeeded" {
                return Ok(());
            }
            let error: Option<String> = row.try_get("error").map_err(ApiError::database)?;
            return Err(conflict(
                "plugin_operation_failed",
                format!("{table} {status}: {}", error.unwrap_or_default()),
            ));
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(conflict(
                "plugin_timeout",
                format!("timed out waiting for {table}"),
            ));
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

async fn save_image(
    state: &AppState,
    service: Uuid,
    key: &str,
    image: &str,
) -> Result<(), ApiError> {
    let encrypted = state.cipher.encrypt(image.as_bytes()).map_err(core_error)?;
    sqlx::query("INSERT INTO env_vars (service_id,key,value_enc,is_build_secret) VALUES ($1,$2,$3,FALSE) ON CONFLICT (service_id,key,is_build_secret) DO UPDATE SET value_enc=$3")
        .bind(service).bind(key).bind(encrypted).execute(&state.pool).await.map_err(ApiError::database)?;
    Ok(())
}

async fn deploy_image(
    state: &AppState,
    install: &PluginInstall,
    plugin: &Plugin,
    image: &str,
) -> Result<(), ApiError> {
    save_image(state, install.service_id, &plugin.image_variable, image).await?;
    let deployment =
        stacks::start_action(state, install.service_id, StackAction::Up, "plugin.update").await?;
    wait_result(
        state,
        "deployments",
        deployment.id,
        Duration::from_secs(2400),
    )
    .await?;
    wait_healthy(state, install.service_id, plugin).await
}

async fn wait_healthy(state: &AppState, service: Uuid, plugin: &Plugin) -> Result<(), ApiError> {
    let stack = stacks::load(state, service).await?;
    let driver = crate::nodes::driver_for(state, stack.server_id).await?;
    let container = stack
        .spec
        .containers
        .get(&plugin.template.main_service)
        .ok_or_else(|| conflict("plugin_health", "plugin main container is missing"))?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(180);
    loop {
        let healthy = if plugin.health_command.is_empty() {
            driver
                .http_ready(container, plugin.template.port, &plugin.health_path)
                .await
                .unwrap_or(false)
        } else {
            driver
                .exec(container, plugin.health_command.clone())
                .await
                .is_ok_and(|output| output.exit_code == 0)
        };
        if healthy {
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(conflict(
                "plugin_health",
                "plugin did not become healthy within 180 seconds",
            ));
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

async fn update_install(
    state: &AppState,
    install: &PluginInstall,
    plugin: &Plugin,
    release: &PluginRelease,
) -> Result<(), ApiError> {
    configure_access(state, install, install.access_enabled).await?;
    crate::publish::publish_for_service(state, install.service_id).await?;
    if let Some(job) = install.backup_job_id {
        let run = crate::backup::start_run(state, job, "plugin.update").await?;
        wait_result(state, "backup_runs", run, Duration::from_secs(7200))
            .await
            .map_err(|error| {
                conflict(
                    "plugin_backup_failed",
                    format!(
                        "update stopped before changing image: {}",
                        error.public_message()
                    ),
                )
            })?;
    }
    if let Err(error) = deploy_image(state, install, plugin, &release.image).await {
        let rollback = deploy_image(state, install, plugin, &install.image).await;
        let detail = match rollback {
            Ok(()) => format!(
                "update failed and previous image was restored: {}",
                error.public_message()
            ),
            Err(rollback) => {
                return Err(conflict(
                    "plugin_rollback_failed",
                    format!(
                        "update failed: {}; rollback failed: {}",
                        error.public_message(),
                        rollback.public_message()
                    ),
                ))
            }
        };
        set_status(state, install.id, "rolled_back", Some(&detail)).await?;
        audit::record(
            &state.pool,
            None,
            "plugin.rolled_back",
            "plugin_install",
            Some(&install.id.to_string()),
            json!({"error": detail}),
            None,
        )
        .await;
        return Ok(());
    }
    sqlx::query("UPDATE plugin_installs SET installed_version=$2,image=$3,status='installed',last_error=NULL,updated_at=now() WHERE id=$1")
        .bind(install.id).bind(&release.version).bind(&release.image).execute(&state.pool).await.map_err(ApiError::database)?;
    let installed = load_install(state, install.id).await?;
    configure_access(state, &installed, installed.access_enabled).await?;
    if let Some(job) = installed.backup_job_id {
        sqlx::query("UPDATE backup_jobs SET consistency=consistency || $2 WHERE id=$1")
            .bind(job)
            .bind(json!({"stop_service":true,"pre_hook":plugin.backup_hook(&release.version)}))
            .execute(&state.pool)
            .await
            .map_err(ApiError::database)?;
    }
    audit::record(
        &state.pool,
        None,
        "plugin.updated",
        "plugin_install",
        Some(&install.id.to_string()),
        json!({"version":release.version}),
        None,
    )
    .await;
    Ok(())
}
