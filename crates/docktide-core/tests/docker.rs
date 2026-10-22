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
    ConnectionStatus, ContainerState, DockerError, DockerService, KeyValue, LiveUpdate, MountType,
    Protocol, endpoint,
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

    /// `body` overrides the defaults; its labels are added to the test label.
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
            cmd: Some(vec!["sleep".into(), "300".into()]),
            labels: Some(labels),
            // `sleep` as PID 1 ignores SIGTERM; keep the Engine's default stop fast.
            stop_timeout: Some(1),
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
