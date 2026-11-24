//! Atomic, credential-free status snapshots for local operator inspection.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::identity::AgentMeta;

const STATUS: &str = "status.json";
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct AgentSettings {
    pub builder: bool,
}

impl AgentSettings {
    pub fn load(dir: &Path) -> anyhow::Result<Self> {
        match fs::read(dir.join("config.json")) {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error.into()),
        }
    }
}

#[derive(Clone, Deserialize, Serialize)]
pub struct AgentStatus {
    pub panel_url: Option<String>,
    pub server_id: Option<String>,
    pub enrolled: bool,
    pub connected: bool,
    pub last_connected_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub builder: bool,
    pub buildx: bool,
    pub nixpacks: bool,
    pub arch: String,
    pub running_jobs: Vec<String>,
    pub cache_bytes: u64,
    pub cache_entries: u64,
    pub updated_at: DateTime<Utc>,
}

impl AgentStatus {
    fn empty(builder: bool) -> Self {
        Self {
            panel_url: None,
            server_id: None,
            enrolled: false,
            connected: false,
            last_connected_at: None,
            last_error: None,
            builder,
            buildx: false,
            nixpacks: false,
            arch: std::env::consts::ARCH.into(),
            running_jobs: Vec::new(),
            cache_bytes: 0,
            cache_entries: 0,
            updated_at: Utc::now(),
        }
    }

    pub fn healthy(&self) -> bool {
        self.enrolled && self.connected
    }

    pub fn render(&self) -> String {
        format!("Panel: {}\nServer: {}\nEnrollment: {}\nConnection: {}\nLast connect: {}\nLast error: {}\nBuilder: {}\nCapabilities: buildx={}, nixpacks={}\nArchitecture: {}\nRunning jobs: {}\nCache: {} bytes ({} entries)\n",
            self.panel_url.as_deref().unwrap_or("not enrolled"), self.server_id.as_deref().unwrap_or("not enrolled"),
            if self.enrolled { "enrolled" } else { "not enrolled" }, if self.connected { "connected" } else { "disconnected" },
            self.last_connected_at.map(|at| at.to_rfc3339()).unwrap_or_else(|| "never".into()),
            self.last_error.as_deref().unwrap_or("none"), self.builder, self.buildx, self.nixpacks,
            self.arch, self.running_jobs.len(), self.cache_bytes, self.cache_entries)
    }
}

pub fn read(dir: &Path) -> anyhow::Result<AgentStatus> {
    let settings = AgentSettings::load(dir)?;
    let meta = match fs::read(dir.join("agent.json")) {
        Ok(bytes) => Some(serde_json::from_slice::<AgentMeta>(&bytes)?),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    let Some(meta) = meta else {
        let mut status = AgentStatus::empty(settings.builder);
        status.last_error = Some("not enrolled; run rudder-agent enroll first".into());
        return Ok(status);
    };
    let enrolled = dir.join("cert.pem").is_file()
        && dir.join("key.pem").is_file()
        && dir.join("ca.pem").is_file();
    let mut status = match fs::read(dir.join(STATUS)) {
        Ok(bytes) => serde_json::from_slice::<AgentStatus>(&bytes).unwrap_or_else(|_| {
            let mut value = AgentStatus::empty(settings.builder);
            value.last_error = Some("invalid status file; restart rudder-agent".into());
            value
        }),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let mut value = AgentStatus::empty(settings.builder);
            value.last_error = Some("no running-service status has been recorded".into());
            value
        }
        Err(error) => return Err(error.into()),
    };
    if status.server_id.as_deref() != Some(&meta.server_id) || !enrolled {
        status.connected = false;
        if !enrolled {
            status.last_error = Some("enrollment identity is incomplete; enroll again".into());
        }
    }
    let age = Utc::now() - status.updated_at;
    if age.num_seconds() > 30 || age.num_seconds() < -30 {
        status.connected = false;
        status.last_error = Some("status is stale; the agent service may not be running".into());
    }
    status.enrolled = enrolled;
    status.server_id = Some(meta.server_id);
    status.panel_url = Some(meta.panel_url);
    Ok(status)
}

pub(crate) struct StatusFile {
    dir: PathBuf,
    state: Mutex<AgentStatus>,
}

impl StatusFile {
    pub(crate) fn new(dir: &Path, meta: &AgentMeta, builder: bool) -> io::Result<Self> {
        let mut state = AgentStatus::empty(builder);
        if let Ok(previous) = read(dir) {
            if previous.server_id.as_deref() == Some(&meta.server_id) {
                state.last_connected_at = previous.last_connected_at;
            }
        }
        state.enrolled = true;
        state.server_id = Some(meta.server_id.clone());
        state.panel_url = Some(meta.panel_url.clone());
        state.last_error = Some("connecting to the panel".into());
        let file = Self {
            dir: dir.to_owned(),
            state: Mutex::new(state),
        };
        file.update(|_| {})?;
        Ok(file)
    }

    fn update(&self, change: impl FnOnce(&mut AgentStatus)) -> io::Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| io::Error::other("status lock poisoned"))?;
        change(&mut state);
        state.updated_at = Utc::now();
        fs::create_dir_all(&self.dir)?;
        let tmp = self.dir.join(format!(
            ".status-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let result = (|| {
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                options.mode(0o600);
            }
            let mut file = options.open(&tmp)?;
            file.write_all(&serde_json::to_vec(&*state)?)?;
            file.sync_all()?;
            fs::rename(&tmp, self.dir.join(STATUS))
        })();
        if result.is_err() {
            let _ = fs::remove_file(tmp);
        }
        result
    }

    pub(crate) fn connection(&self, connected: bool, error: Option<String>) {
        if let Err(error) = self.update(|state| {
            state.connected = connected;
            state.last_error = error;
            if connected {
                state.last_connected_at = Some(Utc::now());
            }
        }) {
            tracing::warn!(%error, "could not update agent status");
        }
    }

    pub(crate) fn runtime(&self, capabilities: (bool, bool), jobs: Vec<String>, cache: (u64, u64)) {
        if let Err(error) = self.update(|state| {
            state.buildx = capabilities.0;
            state.nixpacks = capabilities.1;
            state.running_jobs = jobs;
            state.cache_bytes = cache.0;
            state.cache_entries = cache.1;
        }) {
            tracing::warn!(%error, "could not update agent status");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_missing_connected_disconnected_and_stale_status_and_writes_atomically() {
        let dir = std::env::temp_dir().join(format!(
            "rudder-status-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let empty = read(&dir).unwrap();
        assert!(!empty.healthy());
        assert!(empty.render().contains("not enrolled"));
        let meta = AgentMeta {
            panel_url: "https://panel.example".into(),
            server_id: "server-1".into(),
            grpc_url: "https://panel.example:7443".into(),
            tls_name: "rudder-panel".into(),
        };
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("agent.json"), serde_json::to_vec(&meta).unwrap()).unwrap();
        for file in ["cert.pem", "key.pem", "ca.pem"] {
            fs::write(dir.join(file), "test identity").unwrap();
        }
        let file = StatusFile::new(&dir, &meta, true).unwrap();
        file.connection(true, None);
        file.runtime((true, true), vec!["job-1".into()], (123, 2));
        let status = read(&dir).unwrap();
        assert!(status.healthy());
        assert!(status.builder);
        assert_eq!(status.cache_bytes, 123);
        assert!(status.render().contains("Running jobs: 1"));
        assert!(serde_json::to_value(&status).unwrap()["connected"]
            .as_bool()
            .unwrap());
        assert_eq!(
            fs::read_dir(&dir)
                .unwrap()
                .filter_map(Result::ok)
                .filter(|f| f.file_name().to_string_lossy().starts_with(".status-"))
                .count(),
            0
        );
        std::thread::scope(|scope| {
            let reader = scope.spawn(|| {
                for _ in 0..1000 {
                    let value: AgentStatus =
                        serde_json::from_slice(&fs::read(dir.join(STATUS)).unwrap()).unwrap();
                    assert!(value.connected);
                }
            });
            for bytes in 0..20 {
                file.runtime((true, false), Vec::new(), (bytes, 1));
            }
            reader.join().unwrap();
        });
        file.connection(false, Some("connection refused".into()));
        assert!(read(&dir).unwrap().render().contains("connection refused"));
        assert!(read(&dir).unwrap().last_connected_at.is_some());
        file.update(|s| {
            s.connected = true;
            s.updated_at = Utc::now();
        })
        .unwrap();
        let mut stale: AgentStatus =
            serde_json::from_slice(&fs::read(dir.join(STATUS)).unwrap()).unwrap();
        stale.updated_at -= chrono::Duration::seconds(31);
        fs::write(dir.join(STATUS), serde_json::to_vec(&stale).unwrap()).unwrap();
        assert!(!read(&dir).unwrap().healthy());
        fs::write(dir.join(STATUS), "invalid").unwrap();
        assert!(!read(&dir).unwrap().healthy());
        fs::write(dir.join("config.json"), "{\"builder\":true}").unwrap();
        assert!(AgentSettings::load(&dir).unwrap().builder);
        fs::write(dir.join("config.json"), "invalid").unwrap();
        assert!(AgentSettings::load(&dir).is_err());
        let _ = fs::remove_dir_all(dir);
    }
}
