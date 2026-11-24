//! Opt-in Linux smoke test. Uses a dedicated BuildKit instance, never a shared cache prune.
//! Set BUILDX_BUILDER=rudder-builder-smoke-<unique> and RUDDER_BUILDER_SMOKE_DIR=<empty-dir>.
//! cargo test -p rudder-agent --test builder_docker -- --ignored --nocapture

#![cfg(target_os = "linux")]

use std::fs::{self, File};
use std::io::Write;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use rudder_agent::builder::Builder;
use rudder_core::{BuildPlan, BuildStrategy};
use rudder_node::ops::{BuildCachePrune, BuildEvent, BuildRequest, BuildStatus};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::watch;

const DOCKERFILE: &str = "FROM busybox:1.37.0\nRUN printf 'ready\\n' > /ready\n";
const SERVICE: &str = "real-docker-smoke";

fn docker(args: &[&str]) -> Output {
    Command::new("docker").args(args).output().unwrap()
}

fn docker_ok(args: &[&str]) -> String {
    let output = docker(args);
    assert!(
        output.status.success(),
        "docker {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

struct Fixture {
    root: PathBuf,
    builder: Arc<Builder>,
    buildkit: String,
    images: Vec<String>,
}

impl Fixture {
    fn new() -> Self {
        let root = PathBuf::from(std::env::var("RUDDER_BUILDER_SMOKE_DIR").unwrap());
        assert!(!root.exists(), "use a fresh smoke evidence directory");
        fs::create_dir_all(&root).unwrap();
        let buildkit = std::env::var("BUILDX_BUILDER").unwrap();
        assert!(buildkit.starts_with("rudder-builder-smoke-"));
        assert!(!docker(&["buildx", "inspect", &buildkit]).status.success());
        docker_ok(&[
            "buildx",
            "create",
            "--name",
            &buildkit,
            "--driver",
            "docker-container",
        ]);
        let fixture = Self {
            builder: Arc::new(Builder::new(root.join("builder"), None).unwrap()),
            root,
            buildkit,
            images: Vec::new(),
        };
        docker_ok(&["buildx", "inspect", "--bootstrap", &fixture.buildkit]);
        fixture
    }

    fn request(&mut self, job: &str, branch: &str, dockerfile: &str) -> BuildRequest {
        let src = self.root.join(format!("builder/workspaces/{job}/src"));
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("Dockerfile"), dockerfile).unwrap();
        let image = format!("rudder-builder-smoke:{}-{job}", std::process::id());
        self.images.push(image.clone());
        BuildRequest {
            job_id: job.into(),
            service_id: SERVICE.into(),
            plan: BuildPlan {
                strategy: BuildStrategy::Dockerfile,
                context: ".".into(),
                dockerfile: "Dockerfile".into(),
                publish_dir: "dist".into(),
                image,
                build_args: Vec::new(),
            },
            cache_key: branch.into(),
            cache_fallback_keys: vec!["main".into()],
            cache_limit_bytes: 20 * 1024 * 1024 * 1024,
            timeout_secs: 120,
            test_command: Some("test \"$(cat /ready)\" = ready; echo image-test-passed".into()),
        }
    }

    fn cache(&self, key: &str) -> PathBuf {
        self.root.join(format!("builder/cache/{SERVICE}/{key}"))
    }

    fn cleaned(&self, job: &str) {
        assert!(!self.root.join(format!("builder/workspaces/{job}")).exists());
        assert!(self.builder.running_jobs().unwrap().is_empty());
        for entry in fs::read_dir(self.cache("main").parent().unwrap()).unwrap() {
            assert!(!entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".tmp-"));
        }
        println!("{job}: workspace and temporary export cleaned; no running jobs");
    }

    fn cold_buildkit(&self) {
        docker_ok(&[
            "buildx",
            "prune",
            "--builder",
            &self.buildkit,
            "--all",
            "--force",
        ]);
        let usage = docker_ok(&["buildx", "du", "--builder", &self.buildkit]);
        assert!(
            usage
                .split_whitespace()
                .collect::<String>()
                .contains("Total:0B"),
            "{usage}"
        );
        println!(
            "isolated BuildKit internal cache after prune: {}",
            usage.trim()
        );
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        for image in &self.images {
            let _ = docker(&["image", "rm", "--force", image]);
        }
        let _ = docker(&["buildx", "rm", "--force", &self.buildkit]);
    }
}

async fn collect(
    builder: Arc<Builder>,
    request: BuildRequest,
    root: PathBuf,
) -> (BuildStatus, String) {
    let job = request.job_id.clone();
    let mut log = File::create(root.join(format!("{job}.jsonl"))).unwrap();
    let mut stream = builder.start(request).unwrap();
    let mut text = String::new();
    let mut result = None;
    while let Some(event) = stream.next().await {
        let bytes = event.unwrap();
        log.write_all(&bytes).unwrap();
        log.write_all(b"\n").unwrap();
        match serde_json::from_slice::<BuildEvent>(&bytes).unwrap() {
            BuildEvent::Log { text: line, .. } => {
                print!("[{job}] {line}");
                text.push_str(&line);
            }
            BuildEvent::Finished { status, error, .. } => {
                println!("{job}: finished={status:?} error={error:?}");
                result = Some(status);
            }
        }
    }
    (result.unwrap(), text)
}

struct CacheObserver {
    stop: Arc<AtomicBool>,
    reader: Option<std::thread::JoinHandle<usize>>,
}

impl CacheObserver {
    fn start(path: PathBuf) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let done = stop.clone();
        let reader = std::thread::spawn(move || {
            let mut count = 0;
            while !done.load(Ordering::Relaxed) {
                assert!(
                    fs::metadata(&path).unwrap().is_dir(),
                    "cache target vanished"
                );
                count += 1;
                std::thread::sleep(Duration::from_millis(1));
            }
            count
        });
        Self {
            stop,
            reader: Some(reader),
        }
    }

    fn finish(&mut self) -> usize {
        self.stop.store(true, Ordering::Relaxed);
        self.reader.take().unwrap().join().unwrap()
    }
}

impl Drop for CacheObserver {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

struct Heartbeat {
    url: String,
    ticks: watch::Receiver<u64>,
    task: tokio::task::JoinHandle<()>,
}

impl Heartbeat {
    async fn new(job: &str) -> Self {
        let gateway = docker_ok(&[
            "network",
            "inspect",
            "bridge",
            "--format",
            "{{(index .IPAM.Config 0).Gateway}}",
        ]);
        let listener = TcpListener::bind((gateway.trim(), 0)).await.unwrap();
        let address = listener.local_addr().unwrap();
        let (count, ticks) = watch::channel(0);
        let expected = format!("GET /{job} HTTP/");
        let task = tokio::spawn(async move {
            loop {
                let (stream, _) = listener.accept().await.unwrap();
                let mut stream = BufReader::new(stream);
                let mut request = String::new();
                let read =
                    tokio::time::timeout(Duration::from_secs(1), stream.read_line(&mut request))
                        .await;
                if !matches!(read, Ok(Ok(size)) if size > 0) || !request.starts_with(&expected) {
                    continue;
                }
                count.send_modify(|value| *value += 1);
                let _ = stream
                    .get_mut()
                    .write_all(b"HTTP/1.0 200 OK\r\nContent-Length: 0\r\n\r\n")
                    .await;
            }
        });
        Self {
            url: format!("http://{address}/{job}"),
            ticks,
            task,
        }
    }

    fn command(&self) -> String {
        format!(
            "while :; do wget -q -O /dev/null {} || exit 71; sleep 0.1; done",
            self.url
        )
    }

    async fn started(&mut self) {
        tokio::time::timeout(
            Duration::from_secs(30),
            self.ticks.wait_for(|count| *count >= 2),
        )
        .await
        .unwrap()
        .unwrap();
    }

    async fn stopped(&mut self, job: &str) {
        tokio::time::timeout(Duration::from_secs(8), async {
            loop {
                self.ticks.borrow_and_update();
                if tokio::time::timeout(Duration::from_secs(2), self.ticks.changed())
                    .await
                    .is_err()
                {
                    break;
                }
            }
        })
        .await
        .expect("Docker execution kept sending heartbeats after cancellation");
        println!(
            "{job}: real Docker execution sent {} heartbeats before stopping; quiet for 2s",
            *self.ticks.borrow()
        );
    }
}

impl Drop for Heartbeat {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn bytes(path: &Path) -> u64 {
    let metadata = fs::symlink_metadata(path).unwrap();
    if metadata.is_dir() {
        fs::read_dir(path)
            .unwrap()
            .map(|entry| bytes(&entry.unwrap().path()))
            .sum()
    } else {
        assert!(metadata.is_file());
        metadata.len()
    }
}

fn cached_run(logs: &str) -> bool {
    logs.lines()
        .find(|line| line.contains("RUN printf 'ready"))
        .and_then(|line| line.split_whitespace().next())
        .is_some_and(|step| logs.contains(&format!("{step} CACHED")))
}

fn process_state(pid: u32) -> Option<(String, String)> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let fields: Vec<_> = stat.rsplit_once(')')?.1.split_whitespace().collect();
    Some((fields.first()?.to_string(), fields.get(19)?.to_string()))
}

fn execution_pids(
    fixture: &Fixture,
    heartbeat: &Heartbeat,
    job: &str,
    in_test: bool,
) -> Vec<(u32, String)> {
    let pids = if in_test {
        vec![docker_ok(&[
            "container",
            "inspect",
            "--format",
            "{{.State.Pid}}",
            &format!("rudder-build-test-{job}"),
        ])
        .trim()
        .parse::<u32>()
        .unwrap()]
    } else {
        docker_ok(&[
            "top",
            &format!("buildx_buildkit_{}0", fixture.buildkit),
            "-eo",
            "pid,args",
        ])
        .lines()
        .filter(|line| line.contains(&heartbeat.url))
        .map(|line| {
            line.split_whitespace()
                .next()
                .unwrap()
                .parse::<u32>()
                .unwrap()
        })
        .collect()
    };
    assert!(
        !pids.is_empty(),
        "no live Docker executor PID found for {job}"
    );
    let processes = pids
        .into_iter()
        .map(|pid| {
            let (state, start) = process_state(pid).expect("executor exited before cancellation");
            assert_ne!(state, "Z");
            (pid, start)
        })
        .collect::<Vec<_>>();
    println!("{job}: live host PIDs before cancellation: {processes:?}");
    processes
}

async fn processes_stopped(job: &str, pids: &[(u32, String)]) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let alive = pids.iter().any(|(pid, start)| {
                process_state(*pid)
                    .is_some_and(|(state, current)| state != "Z" && current == *start)
            });
            if !alive {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("Docker executor PID survived cancellation");
    println!("{job}: all captured Docker executor host PIDs exited");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires real Docker, Buildx, network access and an isolated BUILDX_BUILDER"]
async fn real_docker_build_cache_tests_cancel_timeout_and_prune() {
    let mut fixture = Fixture::new();
    println!("evidence directory: {}", fixture.root.display());
    assert!(fixture.builder.capabilities().await.0);
    let mut first_inode = 0;
    for (job, branch) in [
        ("first", "main"),
        ("second", "main"),
        ("new-branch", "feature"),
    ] {
        if job == "new-branch" {
            assert!(!fixture.cache("feature").exists());
            assert_eq!(fixture.builder.usage(None).unwrap().len(), 1);
        }
        let request = fixture.request(job, branch, DOCKERFILE);
        let image = request.plan.image.clone();
        let mut observer = (job == "second").then(|| CacheObserver::start(fixture.cache("main")));
        let (status, logs) = collect(fixture.builder.clone(), request, fixture.root.clone()).await;
        assert_eq!(status, BuildStatus::Succeeded);
        assert!(logs.contains("image-test-passed"));
        println!(
            "{job}: image loaded: {}",
            docker_ok(&["image", "inspect", &image, "--format", "{{.Id}}"]).trim()
        );
        let cache = fixture.cache(branch);
        assert!(cache.join("index.json").is_file());
        assert!(cache.join("blobs/sha256").is_dir());
        assert!(fs::read_dir(cache.join("blobs/sha256")).unwrap().count() > 0);
        if job == "first" {
            first_inode = fs::metadata(&cache).unwrap().ino();
            fs::write(cache.join("old-export-marker"), "old cache").unwrap();
        } else {
            assert!(cached_run(&logs), "RUN was not cached: {logs}");
            assert!(
                logs.contains("importing cache manifest from local:"),
                "{logs}"
            );
        }
        if let Some(observer) = &mut observer {
            let reads = observer.finish();
            let inode = fs::metadata(&cache).unwrap().ino();
            assert_ne!(first_inode, inode);
            assert!(!cache.join("old-export-marker").exists());
            println!("second: atomic cache replacement inode {first_inode} -> {inode}; {reads} concurrent directory reads without a missing target");
        }
        if job == "new-branch" {
            println!("new-branch: feature cache was absent; imported only default-branch main cache into an empty internal BuildKit store; RUN was CACHED");
        }
        fixture.cleaned(job);
        if job != "new-branch" {
            fixture.cold_buildkit();
        }
    }

    let before = fs::metadata(fixture.cache("main")).unwrap().ino();
    let mut request = fixture.request("failed-test", "main", DOCKERFILE);
    request.test_command = Some("echo expected-test-failure; exit 23".into());
    let (status, logs) = collect(fixture.builder.clone(), request, fixture.root.clone()).await;
    assert_eq!(status, BuildStatus::Failed);
    assert!(logs.contains("expected-test-failure"));
    assert_eq!(before, fs::metadata(fixture.cache("main")).unwrap().ino());
    println!("failed-test: nonzero image test rejected; good cache inode unchanged");
    fixture.cleaned("failed-test");

    for (job, in_test, cancel) in [
        ("cancel-run", false, true),
        ("timeout-run", false, false),
        ("cancel-test", true, true),
        ("timeout-test", true, false),
    ] {
        let mut heartbeat = Heartbeat::new(job).await;
        let dockerfile = if in_test {
            DOCKERFILE.to_owned()
        } else {
            format!("{DOCKERFILE}RUN {}\n", heartbeat.command())
        };
        let mut request = fixture.request(job, "main", &dockerfile);
        request.test_command = in_test.then(|| heartbeat.command());
        request.timeout_secs = if cancel { 60 } else { 15 };
        let running = tokio::spawn(collect(
            fixture.builder.clone(),
            request,
            fixture.root.clone(),
        ));
        heartbeat.started().await;
        let pids = execution_pids(&fixture, &heartbeat, job, in_test);
        if cancel {
            assert!(fixture.builder.cancel(job).unwrap());
        }
        let (status, _) = tokio::time::timeout(Duration::from_secs(30), running)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            status,
            if cancel {
                BuildStatus::Cancelled
            } else {
                BuildStatus::TimedOut
            }
        );
        processes_stopped(job, &pids).await;
        heartbeat.stopped(job).await;
        assert!(
            !docker(&["container", "inspect", &format!("rudder-build-test-{job}")])
                .status
                .success()
        );
        fixture.cleaned(job);
    }

    let entries = fixture.builder.usage(Some(SERVICE)).unwrap();
    assert_eq!(entries.len(), 2);
    let total: u64 = entries
        .iter()
        .map(|entry| {
            let measured = bytes(&fixture.cache(&entry.cache_key));
            assert_eq!(entry.size_bytes, measured);
            println!(
                "cache {}: reported={} measured={} bytes",
                entry.cache_key, entry.size_bytes, measured
            );
            measured
        })
        .sum();
    let feature = entries
        .iter()
        .find(|entry| entry.cache_key == "feature")
        .unwrap();
    let pruned = fixture
        .builder
        .prune(BuildCachePrune {
            service_id: Some(SERVICE.into()),
            cache_key: Some("feature".into()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(pruned.reclaimed_bytes, feature.size_bytes);
    assert_eq!(pruned.remaining_bytes, 0);
    assert_eq!(pruned.entries.len(), 1);
    assert_eq!(
        bytes(&fixture.cache("main")),
        total - pruned.reclaimed_bytes
    );
    println!(
        "scoped prune: reclaimed={} bytes; matching remaining={} bytes; main preserved",
        pruned.reclaimed_bytes, pruned.remaining_bytes
    );
    let pruned = fixture
        .builder
        .prune(BuildCachePrune {
            limit_bytes: Some(0),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(pruned.reclaimed_bytes, total - feature.size_bytes);
    assert_eq!(pruned.remaining_bytes, 0);
    assert!(pruned.entries.is_empty());
    assert!(fixture.builder.usage(None).unwrap().is_empty());
    println!(
        "LRU prune to zero: reclaimed={} bytes; remaining={} bytes; catalog empty",
        pruned.reclaimed_bytes, pruned.remaining_bytes
    );
}
