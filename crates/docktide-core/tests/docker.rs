//! Tests against the real Docker Engine. Run with `cargo test -- --ignored`.
//! They must never touch containers without the `io.docktide.test=true` label.

use std::collections::HashMap;
use std::time::Duration;

use bollard::models::{
    ContainerCreateBody, HealthConfig, HostConfig, PortBinding, VolumeCreateRequest,
};
use bollard::query_parameters::{
    CreateContainerOptions, CreateImageOptions, RemoveContainerOptions, RemoveVolumeOptions,
};
use bollard::{API_DEFAULT_VERSION, Docker};
use docktide_core::{
    ComposeInfo, ConnectionStatus, ContainerState, DockerError, DockerService, KeyValue,
    LiveUpdate, LogEvent, LogLine, LogStream, MountType, Protocol, StatsSample, endpoint,
};
use futures_util::TryStreamExt;

const IMAGE: &str = "busybox:latest";

/// A labelled container owned by this test, force-removed on drop.
struct TestContainer {
    docker: Docker,
    id: String,
    name: String,
}

impl TestContainer {
    async fn create(test: &str) -> Self {
        Self::create_with(test, ContainerCreateBody::default()).await
    }

    /// Fields set in `body` replace the defaults; its labels are added to the test label.
    async fn create_with(test: &str, body: ContainerCreateBody) -> Self {
        let endpoint = endpoint::resolve_from_system().unwrap();
        let docker = Docker::connect_with_socket(&endpoint.uri, 120, API_DEFAULT_VERSION).unwrap();
        if docker.inspect_image(IMAGE).await.is_err() {
            let options = CreateImageOptions {
                from_image: Some(IMAGE.into()),
                ..Default::default()
            };
            let _: Vec<_> = docker
                .create_image(Some(options), None, None)
                .try_collect()
                .await
                .unwrap();
        }
        let name = format!("docktide-test-{test}-{}", std::process::id());
        let options = CreateContainerOptions {
            name: Some(name.clone()),
            ..Default::default()
        };
        let mut labels = body.labels.clone().unwrap_or_default();
        labels.insert("io.docktide.test".into(), "true".into());
        let body = ContainerCreateBody {
            image: Some(IMAGE.into()),
            cmd: body
                .cmd
                .or_else(|| Some(vec!["sleep".into(), "300".into()])),
            labels: Some(labels),
            // `sleep` as PID 1 ignores SIGTERM; keep the Engine's default stop fast.
            stop_timeout: body.stop_timeout.or(Some(1)),
            ..body
        };
        let id = docker
            .create_container(Some(options), body)
            .await
            .unwrap()
            .id;
        Self { docker, id, name }
    }
}

impl Drop for TestContainer {
    fn drop(&mut self) {
        let options = RemoveContainerOptions {
            force: true,
            ..Default::default()
        };
        let removed = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(self.docker.remove_container(&self.id, Some(options)))
        });
        if let Err(err) = removed {
            eprintln!("failed to remove test container {}: {err}", self.id);
        }
    }
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn connects_pings_and_negotiates() {
    let service = DockerService::new();
    let status = service.connection_status().await;
    let ConnectionStatus::Connected { endpoint, engine } = status else {
        panic!("expected a connection, got {status:?}");
    };
    assert!(endpoint.uri.starts_with("unix://"));
    assert_ne!(engine.server_version, "");
    // Connected implies the negotiated version passed the minimum check.
    assert_ne!(engine.api_version, "");
    println!("{endpoint:?}\n{engine:?}");

    // The cached client is reused.
    assert!(matches!(
        service.connection_status().await,
        ConnectionStatus::Connected { .. }
    ));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires Docker"]
async fn list_includes_the_test_container() {
    let container = TestContainer::create("list").await;
    let list = DockerService::new().list_containers().await.unwrap();
    let found = list
        .iter()
        .find(|c| c.id == container.id)
        .expect("test container listed");
    assert_eq!(found.name, container.name);
    assert_eq!(found.image, IMAGE);
    assert_eq!(found.state, ContainerState::Created);
    assert_eq!(found.ports, Vec::new());
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires Docker"]
async fn list_reports_compose_labels() {
    let labels = [
        ("com.docker.compose.project", "docktide-test"),
        ("com.docker.compose.service", "web"),
        ("com.docker.compose.container-number", "1"),
        ("com.docker.compose.oneoff", "False"),
    ];
    let container = TestContainer::create_with(
        "compose",
        ContainerCreateBody {
            labels: Some(labels.map(|(k, v)| (k.into(), v.into())).into()),
            ..Default::default()
        },
    )
    .await;
    let list = DockerService::new().list_containers().await.unwrap();
    let found = list
        .iter()
        .find(|c| c.id == container.id)
        .expect("test container listed");
    assert_eq!(
        found.compose,
        Some(ComposeInfo {
            project: "docktide-test".into(),
            service: "web".into(),
            number: Some(1),
            one_off: false,
        })
    );
}

async fn state_of(service: &DockerService, id: &str) -> ContainerState {
    let list = service.list_containers().await.unwrap();
    list.iter()
        .find(|c| c.id == id)
        .expect("test container listed")
        .state
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires Docker"]
async fn start_stop_restart_transitions() {
    let container = TestContainer::create("lifecycle").await;
    let service = DockerService::new();

    service.start_container(&container.id).await.unwrap();
    assert_eq!(
        state_of(&service, &container.id).await,
        ContainerState::Running
    );
    // Docker answers 304 for an already started container.
    service.start_container(&container.id).await.unwrap();

    service.stop_container(&container.id).await.unwrap();
    assert_eq!(
        state_of(&service, &container.id).await,
        ContainerState::Exited
    );
    service.stop_container(&container.id).await.unwrap();

    service.restart_container(&container.id).await.unwrap();
    assert_eq!(
        state_of(&service, &container.id).await,
        ContainerState::Running
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires Docker"]
async fn operations_on_a_removed_container_are_not_found() {
    let id = TestContainer::create("removed").await.id.clone();
    let service = DockerService::new();
    for result in [
        service.start_container(&id).await,
        service.stop_container(&id).await,
        service.restart_container(&id).await,
    ] {
        assert!(matches!(result, Err(DockerError::NotFound)), "{result:?}");
    }
}

/// Waits for a `containersChanged` hint that names `id`.
async fn hint_naming(rx: &mut tokio::sync::mpsc::UnboundedReceiver<LiveUpdate>, id: &str) {
    let wait = async {
        loop {
            match rx.recv().await.expect("the supervisor stopped") {
                LiveUpdate::ContainersChanged { ids } if ids.iter().any(|i| i == id) => return,
                LiveUpdate::Disconnected { error } => panic!("disconnected: {error:?}"),
                _ => {}
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(2), wait)
        .await
        .expect("no hint naming the test container within 2 s");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires Docker"]
async fn events_name_the_test_container_when_it_starts_and_stops() {
    let container = TestContainer::create("events").await;
    let service = DockerService::new();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let subscription = service.subscribe_live_updates(move |update| tx.send(update).is_ok());
    assert_eq!(rx.recv().await, Some(LiveUpdate::Connected));

    service.start_container(&container.id).await.unwrap();
    hint_naming(&mut rx, &container.id).await;
    service.stop_container(&container.id).await.unwrap();
    hint_naming(&mut rx, &container.id).await;
    service.cancel_stream(subscription);
}

/// A labelled volume and a scratch directory owned by the inspect test.
struct TestMounts {
    docker: Docker,
    volume: String,
    dir: std::path::PathBuf,
}

impl TestMounts {
    async fn create() -> Self {
        let endpoint = endpoint::resolve_from_system().unwrap();
        let docker = Docker::connect_with_socket(&endpoint.uri, 120, API_DEFAULT_VERSION).unwrap();
        let volume = format!("docktide-test-inspect-{}", std::process::id());
        docker
            .create_volume(VolumeCreateRequest {
                name: Some(volume.clone()),
                labels: Some(HashMap::from([("io.docktide.test".into(), "true".into())])),
                ..Default::default()
            })
            .await
            .unwrap();
        let dir = std::env::temp_dir().join(format!("docktide-test-bind-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        Self {
            docker,
            volume,
            dir,
        }
    }
}

impl Drop for TestMounts {
    fn drop(&mut self) {
        let removed = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(
                self.docker
                    .remove_volume(&self.volume, None::<RemoveVolumeOptions>),
            )
        });
        if let Err(err) = removed {
            eprintln!("failed to remove test volume {}: {err}", self.volume);
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Ports, a named volume, a read-only bind mount, env, labels, limits, and a healthcheck.
fn configured(mounts: &TestMounts) -> ContainerCreateBody {
    ContainerCreateBody {
        env: Some(vec!["GREETING=hello=world".into(), "BARE".into()]),
        labels: Some(HashMap::from([("app".into(), "docktide".into())])),
        working_dir: Some("/tmp".into()),
        healthcheck: Some(HealthConfig {
            test: Some(vec!["CMD".into(), "true".into()]),
            interval: Some(1_000_000_000),
            ..Default::default()
        }),
        host_config: Some(HostConfig {
            binds: Some(vec![
                format!("{}:/data", mounts.volume),
                format!("{}:/conf:ro", mounts.dir.display()),
            ]),
            port_bindings: Some(HashMap::from([(
                "8080/tcp".into(),
                Some(vec![PortBinding {
                    host_ip: Some("127.0.0.1".into()),
                    host_port: None,
                }]),
            )])),
            memory: Some(64 * 1024 * 1024),
            nano_cpus: Some(500_000_000),
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires Docker"]
async fn inspect_maps_a_configured_container() {
    let mounts = TestMounts::create().await;
    // Drops before `mounts`, so the volume is no longer in use when it is removed.
    let container = TestContainer::create_with("inspect", configured(&mounts)).await;
    let service = DockerService::new();
    service.start_container(&container.id).await.unwrap();

    let inspect = service.inspect_container(&container.id).await.unwrap();
    let d = &inspect.details;
    assert_eq!(d.id, container.id);
    assert_eq!(d.name, container.name);
    assert_eq!(d.image, IMAGE);
    assert!(d.image_id.starts_with("sha256:"), "{}", d.image_id);
    assert_eq!(d.state, ContainerState::Running);
    assert!(d.started_at.is_some());
    assert_eq!((d.finished_at.as_deref(), d.exit_code), (None, None));
    assert!(d.health.is_some(), "a healthcheck is configured");
    assert_eq!(d.command, ["sleep", "300"]);
    assert_eq!(d.working_dir.as_deref(), Some("/tmp"));
    assert_eq!(d.restart_policy, "no");
    assert_eq!(d.memory_limit, Some(64 * 1024 * 1024));
    assert_eq!(d.cpu_limit, Some(0.5));
    assert_ne!(d.log_driver, "");

    let [port] = d.ports.as_slice() else {
        panic!("expected one port, got {:?}", d.ports);
    };
    assert_eq!(
        (port.private_port, port.protocol, port.host_ip.as_deref()),
        (8080, Protocol::Tcp, Some("127.0.0.1"))
    );
    assert!(port.public_port.is_some(), "Docker picks a host port");

    let mounts_seen: Vec<_> = d
        .mounts
        .iter()
        .map(|m| {
            (
                m.typ,
                m.name.as_deref(),
                m.destination.as_str(),
                m.read_only,
            )
        })
        .collect();
    assert_eq!(
        mounts_seen,
        [
            (MountType::Bind, None, "/conf", true),
            (
                MountType::Volume,
                Some(mounts.volume.as_str()),
                "/data",
                false
            ),
        ]
    );

    let env = |key: &str| {
        d.env
            .iter()
            .find(|kv| kv.key == key)
            .map(|kv| kv.value.as_str())
    };
    assert_eq!(env("GREETING"), Some("hello=world"));
    assert_eq!(env("BARE"), Some(""));
    assert!(d.labels.contains(&KeyValue {
        key: "app".into(),
        value: "docktide".into(),
    }));
    assert!(d.labels.is_sorted_by(|a, b| a.key <= b.key));
    assert!(
        d.networks
            .iter()
            .any(|n| n.name == "bridge" && n.ip_address.is_some())
    );

    // The raw JSON keeps Docker's field names.
    assert_eq!(inspect.raw["Id"], serde_json::json!(container.id));
    assert_eq!(
        inspect.raw["Config"]["Env"][0],
        serde_json::json!("GREETING=hello=world")
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires Docker"]
async fn inspecting_a_removed_container_is_not_found() {
    let id = TestContainer::create("inspect-removed").await.id.clone();
    let result = DockerService::new().inspect_container(&id).await;
    assert!(matches!(result, Err(DockerError::NotFound)), "{result:?}");
}

/// Follows logs until Docker ends the stream. Returns the lines and the final event.
async fn logs_until_end(
    service: &DockerService,
    id: &str,
    tail: Option<u32>,
    after: Option<String>,
) -> (Vec<LogLine>, LogEvent) {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    service
        .open_logs(id, tail, after, move |event| tx.send(event).is_ok())
        .await
        .unwrap();
    let collect = async {
        let mut lines = Vec::new();
        loop {
            match rx
                .recv()
                .await
                .expect("the log stream stopped without an event")
            {
                LogEvent::Lines { lines: batch } => lines.extend(batch),
                LogEvent::Gap { dropped } => panic!("{dropped} lines dropped"),
                end => return (lines, end),
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(15), collect)
        .await
        .expect("the log stream did not end")
}

/// Docker trims trailing zeros from the fraction; pad it so timestamps compare as text.
fn comparable(timestamp: &str) -> String {
    let base = timestamp.trim_end_matches('Z');
    let (seconds, fraction) = base.split_once('.').unwrap_or((base, ""));
    format!("{seconds}.{fraction:0<9}")
}

fn texts(lines: &[LogLine], stream: LogStream) -> Vec<&str> {
    lines
        .iter()
        .filter(|line| line.stream == stream)
        .map(|line| line.text.as_str())
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires Docker"]
async fn logs_follow_tail_and_resume() {
    let script = "for i in 1 2 3 4 5 6 7 8 9 10; do echo out$i; echo err$i >&2; done; \
                  sleep 1; echo late; sleep 1; echo later; sleep 1";
    let container = TestContainer::create_with(
        "logs",
        ContainerCreateBody {
            cmd: Some(vec!["sh".into(), "-c".into(), script.into()]),
            ..Default::default()
        },
    )
    .await;
    let service = DockerService::new();
    service.start_container(&container.id).await.unwrap();

    // Follows while the container runs, then ends when it exits.
    let (all, end) = logs_until_end(&service, &container.id, Some(1000), None).await;
    assert_eq!(end, LogEvent::Ended);
    let outs: Vec<String> = (1..=10).map(|i| format!("out{i}")).collect();
    let errs: Vec<String> = (1..=10).map(|i| format!("err{i}")).collect();
    let mut expected_out: Vec<&str> = outs.iter().map(String::as_str).collect();
    expected_out.extend(["late", "later"]);
    assert_eq!(texts(&all, LogStream::Stdout), expected_out);
    assert_eq!(texts(&all, LogStream::Stderr), errs);
    assert_eq!(all.last().unwrap().text, "later");
    assert!(all.iter().all(|line| line.timestamp.ends_with('Z')));

    // Tail of a stopped container.
    let (tail, end) = logs_until_end(&service, &container.id, Some(3), None).await;
    assert_eq!(end, LogEvent::Ended);
    assert_eq!(tail, all[all.len() - 3..]);

    // Resume after the burst: nothing duplicated, nothing missing. (Within a
    // burst Docker does not order stdout and stderr by timestamp; spec §9.)
    let late = all.iter().position(|line| line.text == "late").unwrap();
    let after = all[..late]
        .iter()
        .map(|line| line.timestamp.clone())
        .max_by_key(|timestamp| comparable(timestamp))
        .unwrap();
    let (rest, end) = logs_until_end(&service, &container.id, Some(1000), Some(after)).await;
    assert_eq!(end, LogEvent::Ended);
    assert_eq!(rest, all[late..]);
    let after = all[late].timestamp.clone();
    let (rest, _) = logs_until_end(&service, &container.id, Some(1000), Some(after)).await;
    assert_eq!(rest, all[late + 1..]);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires Docker"]
async fn tty_logs_arrive_as_console_lines() {
    let script = r"echo tty-line; printf '\033[32mgreen\033[0m\n'; printf '10%%\r100%%\n'";
    let container = TestContainer::create_with(
        "logs-tty",
        ContainerCreateBody {
            cmd: Some(vec!["sh".into(), "-c".into(), script.into()]),
            tty: Some(true),
            ..Default::default()
        },
    )
    .await;
    let service = DockerService::new();
    service.start_container(&container.id).await.unwrap();
    let (lines, end) = logs_until_end(&service, &container.id, None, None).await;
    assert_eq!(end, LogEvent::Ended);
    assert_eq!(
        texts(&lines, LogStream::Console),
        ["tty-line", "\x1b[32mgreen\x1b[0m", "100%"]
    );
    assert_eq!(lines.len(), 3, "{lines:?}");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires Docker"]
async fn logs_of_a_removed_container_are_not_found() {
    let id = TestContainer::create("logs-removed").await.id.clone();
    let (lines, end) = logs_until_end(&DockerService::new(), &id, Some(10), None).await;
    assert_eq!(lines, []);
    let LogEvent::Error { error } = end else {
        panic!("expected an error, got {end:?}");
    };
    assert_eq!(error.kind, docktide_core::ErrorKind::NotFound, "{error:?}");
}

/// Stats of one container from a subscription, with live updates subscribed
/// for its lifecycle events. Other containers' samples are dropped. (The
/// supervisor reads the stats of every running container; it changes none.)
struct StatsOf {
    id: String,
    rx: tokio::sync::mpsc::UnboundedReceiver<Vec<StatsSample>>,
    _updates: tokio::sync::mpsc::UnboundedReceiver<LiveUpdate>,
}

impl StatsOf {
    async fn subscribe(service: &DockerService, id: &str) -> Self {
        let (tx, mut updates) = tokio::sync::mpsc::unbounded_channel();
        service.subscribe_live_updates(move |update| tx.send(update).is_ok());
        assert_eq!(updates.recv().await, Some(LiveUpdate::Connected));
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        service.subscribe_stats(move |batch| tx.send(batch).is_ok());
        Self {
            id: id.to_owned(),
            rx,
            _updates: updates,
        }
    }

    /// The next sample of the container matching `want`.
    async fn next(&mut self, want: impl Fn(&StatsSample) -> bool) -> StatsSample {
        loop {
            let batch = self.rx.recv().await.expect("the stats supervisor stopped");
            if let Some(sample) = batch.into_iter().find(|s| s.id == self.id && want(s)) {
                return sample;
            }
        }
    }

    /// The next sample of the container matching `want`, within `within`.
    async fn sample(
        &mut self,
        within: Duration,
        want: impl Fn(&StatsSample) -> bool,
    ) -> StatsSample {
        tokio::time::timeout(within, self.next(want))
            .await
            .expect("no matching sample in time")
    }

    /// Whether no sample of the container arrives within `within`.
    async fn quiet(&mut self, within: Duration) -> bool {
        tokio::time::timeout(within, self.next(|_| true))
            .await
            .is_err()
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires Docker"]
async fn stats_a_busy_loop_reports_cpu() {
    let container = TestContainer::create_with(
        "stats-cpu",
        ContainerCreateBody {
            cmd: Some(vec!["sh".into(), "-c".into(), "while :; do :; done".into()]),
            ..Default::default()
        },
    )
    .await;
    let service = DockerService::new();
    service.start_container(&container.id).await.unwrap();
    let mut stats = StatsOf::subscribe(&service, &container.id).await;
    // The first sample has no CPU value; the second comes 1 s later, and
    // batching adds up to 1 s.
    let sample = stats
        .sample(Duration::from_secs(5), |s| s.cpu_percent.is_some())
        .await;
    assert!(sample.cpu_percent > Some(50.0), "{sample:?}");
    assert!(
        sample.memory_usage.is_some() && sample.pids.is_some(),
        "{sample:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires Docker"]
async fn stats_stop_closes_and_start_reopens() {
    let container = TestContainer::create("stats-stop").await;
    let service = DockerService::new();
    service.start_container(&container.id).await.unwrap();
    let mut stats = StatsOf::subscribe(&service, &container.id).await;
    stats.sample(Duration::from_secs(5), |_| true).await;

    service.stop_container(&container.id).await.unwrap();
    // Samples read before the stop may still be in the next batch.
    let _ = stats.quiet(Duration::from_millis(1500)).await;
    assert!(
        stats.quiet(Duration::from_secs(2)).await,
        "samples after the stop"
    );

    service.start_container(&container.id).await.unwrap();
    let sample = stats.sample(Duration::from_secs(5), |_| true).await;
    assert!(sample.stream_start, "{sample:?}");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires Docker"]
async fn stats_restart_starts_a_new_stream() {
    let container = TestContainer::create("stats-restart").await;
    let service = DockerService::new();
    service.start_container(&container.id).await.unwrap();
    let mut stats = StatsOf::subscribe(&service, &container.id).await;
    stats
        .sample(Duration::from_secs(5), |s| !s.stream_start)
        .await;

    service.restart_container(&container.id).await.unwrap();
    stats
        .sample(Duration::from_secs(5), |s| s.stream_start)
        .await;
}
