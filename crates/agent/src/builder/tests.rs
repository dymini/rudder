use std::fs;
use std::os::unix::fs::symlink;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use rudder_core::{BuildPlan, BuildStrategy};
use rudder_node::ops::{BuildCachePrune, BuildEvent, BuildRequest, BuildStatus};
use tempfile::TempDir;

use super::{cache, runner::Redactor, Builder};

struct Fixture {
    dir: TempDir,
    builder: Arc<Builder>,
    script: PathBuf,
}

impl Fixture {
    fn new(script: &str) -> Self {
        let dir = TempDir::new().unwrap();
        let docker = dir.path().join("docker");
        fs::write(&docker, format!("#!/bin/sh\nset -eu\n{script}\n")).unwrap();
        let mut builder =
            Builder::new(dir.path().join("builder"), Some("/test/docker.sock".into())).unwrap();
        let tool = super::runner::Tool {
            program: "/bin/sh".into(),
            prefix: vec![docker.as_os_str().to_owned()],
        };
        builder.docker = tool.clone();
        builder.nixpacks = tool;
        Self {
            dir,
            builder: Arc::new(builder),
            script: docker,
        }
    }

    fn request(&self, job: &str, strategy: BuildStrategy) -> BuildRequest {
        let src = self.builder.root.join(format!("workspaces/{job}/src"));
        fs::create_dir_all(src.join("dist")).unwrap();
        fs::write(src.join("Dockerfile"), "FROM scratch\n").unwrap();
        BuildRequest {
            job_id: job.into(),
            service_id: "service-a".into(),
            plan: BuildPlan {
                strategy,
                context: ".".into(),
                dockerfile: "Dockerfile".into(),
                publish_dir: "dist".into(),
                image: "test/app:job".into(),
                build_args: vec![("TOKEN".into(), "s3cret-value".into())],
            },
            cache_key: "branch".into(),
            cache_fallback_keys: vec!["main".into()],
            cache_limit_bytes: 20 * 1024 * 1024 * 1024,
            timeout_secs: 10,
            test_command: None,
        }
    }
}

const DOCKER: &str = r#"
test "$DOCKER_HOST" = unix:///test/docker.sock
if [ "$1" = rm ]; then exit 0; fi
if [ "$1" = buildx ] && [ "$2" = version ]; then exit 0; fi
if [ "$1" = --version ]; then exit 0; fi
if [ "$1" = build ] && [ "$2" = . ]; then
  mkdir -p .nixpacks
  printf 'FROM scratch\n' > .nixpacks/Dockerfile
  exit 0
fi
if [ "$1" = run ]; then printf 'image test passed\n'; exit 0; fi
test "$1 $2 $3" = 'buildx build --load'
test "$DOCKER_BUILDKIT" = 1
printf 'build starting\n'
printf 'prefix-s3cret-value-suffix\n' >&2
previous=''
for arg in "$@"; do
  if [ "$previous" = -f ]; then test -f "$arg"; fi
  case "$arg" in
    type=local,src=*) test -f "${arg#type=local,src=}/index.json";;
    type=local,dest=*) dest=${arg#type=local,dest=}; dest=${dest%,mode=max};;
  esac
  previous=$arg
done
printf '{}' > "$dest/index.json"
printf 'cache payload' > "$dest/blob"
"#;

async fn collect(builder: &Arc<Builder>, request: BuildRequest) -> Vec<BuildEvent> {
    let stream = builder.start(request).unwrap();
    tokio::time::timeout(
        Duration::from_secs(15),
        stream
            .map(|item| serde_json::from_slice(&item.unwrap()).unwrap())
            .collect(),
    )
    .await
    .unwrap()
}

fn status(events: &[BuildEvent]) -> BuildStatus {
    match events.last().unwrap() {
        BuildEvent::Finished { status, .. } => *status,
        _ => panic!("missing finished event"),
    }
}

#[tokio::test]
async fn builds_all_strategies_tests_redacts_and_reuses_atomic_cache() {
    let fixture = Fixture::new(DOCKER);
    assert_eq!(fixture.builder.capabilities().await, (true, false)); // nixpacks probe has no Docker socket
    for (index, strategy) in [
        BuildStrategy::Dockerfile,
        BuildStrategy::Static,
        BuildStrategy::Nixpacks,
    ]
    .into_iter()
    .enumerate()
    {
        let job = format!("job-{index}");
        let mut request = fixture.request(&job, strategy);
        request.test_command = Some("test -f /app/ready".into());
        let events = collect(&fixture.builder, request).await;
        assert_eq!(status(&events), BuildStatus::Succeeded, "{events:?}");
        let encoded = serde_json::to_string(&events).unwrap();
        assert!(!encoded.contains("s3cret-value"));
        assert!(encoded.contains("prefix-***-suffix"));
        assert!(encoded.contains("image test passed"));
        assert!(!fixture
            .builder
            .root
            .join(format!("workspaces/{job}"))
            .exists());
        let usage = fixture.builder.usage(None).unwrap();
        assert_eq!(usage.len(), 1);
        assert!(usage[0].size_bytes > 0);
        assert!(usage[0].last_used_at >= usage[0].created_at);
        assert!(fixture
            .builder
            .root
            .join("cache/service-a/branch/index.json")
            .exists());
        assert_eq!(
            fs::read_dir(fixture.builder.root.join("cache/service-a"))
                .unwrap()
                .count(),
            1
        );
    }
}

#[tokio::test]
async fn failures_do_not_replace_good_cache_and_tests_fail_the_build() {
    let fixture = Fixture::new(DOCKER);
    assert_eq!(
        status(
            &collect(
                &fixture.builder,
                fixture.request("good", BuildStrategy::Dockerfile)
            )
            .await
        ),
        BuildStatus::Succeeded
    );
    let before = fs::read(
        fixture
            .builder
            .root
            .join("cache/service-a/branch/rudder-cache.json"),
    )
    .unwrap();
    fs::write(
        &fixture.script,
        format!(
            "#!/bin/sh\nif [ \"$1\" = run ]; then exit 23; fi\n{}",
            DOCKER.replace("cache payload", "bad new payload")
        ),
    )
    .unwrap();
    let mut request = fixture.request("bad-test", BuildStrategy::Dockerfile);
    request.test_command = Some("exit 23".into());
    assert_eq!(
        status(&collect(&fixture.builder, request).await),
        BuildStatus::Failed
    );
    let after = fs::read(
        fixture
            .builder
            .root
            .join("cache/service-a/branch/rudder-cache.json"),
    )
    .unwrap();
    let before: serde_json::Value = serde_json::from_slice(&before).unwrap();
    let after: serde_json::Value = serde_json::from_slice(&after).unwrap();
    assert_eq!(before["created_at"], after["created_at"]);
    assert_eq!(
        fs::read_to_string(fixture.builder.root.join("cache/service-a/branch/blob")).unwrap(),
        "cache payload"
    );
    assert_eq!(
        fs::read_dir(fixture.builder.root.join("cache/service-a"))
            .unwrap()
            .count(),
        1
    );
    assert!(!fixture.builder.root.join("workspaces/bad-test").exists());
    fs::write(
        &fixture.script,
        "#!/bin/sh\necho 'build failed' >&2\nexit 4\n",
    )
    .unwrap();
    assert_eq!(
        status(
            &collect(
                &fixture.builder,
                fixture.request("bad-build", BuildStrategy::Dockerfile)
            )
            .await
        ),
        BuildStatus::Failed
    );
}

#[tokio::test]
async fn logs_arrive_before_exit_cancel_kills_descendants_and_cleans_workspace() {
    let fixture = Fixture::new(
        "if [ \"$1\" = rm ]; then exit 0; fi\nsleep 300 &\necho $! > child.pid\necho ready\nwait",
    );
    let request = fixture.request("cancel", BuildStrategy::Dockerfile);
    let mut stream = fixture.builder.start(request.clone()).unwrap();
    let first = tokio::time::timeout(Duration::from_secs(3), stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(String::from_utf8_lossy(&first).contains("ready"));
    let pid =
        fs::read_to_string(fixture.builder.root.join("workspaces/cancel/src/child.pid")).unwrap();
    assert!(fixture.builder.start(request).is_err());
    assert!(fixture
        .builder
        .prune(BuildCachePrune {
            service_id: Some("service-a".into()),
            ..Default::default()
        })
        .is_err());
    assert!(fixture.builder.cancel("cancel").unwrap());
    let events: Vec<BuildEvent> = stream
        .map(|event| serde_json::from_slice(&event.unwrap()).unwrap())
        .collect()
        .await;
    assert_eq!(status(&events), BuildStatus::Cancelled);
    assert!(!fixture.builder.root.join("workspaces/cancel").exists());
    // Wait for signal delivery; an exited descendant may remain a zombie until reaped.
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            match fs::read_to_string(format!("/proc/{}/stat", pid.trim())) {
                Ok(stat) if stat.split_whitespace().nth(2) != Some("Z") => {
                    tokio::task::yield_now().await
                }
                _ => break,
            }
        }
    })
    .await
    .unwrap();
    assert!(!fixture.builder.cancel("cancel").unwrap());
}

#[tokio::test]
async fn timeout_and_lost_reader_cancel_work() {
    let fixture = Fixture::new("echo ready\nsleep 300");
    let mut request = fixture.request("timeout", BuildStrategy::Dockerfile);
    request.timeout_secs = 1;
    assert_eq!(
        status(&collect(&fixture.builder, request).await),
        BuildStatus::TimedOut
    );
    let mut stream = fixture
        .builder
        .start(fixture.request("lost", BuildStrategy::Dockerfile))
        .unwrap();
    stream.next().await.unwrap().unwrap();
    drop(stream);
    // Synchronize on task completion, not a timing assumption about process shutdown.
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if fixture.builder.state().unwrap().jobs.is_empty() {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(!fixture.builder.root.join("workspaces/lost").exists());
}

#[tokio::test]
async fn capabilities_empty_usage_and_missing_build_tool() {
    let fixture = Fixture::new("exit 0");
    assert_eq!(fixture.builder.capabilities().await, (true, true));
    assert!(fixture.builder.usage(None).unwrap().is_empty());
    fs::remove_file(&fixture.script).unwrap();
    assert_eq!(fixture.builder.capabilities().await, (false, false));
    let events = collect(
        &fixture.builder,
        fixture.request("missing-tool", BuildStrategy::Dockerfile),
    )
    .await;
    assert_eq!(status(&events), BuildStatus::Failed);
    assert!(!fixture
        .builder
        .root
        .join("workspaces/missing-tool")
        .exists());
    let builder = Builder::new("relative-node/builder".into(), None).unwrap();
    assert!(builder.root.is_absolute());
}

#[tokio::test]
async fn different_services_can_build_concurrently() {
    let fixture = Fixture::new("echo ready\nsleep 300");
    let first = fixture.request("first", BuildStrategy::Dockerfile);
    let mut second = fixture.request("second", BuildStrategy::Dockerfile);
    second.service_id = "service-b".into();
    let mut first = fixture.builder.start(first).unwrap();
    let mut second = fixture.builder.start(second).unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        assert!(first.next().await.unwrap().is_ok());
        assert!(second.next().await.unwrap().is_ok());
    })
    .await
    .unwrap();
    assert_eq!(fixture.builder.state().unwrap().jobs.len(), 2);
    fixture.builder.cancel("first").unwrap();
    fixture.builder.cancel("second").unwrap();
    let (first, second) = tokio::join!(first.collect::<Vec<_>>(), second.collect::<Vec<_>>());
    for events in [first, second] {
        let events: Vec<BuildEvent> = events
            .into_iter()
            .map(|e| serde_json::from_slice(&e.unwrap()).unwrap())
            .collect();
        assert_eq!(status(&events), BuildStatus::Cancelled);
    }
}

#[tokio::test]
async fn cache_fallbacks_prune_scopes_and_automatic_lru_limit() {
    let fixture = Fixture::new(DOCKER);
    let mut request = fixture.request("main", BuildStrategy::Dockerfile);
    request.cache_key = "main".into();
    collect(&fixture.builder, request).await;
    collect(
        &fixture.builder,
        fixture.request("branch", BuildStrategy::Dockerfile),
    )
    .await;
    let usage = fixture.builder.usage(Some("service-a")).unwrap();
    assert_eq!(usage.len(), 2);
    let result = fixture
        .builder
        .prune(BuildCachePrune {
            service_id: Some("service-a".into()),
            cache_key: Some("branch".into()),
            limit_bytes: None,
        })
        .unwrap();
    assert!(result.reclaimed_bytes > 0);
    assert_eq!(result.entries[0].cache_key, "main");
    let mut request = fixture.request("zero", BuildStrategy::Dockerfile);
    request.cache_limit_bytes = 0;
    assert_eq!(
        status(&collect(&fixture.builder, request).await),
        BuildStatus::Succeeded
    );
    assert!(fixture.builder.usage(None).unwrap().is_empty());
    assert_eq!(
        fixture
            .builder
            .prune(Default::default())
            .unwrap()
            .reclaimed_bytes,
        0
    );
    assert!(fixture
        .builder
        .prune(BuildCachePrune {
            cache_key: Some("main".into()),
            ..Default::default()
        })
        .is_err());
}

#[test]
fn rejects_unsafe_paths_and_missing_workspaces_without_running_commands() {
    let fixture = Fixture::new("exit 99");
    let mut request = fixture.request("valid", BuildStrategy::Dockerfile);
    request.job_id = "../escape".into();
    assert!(fixture.builder.start(request.clone()).is_err());
    request.job_id = "missing".into();
    assert!(fixture.builder.start(request.clone()).is_err());
    request.job_id = "valid".into();
    request.plan.context = "../escape".into();
    assert!(fixture.builder.start(request.clone()).is_err());
    request.plan.context = ".".into();
    request.cache_key = "../../escape".into();
    assert!(fixture.builder.start(request.clone()).is_err());
    request.cache_key = "main".into();
    fs::remove_file(fixture.builder.root.join("workspaces/valid/src/Dockerfile")).unwrap();
    symlink(
        fixture.dir.path(),
        fixture.builder.root.join("workspaces/valid/src/Dockerfile"),
    )
    .unwrap();
    assert!(fixture.builder.start(request).is_err());
    fs::create_dir_all(fixture.builder.root.join("cache")).unwrap();
    symlink(
        fixture.dir.path(),
        fixture.builder.root.join("cache/service-a"),
    )
    .unwrap();
    assert!(fixture.builder.usage(None).is_err());
}

#[tokio::test]
async fn workspace_cleanup_cancels_an_active_job_and_is_idempotent() {
    let fixture = Fixture::new("echo ready\nsleep 300");
    let mut stream = fixture
        .builder
        .start(fixture.request("cleanup", BuildStrategy::Dockerfile))
        .unwrap();
    assert!(stream.next().await.unwrap().is_ok());
    fixture.builder.cleanup_workspace("cleanup").await.unwrap();
    assert!(!fixture.builder.root.join("workspaces/cleanup").exists());
    fixture.builder.cleanup_workspace("cleanup").await.unwrap();
    assert!(fixture
        .builder
        .cleanup_workspace("../escape")
        .await
        .is_err());
    let events: Vec<_> = stream
        .map(|e| serde_json::from_slice::<BuildEvent>(&e.unwrap()).unwrap())
        .collect()
        .await;
    assert_eq!(status(&events), BuildStatus::Cancelled);
}

#[tokio::test]
async fn uploads_are_confined_to_agent_builder_storage_and_running_jobs_are_visible() {
    let fixture = Fixture::new("echo ready\nsleep 300");
    let src = fixture.dir.path().join("upload");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("Dockerfile"), "FROM scratch\n").unwrap();
    let archive = rudder_node::tree::pack_files(&src, &["Dockerfile".into()]).unwrap();
    let input = || {
        Box::pin(futures_util::stream::iter(vec![Ok(bytes::Bytes::from(
            archive.clone(),
        ))])) as rudder_core::ByteStream
    };
    fixture
        .builder
        .upload_tree("builder/workspaces/upload/src", input())
        .await
        .unwrap();
    let manifest = fixture
        .builder
        .root
        .join("workspaces/upload")
        .join(rudder_node::tree::manifest_relative("src"));
    assert!(manifest.is_file());
    fixture.builder.cleanup_workspace("upload").await.unwrap();
    assert!(!manifest.exists());
    fixture
        .builder
        .upload_tree("builder/workspaces/upload/src", input())
        .await
        .unwrap();
    assert!(fixture
        .builder
        .root
        .join("workspaces/upload/src/Dockerfile")
        .is_file());
    assert!(fixture
        .builder
        .upload_tree("builder/workspaces/../../outside/src", input())
        .await
        .is_err());
    let mut stream = fixture
        .builder
        .start(fixture.request("active", BuildStrategy::Dockerfile))
        .unwrap();
    stream.next().await.unwrap().unwrap();
    assert_eq!(fixture.builder.running_jobs().unwrap(), vec!["active"]);
    assert!(fixture
        .builder
        .upload_tree("builder/workspaces/active/src", input())
        .await
        .is_err());
    fixture.builder.cleanup_workspace("active").await.unwrap();
    assert!(fixture.builder.running_jobs().unwrap().is_empty());
}

#[test]
fn redactor_protects_short_embedded_and_split_secrets_and_streams_lines() {
    let mut redactor = Redactor::new(&["s3cret-value", "x"]);
    let mut output = redactor.push(b"first line\n", false);
    assert_eq!(output, "first line\n");
    output.push_str(&redactor.push(b"prefix-s3cr", false));
    output.push_str(&redactor.push(b"et-value-suffix x\n", false));
    output.push_str(&redactor.push(b"", true));
    assert!(!output.contains("s3cret-value"));
    assert!(!output.contains('x'));
    assert!(output.contains("***"));
}

#[test]
fn lru_keeps_recent_entries_and_ignores_active_service_caches() {
    let fixture = Fixture::new(DOCKER);
    for (service, key, time) in [
        ("service-a", "old", 1),
        ("service-a", "new", 3),
        ("service-b", "active", 0),
    ] {
        let dir = fixture.builder.root.join(format!("cache/{service}/{key}"));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("index.json"), "{}").unwrap();
        fs::write(
            dir.join("rudder-cache.json"),
            serde_json::to_vec(&rudder_node::ops::BuildCacheEntry {
                service_id: service.into(),
                cache_key: key.into(),
                size_bytes: 2,
                last_used_at: time,
                created_at: time,
            })
            .unwrap(),
        )
        .unwrap();
    }
    let entries = fixture.builder.usage(None).unwrap();
    let keep = entries
        .iter()
        .filter(|e| e.cache_key != "old")
        .map(|e| e.size_bytes)
        .sum();
    let result = cache::prune(
        &fixture.builder.root,
        &BuildCachePrune {
            limit_bytes: Some(keep),
            ..Default::default()
        },
        &std::collections::HashSet::from(["service-b".into()]),
    )
    .unwrap();
    assert_eq!(result.entries.len(), 2);
    assert!(result.entries.iter().all(|e| e.cache_key != "old"));
}
