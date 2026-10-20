//! Tests against the real Docker Engine. Run with `cargo test -- --ignored`.
//! They must never touch containers without the `io.docktide.test=true` label.

use docktide_core::{ConnectionStatus, DockerService};

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
