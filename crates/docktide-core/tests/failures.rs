//! Error classification against real socket failures (spec M0 §7.3). No Docker needed.

use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};

use docktide_core::{
    ConnectionStatus, DockerService, EndpointSource, ErrorKind, IpcError, ResolvedEndpoint,
};

/// A scratch directory removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("docktide-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

async fn error_for(socket: &Path) -> IpcError {
    let endpoint = ResolvedEndpoint {
        uri: format!("unix://{}", socket.display()),
        source: EndpointSource::DockerHostEnv,
        context_name: None,
    };
    match DockerService::new().connection_status_for(endpoint).await {
        ConnectionStatus::Disconnected { error, .. } => error,
        status @ ConnectionStatus::Connected { .. } => panic!("unexpected {status:?}"),
    }
}

#[tokio::test]
async fn missing_socket_is_daemon_unavailable() {
    let scratch = Scratch::new("missing");
    let error = error_for(&scratch.0.join("nonexistent.sock")).await;
    assert_eq!(error.kind, ErrorKind::DaemonUnavailable, "{error:?}");
}

#[tokio::test]
async fn socket_without_listener_is_daemon_unavailable() {
    let scratch = Scratch::new("refused");
    let socket = scratch.0.join("docker.sock");
    drop(UnixListener::bind(&socket).unwrap());
    let error = error_for(&socket).await;
    assert_eq!(error.kind, ErrorKind::DaemonUnavailable, "{error:?}");
}

#[tokio::test]
async fn unreadable_socket_is_permission_denied() {
    if std::fs::metadata("/proc/self").unwrap().uid() == 0 {
        eprintln!("skipped: root ignores socket permissions");
        return;
    }
    let scratch = Scratch::new("denied");
    let socket = scratch.0.join("docker.sock");
    let _listener = UnixListener::bind(&socket).unwrap();
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o000)).unwrap();
    let error = error_for(&socket).await;
    assert_eq!(error.kind, ErrorKind::PermissionDenied, "{error:?}");
}
