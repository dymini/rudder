use super::*;
use crate::backup::{self, RestoreRequest};
use dymini_panel_core::{ArchiveSource, ContainerSpec, NodeDriver, VolumeMount};
use std::future::Future;

#[derive(Deserialize, ToSchema)]
pub struct PluginRestore {
    pub run_id: Uuid,
}

pub(super) async fn restore(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(body): Json<PluginRestore>,
) -> Result<(StatusCode, Json<PluginInstall>), ApiError> {
    auth.require_write()?;
    let install = load_install(&state, id).await?;
    let source = backup::load_source_run(&state, body.run_id).await?;
    let job = backup::load_job(&state, source.job_id).await?;
    let plugin = manifest(&install.plugin)?;
    let stack = stacks::load(&state, install.service_id).await?;
    let volume = format!("{}_{}", stack.spec.project, plugin.data_volume);
    if job.service_id != Some(install.service_id)
        || source.meta.target_kind != "volume"
        || source.meta.source
            != Some(ArchiveSource::Volume {
                name: volume.clone(),
            })
    {
        return Err(bad(
            "wrong_plugin_backup",
            "choose a volume backup of this install's data volume",
        ));
    }
    let container = stack
        .spec
        .containers
        .get(&plugin.template.main_service)
        .cloned()
        .ok_or_else(|| conflict("plugin_restore", "plugin main container is missing"))?;
    let driver = crate::nodes::driver_for(&state, install.server_id).await?;
    claim(&state, id).await?;
    set_status(&state, id, "restoring", None).await?;
    audit::record(
        &state.pool,
        Some(auth.id),
        "plugin.restore",
        "plugin_install",
        Some(&id.to_string()),
        json!({"source_run":body.run_id}),
        None,
    )
    .await;
    let response = load_install(&state, id).await?;
    tokio::spawn(async move {
        let context = Context {
            state: &state,
            auth,
            install: &install,
            plugin,
            source_run: body.run_id,
            job: &job,
            volume,
            container,
            driver: driver.as_ref(),
        };
        let result = orchestrate(|step| context.step(step)).await;
        let (status, error) = match result {
            Ok(None) => ("installed", None),
            Ok(Some(error)) => ("rolled_back", Some(error)),
            Err(error) => ("failed", Some(error.public_message())),
        };
        let _ = set_status(&state, id, status, error.as_deref()).await;
        audit::record(
            &state.pool,
            None,
            "plugin.restore.finished",
            "plugin_install",
            Some(&id.to_string()),
            json!({"status":status,"error":error}),
            None,
        )
        .await;
    });
    Ok((StatusCode::ACCEPTED, Json(response)))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Stop,
    Safety,
    RestoreSource,
    Start,
    Health,
    RestoreSafety(Uuid),
}

async fn orchestrate<F, Fut>(mut step: F) -> Result<Option<String>, ApiError>
where
    F: FnMut(Step) -> Fut,
    Fut: Future<Output = Result<Option<Uuid>, ApiError>>,
{
    if let Err(error) = step(Step::Stop).await {
        return Err(restart_error(&mut step, error).await);
    }
    let safety = match step(Step::Safety).await {
        Ok(Some(run)) => run,
        Ok(None) => {
            return Err(restart_error(
                &mut step,
                conflict("plugin_safety", "the safety backup did not return a run"),
            )
            .await)
        }
        Err(error) => return Err(restart_error(&mut step, error).await),
    };
    let result = async {
        step(Step::RestoreSource).await?;
        step(Step::Start).await?;
        step(Step::Health).await?;
        Ok::<_, ApiError>(())
    }
    .await;
    if let Err(error) = result {
        let rollback = async {
            step(Step::Stop).await?;
            step(Step::RestoreSafety(safety)).await?;
            step(Step::Start).await?;
            step(Step::Health).await?;
            Ok::<_, ApiError>(())
        }
        .await;
        return match rollback {
            Ok(()) => Ok(Some(format!(
                "restore failed; safety backup {safety} restored: {}",
                error.public_message()
            ))),
            Err(rollback) => {
                let stopped = step(Step::Stop)
                    .await
                    .err()
                    .map(|e| format!(" Could not stop: {}.", e.public_message()))
                    .unwrap_or_default();
                Err(conflict("plugin_rollback_failed",format!("restore failed: {}; safety rollback failed: {}. Safety backup: {safety}; keep the service stopped and retry recovery.{stopped}",error.public_message(),rollback.public_message())))
            }
        };
    }
    Ok(None)
}

async fn restart_error<F, Fut>(step: &mut F, error: ApiError) -> ApiError
where
    F: FnMut(Step) -> Fut,
    Fut: Future<Output = Result<Option<Uuid>, ApiError>>,
{
    match step(Step::Start).await {
        Ok(_) => error,
        Err(start) => conflict("plugin_restart_failed",format!("restore stopped before replacing data: {}; could not restart the original service: {}",error.public_message(),start.public_message())),
    }
}

struct Context<'a> {
    state: &'a AppState,
    auth: AuthUser,
    install: &'a PluginInstall,
    plugin: &'a Plugin,
    source_run: Uuid,
    job: &'a backup::Job,
    volume: String,
    container: String,
    driver: &'a dyn NodeDriver,
}

impl Context<'_> {
    async fn idle(&self) -> Result<(), ApiError> {
        let active: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM backup_runs r JOIN backup_jobs j ON j.id=r.job_id WHERE j.service_id=$1 AND r.status IN ('queued','running'))")
            .bind(self.install.service_id).fetch_one(&self.state.pool).await.map_err(ApiError::database)?;
        if active {
            return Err(conflict(
                "plugin_busy",
                "a backup or restore is still writing; the service must stay stopped",
            ));
        }
        Ok(())
    }

    async fn step(&self, step: Step) -> Result<Option<Uuid>, ApiError> {
        match step {
            Step::Stop => {
                self.idle().await?;
                if self
                    .driver
                    .container_running(&self.container)
                    .await
                    .map_err(core_error)?
                {
                    self.driver
                        .stop_container(&self.container, 30)
                        .await
                        .map_err(core_error)?;
                }
            }
            Step::Safety => {
                let (_, Json(job)) = create_job(
                    State(self.state.clone()),
                    self.auth.clone(),
                    Json(SaveBackupJob {
                        name: Some(format!("{} restore safety", self.install.service_name)),
                        service_id: Some(self.install.service_id),
                        server_id: Some(self.install.server_id),
                        target_kind: Some("volume".into()),
                        target_spec: Some(json!({"volume":self.volume})),
                        destination_ids: Some(self.job.destination_ids.clone()),
                        encryption_passphrase: self.job.passphrase.clone(),
                        consistency: Some(Consistency {
                            stop_service: true,
                            ..Default::default()
                        }),
                        enabled: Some(false),
                        retries: Some(0),
                        timeout_secs: Some(3600),
                        retention: Some(RetentionSpec::default()),
                        ..Default::default()
                    }),
                )
                .await?;
                let run = backup::start_run(self.state, job.id, "plugin.restore.safety").await?;
                wait_result(self.state, "backup_runs", run, Duration::from_secs(3700)).await?;
                sqlx::query(
                    "UPDATE plugin_installs SET safety_run_id=$2,updated_at=now() WHERE id=$1",
                )
                .bind(self.install.id)
                .bind(run)
                .execute(&self.state.pool)
                .await
                .map_err(ApiError::database)?;
                return Ok(Some(run));
            }
            Step::RestoreSource => self.restore_run(self.source_run).await?,
            Step::RestoreSafety(run) => self.restore_run(run).await?,
            Step::Start => {
                self.idle().await?;
                if !self
                    .driver
                    .container_running(&self.container)
                    .await
                    .map_err(core_error)?
                {
                    self.driver
                        .start_container(&self.container)
                        .await
                        .map_err(core_error)?;
                }
            }
            Step::Health => wait_healthy(self.state, self.install.service_id, self.plugin).await?,
        }
        Ok(None)
    }

    async fn restore_run(&self, source: Uuid) -> Result<(), ApiError> {
        self.idle().await?;
        if self
            .driver
            .container_running(&self.container)
            .await
            .map_err(core_error)?
        {
            return Err(conflict(
                "plugin_running",
                "refusing to replace data while the service is running",
            ));
        }
        clear_volume(self.driver, &self.volume).await?;
        let run = backup::start_plugin_restore(
            self.state,
            source,
            RestoreRequest {
                target_service_id: Some(self.install.service_id),
                target_volume: Some(self.volume.clone()),
                target_path: None,
            },
        )
        .await?;
        wait_result(self.state, "backup_runs", run, Duration::from_secs(86400)).await
    }
}

async fn clear_volume(driver: &dyn NodeDriver, volume: &str) -> Result<(), ApiError> {
    ArchiveSource::Volume {
        name: volume.into(),
    }
    .validate()
    .map_err(core_error)?;
    let helper = driver
        .create_container(&ContainerSpec {
            name: format!("dymini-panel-plugin-restore-{}", Uuid::new_v4().simple()),
            image: dymini_panel_node::HELPER_IMAGE.into(),
            env: vec![],
            command: vec!["sleep".into(), "3600".into()],
            ports: vec![],
            volumes: vec![VolumeMount {
                source: volume.into(),
                target: "/data".into(),
                read_only: false,
            }],
            memory_bytes: Some(128 * 1024 * 1024),
            nano_cpus: None,
            restart: "no".into(),
            network: Some("none".into()),
            labels: vec![],
        })
        .await
        .map_err(core_error)?;
    let result = async {
        driver.start_container(&helper).await.map_err(core_error)?;
        let result = driver
            .exec(
                &helper,
                vec![
                    "sh".into(),
                    "-c".into(),
                    "find /data -mindepth 1 -maxdepth 1 -exec rm -rf -- {} +".into(),
                ],
            )
            .await
            .map_err(core_error)?;
        if result.exit_code != 0 {
            return Err(conflict(
                "plugin_clear_volume",
                "could not clear the data volume before restore",
            ));
        }
        Ok(())
    }
    .await;
    let cleanup = driver
        .remove_container(&helper, true)
        .await
        .map_err(core_error);
    result.and(cleanup)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    async fn run(fail_at: &[usize]) -> (Result<Option<String>, ApiError>, Vec<Step>) {
        let calls = Mutex::new(Vec::new());
        let result = orchestrate(|step| {
            let mut calls = calls.lock().unwrap();
            calls.push(step);
            let fail = fail_at.contains(&calls.len());
            async move {
                if fail {
                    Err(conflict("test_failure", "injected failure"))
                } else {
                    Ok(if step == Step::Safety {
                        Some(Uuid::nil())
                    } else {
                        None
                    })
                }
            }
        })
        .await;
        (result, calls.into_inner().unwrap())
    }

    #[tokio::test]
    async fn stops_and_saves_safety_before_replacing_data() {
        let (result, calls) = run(&[]).await;
        assert!(matches!(result, Ok(None)));
        assert_eq!(
            calls,
            vec![
                Step::Stop,
                Step::Safety,
                Step::RestoreSource,
                Step::Start,
                Step::Health
            ]
        );
    }

    #[tokio::test]
    async fn failures_before_safety_never_restore_any_data() {
        for failure in [1, 2] {
            let (result, calls) = run(&[failure]).await;
            assert!(result.is_err());
            assert_eq!(calls.last(), Some(&Step::Start));
            assert!(!calls.contains(&Step::RestoreSource));
        }
    }

    #[tokio::test]
    async fn restore_start_and_health_errors_roll_back_and_health_check_the_safety_copy() {
        for failure in [3, 4, 5] {
            let (result, calls) = run(&[failure]).await;
            assert!(matches!(result, Ok(Some(_))));
            assert!(calls.ends_with(&[
                Step::Stop,
                Step::RestoreSafety(Uuid::nil()),
                Step::Start,
                Step::Health
            ]));
        }
    }

    #[tokio::test]
    async fn failed_rollback_reports_the_safety_run_for_manual_recovery() {
        let (result, _) = run(&[3, 5]).await;
        assert!(result
            .unwrap_err()
            .public_message()
            .contains("Safety backup:"));
        let (result, _) = run(&[2, 3]).await;
        assert!(result
            .unwrap_err()
            .public_message()
            .contains("could not restart"));
        let (result, calls) = run(&[5, 9]).await;
        assert!(result.is_err());
        assert_eq!(calls.last(), Some(&Step::Stop));
    }
}
