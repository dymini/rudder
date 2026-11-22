//! A surge connector is registered before the old connector is retired.

use std::time::Duration;

use async_trait::async_trait;
use axum::{extract::State, http::StatusCode, Json};
use chrono::Utc;
use rudder_core::{cloudflared_spec, replica_container_name, NodeDriver, CLOUDFLARED_METRICS_PORT};
use serde::Deserialize;
use serde_json::json;
use utoipa::ToSchema;

use crate::{
    auth::AuthUser,
    cloudflare as cf,
    error::ApiError,
    routes::catalog::{bad, conflict, core_error},
    state::AppState,
    tunnel::{self, CloudflaredRelease, TunnelUpdateProgress},
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(180);

#[async_trait]
trait Connectors: Send + Sync {
    async fn exists(&self, name: &str) -> Result<bool, String>;
    async fn start(&self, name: &str, image: &str) -> Result<String, String>;
    async fn ready(&self, name: &str) -> Result<bool, String>;
    async fn rename(&self, from: &str, to: &str) -> Result<(), String>;
    async fn remove(&self, name: &str) -> Result<(), String>;
    async fn resume(&self, name: &str) -> Result<(), String>;
    async fn retire(&self, name: &str) -> Result<(), String> {
        self.remove(name).await
    }
}

struct Docker<'a> {
    state: &'a AppState,
    token: &'a str,
    index: u8,
}

#[async_trait]
impl Connectors for Docker<'_> {
    async fn exists(&self, name: &str) -> Result<bool, String> {
        self.state
            .local_driver
            .container_snapshot(name)
            .await
            .map(|v| v.is_some())
            .map_err(|e| e.to_string())
    }
    async fn resume(&self, name: &str) -> Result<(), String> {
        let driver = &self.state.local_driver;
        if !driver
            .container_running(name)
            .await
            .map_err(|e| e.to_string())?
        {
            driver
                .start_container(name)
                .await
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    async fn start(&self, name: &str, image: &str) -> Result<String, String> {
        let mut spec =
            cloudflared_spec(self.token, image, self.index).map_err(|e| e.to_string())?;
        spec.name = name.to_owned();
        let id = self
            .state
            .local_driver
            .create_container(&spec)
            .await
            .map_err(|e| e.to_string())?;
        self.state
            .local_driver
            .start_container(&id)
            .await
            .map_err(|e| e.to_string())?;
        Ok(id)
    }
    async fn ready(&self, name: &str) -> Result<bool, String> {
        let snapshot = self
            .state
            .local_driver
            .container_snapshot(name)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "The connector disappeared".to_owned())?;
        if !snapshot.running || (name.ends_with("-next") && snapshot.restarts > 0) {
            return Err("The connector stopped or restarted before becoming ready".into());
        }
        let logs = self
            .state
            .local_driver
            .container_logs(name, 200)
            .await
            .map_err(|e| e.to_string())?;
        if !registered(&logs) {
            return Ok(false);
        }
        self.state
            .local_driver
            .http_ready(name, CLOUDFLARED_METRICS_PORT, "/ready")
            .await
            .map_err(|e| e.to_string())
    }
    async fn rename(&self, from: &str, to: &str) -> Result<(), String> {
        self.state
            .local_driver
            .rename_running_container(from, to)
            .await
            .map_err(|e| e.to_string())
    }
    async fn remove(&self, name: &str) -> Result<(), String> {
        self.state
            .local_driver
            .remove_container(name, true)
            .await
            .map_err(|e| e.to_string())
    }
    async fn retire(&self, name: &str) -> Result<(), String> {
        let driver = &self.state.local_driver;
        if driver
            .container_running(name)
            .await
            .map_err(|e| e.to_string())?
        {
            driver
                .stop_container(name, 60)
                .await
                .map_err(|e| e.to_string())?;
        }
        driver
            .remove_container(name, false)
            .await
            .map_err(|e| e.to_string())
    }
}

fn registered(logs: &str) -> bool {
    logs.lines()
        .any(|line| line.contains("Registered tunnel connection"))
}

async fn cleanup(ops: &impl Connectors, name: &str) -> Result<(), String> {
    if ops.exists(name).await? {
        ops.remove(name).await?;
    }
    Ok(())
}

async fn recover(ops: &impl Connectors, name: &str) -> Result<(), String> {
    let previous = format!("{name}-previous");
    let next = format!("{name}-next");
    if ops.exists(&previous).await? {
        if !ops.exists(name).await? {
            ops.rename(&previous, name).await?;
        } else if ops.ready(name).await? {
            ops.retire(&previous).await?;
        } else {
            return Err(
                "An interrupted replacement is not healthy; both connectors were preserved".into(),
            );
        }
    }
    if ops.exists(&next).await? {
        if !ops.exists(name).await? || !ops.ready(name).await? {
            return Err("An interrupted surge remains and the original is not healthy; both connectors were preserved".into());
        }
        ops.remove(&next).await?;
    }
    Ok(())
}

pub(crate) async fn recover_replicas(state: &AppState, token: &str) -> Result<(), ApiError> {
    for index in 1..=rudder_core::MAX_TUNNEL_REPLICAS {
        recover(
            &Docker {
                state,
                token,
                index,
            },
            &replica_container_name(index),
        )
        .await
        .map_err(|error| {
            ApiError::new(StatusCode::FAILED_DEPENDENCY, "connector_recovery", error)
        })?;
    }
    Ok(())
}

async fn wait_connected(
    ops: &impl Connectors,
    name: &str,
    timeout: Duration,
) -> Result<(), String> {
    tokio::time::timeout(timeout, async {
        loop {
            if ops.ready(name).await? {
                return Ok::<_, String>(());
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    })
    .await
    .unwrap_or_else(|_| Err("Timed out waiting for the connector to register".into()))
}

async fn restore_original(
    ops: &impl Connectors,
    name: &str,
    previous: &str,
    next: &str,
    timeout: Duration,
) -> Result<(), String> {
    if !ops.exists(previous).await? {
        return Err("The original container is no longer present".into());
    }
    ops.resume(previous).await?;
    wait_connected(ops, previous, timeout).await?;
    ops.rename(name, next).await?;
    if let Err(error) = ops.rename(previous, name).await {
        let _ = ops.rename(next, name).await;
        return Err(error);
    }
    cleanup(ops, next).await
}

async fn replace(
    ops: &impl Connectors,
    name: &str,
    image: &str,
    timeout: Duration,
) -> Result<(), String> {
    recover(ops, name).await?;
    let next = format!("{name}-next");
    let previous = format!("{name}-previous");
    let old_exists = ops.exists(name).await?;
    let id = match ops.start(&next, image).await {
        Ok(id) => id,
        Err(error) => {
            let cleanup = cleanup(ops, &next).await;
            return Err(format!(
                "{error}; surge cleanup: {}",
                cleanup.err().unwrap_or_else(|| "complete".into())
            ));
        }
    };
    let ready = wait_connected(ops, &next, timeout).await;
    if let Err(error) = ready {
        let cleanup = ops.remove(&id).await;
        return Err(format!(
            "{error}; the original connector was kept; surge cleanup: {}",
            cleanup.err().unwrap_or_else(|| "complete".into())
        ));
    }
    if old_exists {
        if let Err(error) = ops.rename(name, &previous).await {
            let _ = ops.remove(&id).await;
            return Err(error);
        }
    }
    if let Err(error) = ops.rename(&next, name).await {
        let _ = ops.remove(&id).await;
        if old_exists {
            let _ = ops.rename(&previous, name).await;
        }
        return Err(error);
    }
    if old_exists {
        if let Err(error) = ops.retire(&previous).await {
            let result = restore_original(ops, name, &previous, &next, timeout).await;
            return Err(match result {
                Ok(()) => format!("Could not retire the old connector: {error}. The original was restored and the surge removed."),
                Err(restore) => format!("Could not retire the old connector: {error}. Could not restore the original: {restore}. The registered replacement was retained to preserve connectivity; retry to recover."),
            });
        }
    }
    Ok(())
}

#[derive(Deserialize, ToSchema)]
pub struct UpdateTunnel {
    pub version: Option<String>,
}

pub async fn update(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(request): Json<UpdateTunnel>,
) -> Result<(StatusCode, Json<TunnelUpdateProgress>), ApiError> {
    auth.require_write()?;
    let guard = tunnel::lock(&state).await?;
    if state.config.cloudflared_image_overridden {
        return Err(conflict("image_overridden", "RUDDER_CLOUDFLARED_IMAGE overrides managed versions. Remove the override before using rolling updates."));
    }
    let settings = cf::load(&state.pool).await?;
    cf::require_tunnel(&settings)?;
    let token = cf::tunnel_token(&state, &settings)?;
    let version = match request.version {
        Some(version) => tunnel::version(version.trim())?,
        None => {
            let cached = tunnel::check_release(&state, false).await?;
            let release: CloudflaredRelease = cached.cloudflared_release.and_then(|v| serde_json::from_value(v).ok())
                .ok_or_else(|| bad("no_cloudflared_release", "No stable cloudflared release is cached. Check for updates, or specify a stable target version."))?;
            release.version
        }
    };
    let mut job = TunnelUpdateProgress {
        status: "running".into(),
        image: format!("cloudflare/cloudflared:{version}"),
        version,
        replica: None,
        completed: 0,
        total: settings.replicas as u8,
        message: None,
        started_at: Utc::now(),
        finished_at: None,
    };
    tunnel::progress(&state, &job).await?;
    crate::audit::record(
        &state.pool,
        Some(auth.id),
        "cloudflare.tunnel.update",
        "cloudflare",
        settings.tunnel_id.as_deref(),
        json!({"version":job.version}),
        None,
    )
    .await;
    let initial = job.clone();
    tokio::spawn(async move {
        let _guard = guard;
        let result = rollout(&state, &token, &mut job).await;
        job.status = if result.is_ok() {
            "succeeded"
        } else {
            "failed"
        }
        .into();
        job.message = result.err().map(|e| e.public_message());
        job.finished_at = Some(Utc::now());
        if let Err(error) = tunnel::progress(&state, &job).await {
            tracing::error!(error = %error.public_message(), "could not save tunnel update result");
        }
    });
    Ok((StatusCode::ACCEPTED, Json(initial)))
}

async fn rollout(
    state: &AppState,
    token: &str,
    job: &mut TunnelUpdateProgress,
) -> Result<(), ApiError> {
    state
        .local_driver
        .pull_image(&job.image, None)
        .await
        .map_err(core_error)?;
    for index in 1..=job.total {
        job.replica = Some(index);
        tunnel::progress(state, job).await?;
        replace(
            &Docker {
                state,
                token,
                index,
            },
            &replica_container_name(index),
            &job.image,
            CONNECT_TIMEOUT,
        )
        .await
        .map_err(|error| ApiError::new(StatusCode::FAILED_DEPENDENCY, "connector_update", error))?;
        job.completed = index;
        tunnel::progress(state, job).await?;
    }
    sqlx::query("UPDATE cloudflare_settings SET cloudflared_version = $1, updated_at = now() WHERE id = TRUE")
        .bind(&job.version).execute(&state.pool).await.map_err(ApiError::database)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::HashSet, sync::Mutex};

    struct Fake {
        names: Mutex<HashSet<String>>,
        calls: Mutex<Vec<String>>,
        healthy: bool,
        fail_start: bool,
        fail_retire: bool,
        fail_restore: bool,
    }
    impl Fake {
        fn new(healthy: bool) -> Self {
            Self {
                names: Mutex::new(HashSet::from(["one".into(), "two".into()])),
                calls: Mutex::default(),
                healthy,
                fail_start: false,
                fail_retire: false,
                fail_restore: false,
            }
        }
        fn log(&self, value: String) {
            self.calls.lock().unwrap().push(value);
        }
    }
    #[async_trait]
    impl Connectors for Fake {
        async fn exists(&self, name: &str) -> Result<bool, String> {
            Ok(self.names.lock().unwrap().contains(name))
        }
        async fn start(&self, name: &str, _: &str) -> Result<String, String> {
            self.log(format!("start {name}"));
            self.names.lock().unwrap().insert(name.into());
            if self.fail_start {
                Err("failed to start".into())
            } else {
                Ok(name.into())
            }
        }
        async fn ready(&self, name: &str) -> Result<bool, String> {
            self.log(format!("ready {name}"));
            Ok(self.healthy)
        }
        async fn rename(&self, from: &str, to: &str) -> Result<(), String> {
            self.log(format!("rename {from} {to}"));
            let mut names = self.names.lock().unwrap();
            assert!(names.remove(from));
            names.insert(to.into());
            Ok(())
        }
        async fn remove(&self, name: &str) -> Result<(), String> {
            self.log(format!("remove {name}"));
            if self.fail_retire && name.ends_with("-previous") {
                return Err("retirement failed".into());
            }
            assert!(self.names.lock().unwrap().remove(name));
            Ok(())
        }
        async fn resume(&self, name: &str) -> Result<(), String> {
            self.log(format!("resume {name}"));
            if self.fail_restore {
                Err("original cannot restart".into())
            } else {
                Ok(())
            }
        }
    }
    #[tokio::test]
    async fn connects_each_surge_before_removing_original_even_with_one_replica() {
        let single = Fake::new(true);
        single.names.lock().unwrap().remove("two");
        replace(&single, "one", "image", Duration::from_secs(1))
            .await
            .unwrap();
        assert_eq!(single.names.lock().unwrap().len(), 1);
        let ops = Fake::new(true);
        for name in ["one", "two"] {
            replace(
                &ops,
                name,
                "cloudflare/cloudflared:2026.11.0",
                Duration::from_secs(1),
            )
            .await
            .unwrap();
        }
        assert_eq!(
            *ops.calls.lock().unwrap(),
            [
                "start one-next",
                "ready one-next",
                "rename one one-previous",
                "rename one-next one",
                "remove one-previous",
                "start two-next",
                "ready two-next",
                "rename two two-previous",
                "rename two-next two",
                "remove two-previous"
            ]
        );
        assert!(ops.names.lock().unwrap().contains("one"));
    }
    #[tokio::test]
    async fn retirement_failure_restores_original_before_removing_surge() {
        let mut ops = Fake::new(true);
        ops.fail_retire = true;
        let error = replace(&ops, "one", "image", Duration::from_secs(1))
            .await
            .unwrap_err();
        assert!(error.contains("restored"));
        assert!(ops.names.lock().unwrap().contains("one"));
        assert!(!ops.names.lock().unwrap().contains("one-previous"));
        assert!(!ops.names.lock().unwrap().contains("one-next"));
    }
    #[tokio::test]
    async fn failed_original_restore_keeps_registered_replacement() {
        let mut ops = Fake::new(true);
        ops.fail_retire = true;
        ops.fail_restore = true;
        let error = replace(&ops, "one", "image", Duration::from_secs(1))
            .await
            .unwrap_err();
        assert!(error.contains("retained"));
        assert!(ops.names.lock().unwrap().contains("one"));
        assert!(ops.names.lock().unwrap().contains("one-previous"));
    }
    #[tokio::test]
    async fn timeout_and_start_failure_remove_surge_and_keep_original() {
        for fail_start in [false, true] {
            let mut ops = Fake::new(false);
            ops.fail_start = fail_start;
            assert!(replace(&ops, "one", "image", Duration::from_millis(2))
                .await
                .is_err());
            assert!(ops.names.lock().unwrap().contains("one"));
            assert!(!ops.names.lock().unwrap().contains("one-next"));
            assert!(!ops
                .calls
                .lock()
                .unwrap()
                .iter()
                .any(|c| c.starts_with("rename")));
        }
    }
    #[tokio::test]
    async fn interrupted_rename_restores_original_before_cleaning_surge() {
        let ops = Fake::new(true);
        {
            let mut names = ops.names.lock().unwrap();
            names.remove("one");
            names.insert("one-previous".into());
            names.insert("one-next".into());
        }
        recover(&ops, "one").await.unwrap();
        assert!(ops.names.lock().unwrap().contains("one"));
        assert!(!ops.names.lock().unwrap().contains("one-next"));
    }
    #[test]
    fn registration_is_not_confused_with_other_messages() {
        assert!(registered(
            "INF Registered tunnel connection connIndex=0 location=sin02"
        ));
        assert!(registered(r#"{"message":"Registered tunnel connection"}"#));
        assert!(!registered(""));
        assert!(!registered("Unregistered tunnel connection"));
        assert!(!registered("Unable to establish connection"));
    }
}
