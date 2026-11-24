//! Outbound build commands. The panel uploads a tracked tree before BuildStart.

mod cache;
mod runner;
#[cfg(all(test, target_os = "linux"))]
mod tests;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard};
use std::task::{Context, Poll};
use std::time::Duration;

use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use rudder_core::{
    buildx_argv, docker_build_argv, nixpacks_generate_argv, static_dockerfile, validate_rel_path,
    BuildStrategy, ByteStream, Error,
};
use rudder_node::ops::{
    BuildCacheEntry, BuildCachePrune, BuildCachePruned, BuildEvent, BuildRequest, BuildStatus,
};
use tokio::sync::{mpsc, watch};
use tokio_stream::wrappers::ReceiverStream;

type Events = mpsc::Sender<Result<Bytes, Error>>;

#[derive(Default)]
struct State {
    jobs: HashMap<String, watch::Sender<bool>>,
    services: HashSet<String>,
}

pub struct Builder {
    root: PathBuf,
    docker: runner::Tool,
    nixpacks: runner::Tool,
    socket: Option<PathBuf>,
    state: Mutex<State>,
}

impl Builder {
    /// `root` is `node_dir/builder`; uploads go to `builder/workspaces/<job>/src`.
    pub fn new(root: PathBuf, socket: Option<PathBuf>) -> Result<Self, Error> {
        Ok(Self {
            root: std::path::absolute(root)
                .map_err(|_| node("could not resolve builder directory"))?,
            docker: runner::Tool::new("docker"),
            nixpacks: runner::Tool::new("nixpacks"),
            socket,
            state: Mutex::new(State::default()),
        })
    }

    fn state(&self) -> Result<MutexGuard<'_, State>, Error> {
        self.state
            .lock()
            .map_err(|_| node("builder state unavailable"))
    }

    pub async fn capabilities(&self) -> (bool, bool) {
        tokio::join!(
            runner::probe(&self.docker, &["buildx", "version"], self.socket.as_deref()),
            runner::probe(&self.nixpacks, &["--version"], None),
        )
    }

    pub fn cancel(&self, job_id: &str) -> Result<bool, Error> {
        identifier(job_id)?;
        Ok(self
            .state()?
            .jobs
            .get(job_id)
            .is_some_and(|cancel| cancel.send(true).is_ok()))
    }

    pub async fn cleanup_workspace(&self, job_id: &str) -> Result<(), Error> {
        self.cancel(job_id)?;
        for _ in 0..100 {
            {
                let state = self.state()?;
                if !state.jobs.contains_key(job_id) {
                    let path = checked(&self.root, &format!("workspaces/{job_id}"))?;
                    return match std::fs::remove_dir_all(path) {
                        Ok(()) => Ok(()),
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                        Err(_) => Err(node("could not remove build workspace")),
                    };
                }
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Err(node("builder job did not stop before workspace cleanup"))
    }

    pub fn usage(&self, service_id: Option<&str>) -> Result<Vec<BuildCacheEntry>, Error> {
        if let Some(service) = service_id {
            identifier(service)?;
        }
        let _state = self.state()?;
        cache::usage(&self.root, service_id)
    }

    pub fn prune(&self, request: BuildCachePrune) -> Result<BuildCachePruned, Error> {
        let state = self.state()?;
        cache::prune(&self.root, &request, &state.services)
    }

    pub fn start(self: &Arc<Self>, request: BuildRequest) -> Result<ByteStream, Error> {
        identifier(&request.job_id)?;
        identifier(&request.service_id)?;
        identifier(&request.cache_key)?;
        if request.cache_fallback_keys.len() > 2 {
            return Err(invalid(
                "at most branch and default-branch fallback keys are allowed",
            ));
        }
        for key in &request.cache_fallback_keys {
            identifier(key)?;
        }
        if request.timeout_secs == 0 || request.timeout_secs > i32::MAX as u64 {
            return Err(invalid(
                "build timeout must be a positive 32-bit number of seconds",
            ));
        }
        if request
            .test_command
            .as_ref()
            .is_some_and(|cmd| cmd.trim().is_empty() || cmd.len() > 16384 || cmd.contains('\0'))
        {
            return Err(invalid("invalid build test command"));
        }
        // Validate arguments before starting a task; never include values in errors.
        docker_build_argv(&request.plan)?;
        validate_rel_path(&request.plan.publish_dir)?;
        let workspace = checked(&self.root, &format!("workspaces/{}", request.job_id))?;
        let src = checked(&workspace, "src")?;
        if !src.is_dir() {
            return Err(invalid("upload the tracked build tree before starting"));
        }
        checked(&src, &request.plan.context)?;
        checked(&src, &request.plan.dockerfile)?;
        let (cancel, mut cancelled) = watch::channel(false);
        {
            let mut state = self.state()?;
            if state.jobs.contains_key(&request.job_id)
                || state.services.contains(&request.service_id)
            {
                return Err(node("a build for this job or service is already running"));
            }
            state.jobs.insert(request.job_id.clone(), cancel.clone());
            state.services.insert(request.service_id.clone());
        }
        let (events, rx) = mpsc::channel(32);
        let this = self.clone();
        tokio::spawn(async move {
            let cleanup = CleanDir(workspace);
            let result = tokio::select! {
                result = this.execute(&request, &src, &events) => result.map_err(|error| (BuildStatus::Failed, error)),
                _ = cancelled.changed() => Err((BuildStatus::Cancelled, node("build cancelled"))),
                _ = tokio::time::sleep(Duration::from_secs(request.timeout_secs)) => {
                    Err((BuildStatus::TimedOut, node("build timed out")))
                }
            };
            // Docker containers live outside the process group, including after client loss.
            if request.test_command.is_some() {
                runner::remove_test_container(&this, &request.job_id).await;
            }
            drop(cleanup);
            // Keep active caches protected throughout export and workspace cleanup.
            if let Ok(mut state) = this.state() {
                state.jobs.remove(&request.job_id);
                state.services.remove(&request.service_id);
            }
            let pruned = this.prune(BuildCachePrune {
                limit_bytes: Some(request.cache_limit_bytes),
                ..BuildCachePrune::default()
            });
            let result = result.and_then(|()| {
                pruned.as_ref().map(|_| ()).map_err(|_| {
                    (
                        BuildStatus::Failed,
                        node("could not enforce build cache limit"),
                    )
                })
            });
            let (status, error) = match result {
                Ok(()) => (BuildStatus::Succeeded, None),
                Err((status, error)) => (status, Some(error.to_string())),
            };
            let cache = pruned.map(|p| p.entries).unwrap_or_default();
            let _ = emit(
                &events,
                BuildEvent::Finished {
                    status,
                    error,
                    cache,
                },
            )
            .await;
        });
        Ok(Box::pin(BuildStream {
            rx: ReceiverStream::new(rx),
            cancel,
        }))
    }

    async fn execute(
        &self,
        request: &BuildRequest,
        src: &Path,
        events: &Events,
    ) -> Result<(), Error> {
        let plan = request.plan.clone();
        let context = checked(src, &plan.context)?;
        if !context.is_dir() {
            return Err(invalid("build context is not a directory"));
        }
        let secrets: Vec<&str> = plan
            .build_args
            .iter()
            .map(|(_, value)| value.as_str())
            .collect();
        match plan.strategy {
            BuildStrategy::Static => {
                let publish = checked(&context, &plan.publish_dir)?;
                if !publish.is_dir() {
                    return Err(invalid("static publish directory is missing"));
                }
                let file = checked(&context, "Dockerfile.rudder")?;
                std::fs::write(file, static_dockerfile(&plan.publish_dir)?)
                    .map_err(|_| node("could not write static Dockerfile"))?;
            }
            BuildStrategy::Nixpacks => {
                // Nixpacks --out only generates .nixpacks assets; Buildx owns the build/cache.
                checked(&context, ".nixpacks")?;
                runner::run(self, nixpacks_generate_argv(&plan)?, src, &secrets, events).await?;
            }
            BuildStrategy::Dockerfile => {}
        }
        let (imports, stage) = {
            let _state = self.state()?;
            cache::prepare(&self.root, request)?
        };
        let stage_cleanup = CleanDir(stage.clone());
        // The Dockerfile path for every strategy remains owned by core buildspec.
        let argv = buildx_argv(&plan, &imports, &path_string(&stage)?)?;
        runner::run(self, argv, src, &secrets, events).await?;
        if let Some(command) = &request.test_command {
            runner::run(
                self,
                vec![
                    "docker".into(),
                    "run".into(),
                    "--rm".into(),
                    "--name".into(),
                    format!("rudder-build-test-{}", request.job_id),
                    plan.image.clone(),
                    "sh".into(),
                    "-c".into(),
                    command.clone(),
                ],
                src,
                &secrets,
                events,
            )
            .await?;
        }
        {
            let _state = self.state()?;
            cache::publish(&self.root, request, &stage)?;
        }
        drop(stage_cleanup);
        Ok(())
    }
}

struct BuildStream {
    rx: ReceiverStream<Result<Bytes, Error>>,
    cancel: watch::Sender<bool>,
}

impl Stream for BuildStream {
    type Item = Result<Bytes, Error>;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.rx.poll_next_unpin(cx)
    }
}

impl Drop for BuildStream {
    fn drop(&mut self) {
        let _ = self.cancel.send(true);
    }
}

async fn emit(events: &Events, event: BuildEvent) -> Result<(), Error> {
    let bytes = serde_json::to_vec(&event).map_err(|_| node("could not encode build event"))?;
    events
        .send(Ok(Bytes::from(bytes)))
        .await
        .map_err(|_| node("build log channel closed"))
}

fn identifier(value: &str) -> Result<(), Error> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
    {
        return Err(invalid(
            "build identifiers must contain 1-128 letters, digits, hyphens or underscores",
        ));
    }
    Ok(())
}

/// Refuse symlinks even when their current target happens to be inside the root.
fn checked(root: &Path, relative: &str) -> Result<PathBuf, Error> {
    validate_rel_path(relative)?;
    let mut path = root.to_owned();
    for component in relative.split('/').filter(|p| *p != ".") {
        path.push(component);
        match std::fs::symlink_metadata(&path) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(invalid("symlinks are not allowed in builder paths"))
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(node("could not inspect builder path")),
        }
    }
    Ok(path)
}

fn path_string(path: &Path) -> Result<String, Error> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| invalid("builder path must be UTF-8"))
}

fn node(message: &str) -> Error {
    Error::Node(message.into())
}
fn invalid(message: &str) -> Error {
    Error::Validation(message.into())
}

struct CleanDir(PathBuf);
impl Drop for CleanDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
