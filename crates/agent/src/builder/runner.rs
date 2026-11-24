use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use rudder_core::Error;
use rudder_node::ops::BuildEvent;
use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};

use super::{emit, node, Builder, Events};

#[derive(Clone)]
pub(super) struct Tool {
    pub program: PathBuf,
    pub prefix: Vec<OsString>,
}

impl Tool {
    pub(super) fn new(program: &str) -> Self {
        Self {
            program: program.into(),
            prefix: Vec::new(),
        }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(&self.program);
        command.args(&self.prefix);
        command
    }
}

struct Process {
    child: Child,
    #[cfg(unix)]
    group: Option<rustix::process::Pid>,
}

impl Process {
    fn spawn(command: &mut Command) -> Result<Self, Error> {
        #[cfg(unix)]
        command.process_group(0);
        let child = command
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| node(&format!("could not start build tool: {error}")))?;
        #[cfg(unix)]
        let group = child
            .id()
            .and_then(|id| i32::try_from(id).ok())
            .and_then(rustix::process::Pid::from_raw);
        Ok(Self {
            child,
            #[cfg(unix)]
            group,
        })
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(group) = self.group {
            let _ = rustix::process::kill_process_group(group, rustix::process::Signal::KILL);
        }
        let _ = self.child.start_kill();
    }
}

fn socket(command: &mut Command, socket: Option<&Path>) {
    if let Some(path) = socket {
        command.env("DOCKER_HOST", format!("unix://{}", path.display()));
    }
}

pub(super) async fn probe(program: &Tool, args: &[&str], docker_socket: Option<&Path>) -> bool {
    let mut command = program.command();
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    socket(&mut command, docker_socket);
    let Ok(mut process) = Process::spawn(&mut command) else {
        return false;
    };
    matches!(tokio::time::timeout(Duration::from_secs(5), process.child.wait()).await, Ok(Ok(status)) if status.success())
}

pub(super) async fn remove_test_container(builder: &Builder, job: &str) {
    let name = format!("rudder-build-test-{job}");
    let _ = probe(
        &builder.docker,
        &["rm", "-f", &name],
        builder.socket.as_deref(),
    )
    .await;
}

pub(super) async fn run(
    builder: &Builder,
    argv: Vec<String>,
    cwd: &Path,
    secrets: &[&str],
    events: &Events,
) -> Result<(), Error> {
    let program = if argv.first().is_some_and(|p| p == "nixpacks") {
        &builder.nixpacks
    } else {
        &builder.docker
    };
    let mut command = program.command();
    command
        .args(&argv[1..])
        .current_dir(cwd)
        .env("DOCKER_BUILDKIT", "1")
        .env("BUILDKIT_PROGRESS", "plain")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    socket(&mut command, builder.socket.as_deref());
    let mut process = Process::spawn(&mut command)?;
    let mut stdout = process
        .child
        .stdout
        .take()
        .ok_or_else(|| node("missing build stdout"))?;
    let mut stderr = process
        .child
        .stderr
        .take()
        .ok_or_else(|| node("missing build stderr"))?;
    let mut out_redactor = Redactor::new(secrets);
    let mut err_redactor = Redactor::new(secrets);
    let (mut out_done, mut err_done) = (false, false);
    let mut out_buf = [0; 4096];
    let mut err_buf = [0; 4096];
    while !out_done || !err_done {
        let (is_err, data, eof) = tokio::select! {
            read = stdout.read(&mut out_buf), if !out_done => {
                let size = read.map_err(|_| node("could not read build output"))?;
                (false, &out_buf[..size], size == 0)
            }
            read = stderr.read(&mut err_buf), if !err_done => {
                let size = read.map_err(|_| node("could not read build output"))?;
                (true, &err_buf[..size], size == 0)
            }
        };
        let redactor = if is_err {
            &mut err_redactor
        } else {
            &mut out_redactor
        };
        let text = redactor.push(data, eof);
        if !text.is_empty() {
            emit(
                events,
                BuildEvent::Log {
                    stderr: is_err,
                    text,
                },
            )
            .await?;
        }
        if is_err {
            err_done = eof;
        } else {
            out_done = eof;
        }
    }
    let status = process
        .child
        .wait()
        .await
        .map_err(|_| node("could not wait for build tool"))?;
    if status.success() {
        Ok(())
    } else {
        Err(node("build or image test command failed; see build logs"))
    }
}

/// Retain enough raw bytes to redact secrets split across arbitrary pipe reads.
pub(super) struct Redactor {
    secrets: Vec<Vec<u8>>,
    pending: Vec<u8>,
    retain: usize,
}

impl Redactor {
    pub(super) fn new(secrets: &[&str]) -> Self {
        let mut secrets: Vec<Vec<u8>> = secrets
            .iter()
            .filter(|s| !s.is_empty())
            .map(|s| s.as_bytes().to_vec())
            .collect();
        secrets.sort_by_key(|s| std::cmp::Reverse(s.len()));
        let retain = secrets.first().map_or(0, |s| s.len() - 1);
        Self {
            secrets,
            pending: Vec::new(),
            retain,
        }
    }

    pub(super) fn push(&mut self, bytes: &[u8], eof: bool) -> String {
        self.pending.extend_from_slice(bytes);
        let mut end = if eof {
            self.pending.len()
        } else {
            // Build arguments cannot contain newlines, so a full line is safe to flush.
            self.pending.len().saturating_sub(self.retain).max(
                self.pending
                    .iter()
                    .rposition(|b| *b == b'\n')
                    .map_or(0, |n| n + 1),
            )
        };
        while end < self.pending.len() && end > 0 && self.pending[end] & 0xc0 == 0x80 {
            end -= 1;
        }
        let mut index = 0;
        let mut output = Vec::new();
        while index < end {
            if let Some(secret) = self
                .secrets
                .iter()
                .find(|s| self.pending[index..].starts_with(s))
            {
                output.extend_from_slice(b"***");
                index += secret.len();
            } else {
                output.push(self.pending[index]);
                index += 1;
            }
        }
        self.pending.drain(..index);
        String::from_utf8_lossy(&output).into_owned()
    }
}
