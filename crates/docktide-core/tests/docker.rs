//! Tests against the real Docker Engine. Run with `cargo test -- --ignored`.
//! They must never touch containers without the `io.docktide.test=true` label.

use std::collections::HashMap;
use std::time::Duration;

use bollard::models::ContainerCreateBody;
use bollard::query_parameters::{
    CreateContainerOptions, CreateImageOptions, RemoveContainerOptions,
};
use bollard::{API_DEFAULT_VERSION, Docker};
use docktide_core::{
    ConnectionStatus, ContainerState, DockerError, DockerService, LiveUpdate, endpoint,
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
        let body = ContainerCreateBody {
            image: Some(IMAGE.into()),
            cmd: Some(vec!["sleep".into(), "300".into()]),
            labels: Some(HashMap::from([("io.docktide.test".into(), "true".into())])),
            // `sleep` as PID 1 ignores SIGTERM; keep the Engine's default stop fast.
            stop_timeout: Some(1),
            ..Default::default()
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
