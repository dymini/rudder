use std::sync::Arc;

use futures_util::StreamExt;
use rudder_core::NodeDriver;
use rudder_node::ops::{BuildCacheEntry, BuildEvent, BuildRequest, BuildStatus, Request};
use sqlx::FromRow;

use super::*;
use crate::driver::remote::{RemoteDriver, RemoteHandle};

#[derive(Debug)]
pub(super) enum BuildFailure {
    Failed(String),
    Disconnected,
}

impl BuildFailure {
    pub(super) fn text(&self) -> &str {
        match self {
            Self::Failed(message) => message,
            Self::Disconnected => "builder disconnected again; build failed after one retry",
        }
    }
}

impl From<String> for BuildFailure {
    fn from(message: String) -> Self {
        Self::Failed(message)
    }
}

impl From<&str> for BuildFailure {
    fn from(message: &str) -> Self {
        Self::Failed(message.into())
    }
}

#[derive(FromRow)]
pub(super) struct Settings {
    pub builder_id: Option<Uuid>,
    pub target_id: Uuid,
    pub test_command: Option<String>,
    pub timeout_secs: i32,
    cache_limit: i64,
}

pub(super) async fn settings(state: &AppState, queue: Uuid) -> Result<Settings, String> {
    sqlx::query_as(
        "SELECT q.builder_id, s.server_id AS target_id, s.build_test_command AS test_command,
                s.build_timeout_secs AS timeout_secs, COALESCE(b.builder_cache_limit_bytes, 21474836480) AS cache_limit
         FROM build_queue q JOIN deployments d ON d.id = q.deployment_id
         JOIN services s ON s.id = d.service_id LEFT JOIN servers b ON b.id = q.builder_id WHERE q.id = $1"
    ).bind(queue).fetch_one(&state.pool).await.map_err(|_| "could not load build settings".into())
}

pub(super) struct RemoteBuild<'a> {
    pub queue: Uuid,
    pub deployment: Uuid,
    pub service: Uuid,
    pub source: &'a GitSource,
    pub repo: &'a Path,
    pub env: &'a [(String, String)],
    pub plan: &'a BuildPlan,
    pub settings: &'a Settings,
    pub secrets: &'a [&'a str],
}

fn remote_error(session: &RemoteDriver, error: impl std::fmt::Display) -> BuildFailure {
    if session.is_open() {
        BuildFailure::Failed(error.to_string())
    } else {
        BuildFailure::Disconnected
    }
}

fn cache_key(branch: &str) -> String {
    format!("branch-{}", rudder_core::sha256_hex(branch.as_bytes()))
}

async fn default_branch(job: &RemoteBuild<'_>) -> Result<String, String> {
    let argv = vec![
        "git".into(),
        "ls-remote".into(),
        "--symref".into(),
        "origin".into(),
        "HEAD".into(),
    ];
    let output = run_command(&argv, job.repo, job.env, &[], Duration::from_secs(120))
        .await
        .map_err(|_| "could not discover the repository default branch".to_owned())?;
    output
        .lines()
        .find_map(|line| {
            line.strip_prefix("ref: refs/heads/")
                .and_then(|line| line.strip_suffix("\tHEAD"))
        })
        .map(str::to_owned)
        .ok_or_else(|| "repository did not advertise a default branch".into())
}

pub(super) async fn execute(
    state: &AppState,
    job: RemoteBuild<'_>,
    logs: &mut String,
) -> Result<(), BuildFailure> {
    let builder = job.settings.builder_id.ok_or("builder was not selected")?;
    let session = state
        .agents
        .session(builder)
        .ok_or(BuildFailure::Disconnected)?;
    let job_id = job.queue.to_string();
    let result = tokio::select! {
        result = execute_inner(state, &session, &job, logs) => result,
        () = wait_until_cancelled(state, job.queue) => Err(BuildFailure::from("build cancelled")),
        () = tokio::time::sleep(Duration::from_secs(job.settings.timeout_secs as u64)) => Err(BuildFailure::from("build timed out")),
    };
    if result.is_err() {
        let _ = session
            .call::<bool>(Request::BuildCancel {
                job_id: job_id.clone(),
            })
            .await;
    }
    let _ = session
        .call::<()>(Request::BuildWorkspaceRemove { job_id })
        .await;
    if (result.is_err() || builder != job.settings.target_id) && session.is_open() {
        if let Err(error) = RemoteHandle(session).remove_image(&job.plan.image).await {
            logs.push_str(&format!(
                "Builder image cleanup could not finish: {}\n",
                redact(&error.to_string(), job.secrets)
            ));
        }
    }
    result
}

async fn execute_inner(
    state: &AppState,
    session: &Arc<RemoteDriver>,
    job: &RemoteBuild<'_>,
    logs: &mut String,
) -> Result<(), BuildFailure> {
    let default = default_branch(job).await?;
    session
        .call::<()>(Request::BuildWorkspaceRemove {
            job_id: job.queue.to_string(),
        })
        .await
        .map_err(|e| remote_error(session, e))?;
    let repo = job.repo.to_owned();
    let bytes = tokio::task::spawn_blocking(move || {
        let files = rudder_node::tree::git_tracked(&repo)?;
        rudder_node::tree::pack_files(&repo, &files)
    })
    .await
    .map_err(|_| "could not prepare the tracked build tree".to_owned())?
    .map_err(|error| error.to_string())?;
    let driver = RemoteHandle(session.clone());
    let tree = Box::pin(futures_util::stream::once(async move {
        Ok(bytes::Bytes::from(bytes))
    }));
    logs.push_str("copying tracked repository files to builder\n");
    write_logs(state, job.deployment, logs)
        .await
        .map_err(|e| e.public_message())?;
    driver
        .extract_tree(&format!("builder/workspaces/{}/src", job.queue), tree)
        .await
        .map_err(|e| remote_error(session, e))?;
    let current = cache_key(&job.source.branch);
    let fallback = cache_key(&default);
    let mut stream = session
        .output_stream(Request::BuildStart {
            request: BuildRequest {
                job_id: job.queue.to_string(),
                service_id: job.service.to_string(),
                plan: job.plan.clone(),
                cache_key: current.clone(),
                cache_fallback_keys: if current == fallback {
                    Vec::new()
                } else {
                    vec![fallback]
                },
                cache_limit_bytes: job.settings.cache_limit as u64,
                timeout_secs: job.settings.timeout_secs as u64,
                test_command: job.settings.test_command.clone(),
            },
        })
        .await
        .map_err(|e| remote_error(session, e))?;
    let mut flush = tokio::time::interval(Duration::from_secs(1));
    loop {
        tokio::select! {
            _ = flush.tick() => {
                write_logs(state, job.deployment, logs).await.map_err(|e| e.public_message())?;
                if !session.is_open() { return Err(BuildFailure::Disconnected); }
            }
            event = stream.next() => {
                let Some(event) = event else { return Err(BuildFailure::Disconnected); };
                let event = event.map_err(|e| remote_error(session, e))?;
                let event: BuildEvent = serde_json::from_slice(&event).map_err(|_| "invalid builder event".to_owned())?;
                match event {
                    BuildEvent::Log { text, .. } => logs.push_str(&redact(&text, job.secrets)),
                    BuildEvent::Finished { status: BuildStatus::Succeeded, .. } => break,
                    BuildEvent::Finished { status, error, .. } => {
                        let message = match status {
                            BuildStatus::Cancelled => "build cancelled".to_owned(),
                            BuildStatus::TimedOut => "build timed out".to_owned(),
                            _ => redact(error.as_deref().unwrap_or("builder build or image test failed"), job.secrets),
                        };
                        return Err(BuildFailure::Failed(message));
                    }
                }
            }
        }
    }
    write_logs(state, job.deployment, logs)
        .await
        .map_err(|e| e.public_message())?;
    refresh_catalog(state, session).await?;
    if session.server_id != job.settings.target_id {
        logs.push_str("relaying image from builder to deploy target through panel\n");
        write_logs(state, job.deployment, logs)
            .await
            .map_err(|e| e.public_message())?;
        let target = crate::nodes::driver_for(state, job.settings.target_id)
            .await
            .map_err(|e| e.public_message())?;
        crate::deploy::copy_image(&driver, target.as_ref(), &job.plan.image)
            .await
            .map_err(|e| remote_error(session, e))?;
    }
    Ok(())
}

pub(super) async fn refresh_catalog(
    state: &AppState,
    session: &RemoteDriver,
) -> Result<(), BuildFailure> {
    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|_| "could not refresh builder cache catalog".to_owned())?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(format!("build-cache:{}", session.server_id))
        .execute(&mut *tx)
        .await
        .map_err(|_| "could not lock builder cache catalog".to_owned())?;
    let entries: Vec<BuildCacheEntry> = session
        .call(Request::BuildCacheUsage { service_id: None })
        .await
        .map_err(|e| remote_error(session, e))?;
    persist_catalog(&mut tx, session.server_id, entries).await?;
    tx.commit()
        .await
        .map_err(|_| "could not publish builder cache catalog".to_owned())?;
    Ok(())
}

async fn persist_catalog(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    builder: Uuid,
    entries: Vec<BuildCacheEntry>,
) -> Result<(), String> {
    sqlx::query("DELETE FROM build_cache_entries WHERE builder_id = $1")
        .bind(builder)
        .execute(&mut **tx)
        .await
        .map_err(|_| "could not refresh builder cache catalog")?;
    for entry in entries {
        let service = Uuid::parse_str(&entry.service_id)
            .map_err(|_| "invalid service in builder cache catalog")?;
        let size = i64::try_from(entry.size_bytes).map_err(|_| "invalid cache size")?;
        let used = chrono::DateTime::from_timestamp_millis(entry.last_used_at)
            .ok_or("invalid cache timestamp")?;
        let created = chrono::DateTime::from_timestamp_millis(entry.created_at)
            .ok_or("invalid cache timestamp")?;
        sqlx::query("INSERT INTO build_cache_entries (builder_id, service_id, cache_key, size_bytes, last_used_at, created_at)
            SELECT $1, $2, $3, $4, $5, $6 WHERE EXISTS(SELECT 1 FROM services WHERE id = $2)
            ON CONFLICT (builder_id, service_id, cache_key) DO UPDATE SET size_bytes = EXCLUDED.size_bytes, last_used_at = EXCLUDED.last_used_at")
            .bind(builder).bind(service).bind(entry.cache_key).bind(size).bind(used).bind(created)
            .execute(&mut **tx).await.map_err(|_| "could not store builder cache catalog")?;
    }
    Ok(())
}

pub(super) async fn requeue_once(
    state: &AppState,
    queue: Uuid,
    deployment: Uuid,
    service: Uuid,
    had_container: bool,
    logs: &str,
) -> Result<bool, ApiError> {
    let mut tx = state.pool.begin().await.map_err(ApiError::database)?;
    let updated = sqlx::query("UPDATE build_queue SET status = 'queued', builder_id = NULL, started_at = NULL, disconnect_retries = 1
        WHERE id = $1 AND status = 'running' AND disconnect_retries = 0 AND NOT cancel_requested")
        .bind(queue).execute(&mut *tx).await.map_err(ApiError::database)?;
    if updated.rows_affected() == 0 {
        return Ok(false);
    }
    let logs = format!("{logs}Builder disconnected; requeued once for placement\n");
    sqlx::query("UPDATE deployments SET status = 'queued', error = NULL, logs = $2, finished_at = NULL WHERE id = $1")
        .bind(deployment).bind(logs).execute(&mut *tx).await.map_err(ApiError::database)?;
    sqlx::query(
        "UPDATE services SET status = $2, updated_at = now() WHERE id = $1 AND status = 'building'",
    )
    .bind(service)
    .bind(if had_container { "running" } else { "stopped" })
    .execute(&mut *tx)
    .await
    .map_err(ApiError::database)?;
    tx.commit().await.map_err(ApiError::database)?;
    Ok(true)
}
