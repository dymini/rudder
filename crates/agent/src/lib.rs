//! Rudder node agent.
//!
//! The agent dials the panel over gRPC with a client certificate, runs the
//! panel's [`NodeDriver`] calls against the local Docker Engine, and ships
//! container logs and metrics. Collection keeps running while the panel is
//! unreachable; see [`telemetry`] for the buffering rules.

pub mod builder;
mod collect;
mod host;
pub mod identity;
mod session;
pub mod status;
pub mod telemetry;
pub mod update;

use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use rudder_core::NodeDriver;
use rudder_node::LocalDriver;

pub use identity::{enroll, Identity};
pub use telemetry::Buffer;
pub use update::SelfUpdate;

const CURSOR_FILE: &str = "log-cursors.json";
const STATS_INTERVAL: Duration = Duration::from_secs(10);

/// `rudder-agent run` settings.
#[derive(Debug, Clone)]
pub struct AgentConfig {
    pub builder: bool,
    /// Identity and cursors, `/var/lib/rudder-agent`.
    pub data_dir: PathBuf,
    /// Root for files the panel writes, `/var/lib/rudder`.
    pub node_dir: PathBuf,
    pub docker_socket: Option<PathBuf>,
    pub cache_admin_url: Option<String>,
    pub host_proc: Option<PathBuf>,
    /// The binary replaced on update; the running executable by default.
    pub binary: Option<PathBuf>,
    pub update_grace: Duration,
}

/// Exit code that makes the service manager start the (new) binary again.
pub const RESTART_EXIT_CODE: i32 = 75;

/// Settings for [`run_with`].
pub struct RunOptions {
    pub identity: Identity,
    pub data_dir: PathBuf,
    pub stats_interval: Duration,
    /// `None` refuses binary updates from the panel.
    pub self_update: Option<SelfUpdate>,
    /// None disables build operations for embedded agents without a filesystem.
    pub builder: Option<Arc<builder::Builder>>,
}

/// Run the agent with the local Docker driver until SIGINT or SIGTERM.
pub async fn run(config: AgentConfig) -> anyhow::Result<()> {
    let identity = Identity::load(&config.data_dir).with_context(|| {
        format!(
            "not enrolled; run `rudder-agent enroll --server <panel> --token <token> --data-dir {}`",
            config.data_dir.display()
        )
    })?;
    std::fs::create_dir_all(&config.node_dir)
        .with_context(|| format!("create {}", config.node_dir.display()))?;
    let mut driver = LocalDriver::connect(&config.node_dir, config.docker_socket.as_deref());
    if let Some(proc) = &config.host_proc {
        driver = driver.with_host_proc(proc);
    }
    if let Some(url) = &config.cache_admin_url {
        driver = driver.with_cache_admin(url);
    }
    let buffer = Arc::new(Buffer::with_cursors(collect::load_cursors(
        &config.data_dir.join(CURSOR_FILE),
    )));
    collect::ship_own_logs(buffer.clone());
    tracing::info!(
        version = rudder_core::VERSION,
        server_id = %identity.meta.server_id,
        panel = %identity.meta.grpc_url,
        "rudder-agent starting"
    );
    let binary = match config.binary {
        Some(binary) => binary,
        None => std::env::current_exe().context("locate the agent binary")?,
    };
    let self_update = SelfUpdate {
        binary,
        data_dir: config.data_dir.clone(),
        restart: Arc::new(|| std::process::exit(RESTART_EXIT_CODE)),
        grace: config.update_grace,
    };
    run_with(
        RunOptions {
            identity,
            data_dir: config.data_dir.clone(),
            stats_interval: STATS_INTERVAL,
            self_update: Some(self_update),
            builder: if config.builder {
                Some(Arc::new(builder::Builder::new(
                    config.data_dir.join("builder"),
                    config.docker_socket,
                )?))
            } else {
                None
            },
        },
        Arc::new(driver),
        buffer,
        shutdown_signal(),
    )
    .await;
    tracing::info!("rudder-agent stopped");
    Ok(())
}

/// Run the session and collectors on any driver until `shutdown` resolves.
/// Dropping the returned future also stops every task it started.
pub async fn run_with(
    options: RunOptions,
    driver: Arc<dyn NodeDriver>,
    buffer: Arc<Buffer>,
    shutdown: impl Future<Output = ()>,
) {
    let (connected, welcomed) = tokio::sync::watch::channel(false);
    let status = match status::StatusFile::new(
        &options.data_dir,
        &options.identity.meta,
        options.builder.is_some(),
    ) {
        Ok(file) => Some(Arc::new(file)),
        Err(error) => {
            tracing::warn!(%error, "could not initialize agent status");
            None
        }
    };
    let status_shutdown = status.clone();
    let builder_status = options.builder.clone();
    let mut tasks = Tasks(vec![
        tokio::spawn(collect::stats(
            driver.clone(),
            buffer.clone(),
            options.stats_interval,
        )),
        tokio::spawn(collect::logs(
            driver.clone(),
            buffer.clone(),
            options.data_dir.join(CURSOR_FILE),
        )),
        tokio::spawn(
            Arc::new(session::Session {
                identity: options.identity,
                driver,
                buffer,
                self_update: options.self_update.clone(),
                connected,
                builder: options.builder,
                status: status.clone(),
            })
            .run(),
        ),
    ]);
    if let Some(status) = status {
        tasks.0.push(tokio::spawn(async move {
            let capabilities = match &builder_status {
                Some(builder) => builder.capabilities().await,
                None => (false, false),
            };
            let mut tick = tokio::time::interval(Duration::from_secs(5));
            loop {
                tick.tick().await;
                let (jobs, cache) = match &builder_status {
                    Some(builder) => (
                        builder.running_jobs().unwrap_or_default(),
                        builder.usage(None).unwrap_or_default(),
                    ),
                    None => (Vec::new(), Vec::new()),
                };
                let bytes = cache
                    .iter()
                    .fold(0u64, |sum, entry| sum.saturating_add(entry.size_bytes));
                status.runtime(capabilities, jobs, (bytes, cache.len() as u64));
            }
        }));
    }
    if let Some(self_update) = options.self_update {
        tasks.0.push(tokio::spawn(async move {
            update::watchdog(self_update, welcomed).await;
        }));
    }
    shutdown.await;
    drop(tasks);
    if let Some(status) = status_shutdown {
        status.connection(false, Some("agent service stopped".into()));
    }
}

struct Tasks(Vec<tokio::task::JoinHandle<()>>);

impl Drop for Tasks {
    fn drop(&mut self) {
        for task in &self.0 {
            task.abort();
        }
    }
}

/// `tracing_subscriber` writer that prints to stdout and ships lines to the panel.
#[derive(Clone, Copy, Default)]
pub struct LogWriterFactory;

pub async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        () = ctrl_c => {}
        () = terminate => {}
    }
}
