//! Tests against the real Docker Engine. Run with `cargo test -- --ignored`.
//! They must never touch containers without the `io.docktide.test=true` label.

use std::collections::HashMap;

use bollard::models::ContainerCreateBody;
use bollard::query_parameters::{
    CreateContainerOptions, CreateImageOptions, RemoveContainerOptions,
};
use bollard::{API_DEFAULT_VERSION, Docker};
use docktide_core::{ConnectionStatus, ContainerState, DockerService, endpoint};
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
