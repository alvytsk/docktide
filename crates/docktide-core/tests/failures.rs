//! Error classification against real socket failures (spec M0 §7.3), and API
//! negotiation against a fake Engine. No Docker needed.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

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

fn endpoint(socket: &Path) -> ResolvedEndpoint {
    ResolvedEndpoint {
        uri: format!("unix://{}", socket.display()),
        source: EndpointSource::DockerHostEnv,
        context_name: None,
    }
}

async fn error_for(socket: &Path) -> IpcError {
    match DockerService::new()
        .connection_status_for(endpoint(socket))
        .await
    {
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
    if is_root() {
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

fn is_root() -> bool {
    std::fs::metadata("/proc/self").unwrap().uid() == 0
}

#[tokio::test]
async fn socket_in_untraversable_directory_is_permission_denied() {
    if is_root() {
        eprintln!("skipped: root ignores directory permissions");
        return;
    }
    let scratch = Scratch::new("untraversable");
    let dir = scratch.0.join("private");
    std::fs::create_dir(&dir).unwrap();
    let _listener = UnixListener::bind(dir.join("docker.sock")).unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o000)).unwrap();
    let error = error_for(&dir.join("docker.sock")).await;
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(error.kind, ErrorKind::PermissionDenied, "{error:?}");
}

/// A minimal Engine whose maximum API version is `api`. Like the real daemon, it
/// rejects versioned paths above that with HTTP 400. Records each request path.
struct FakeEngine {
    _scratch: Scratch,
    socket: PathBuf,
    requests: Arc<Mutex<Vec<String>>>,
}

impl FakeEngine {
    fn start(name: &str, api: &'static str) -> Self {
        let scratch = Scratch::new(name);
        let socket = scratch.0.join("docker.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&requests);
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let log = Arc::clone(&log);
                std::thread::spawn(move || serve(stream, api, &log));
            }
        });
        Self {
            _scratch: scratch,
            socket,
            requests,
        }
    }

    fn take_requests(&self) -> Vec<String> {
        std::mem::take(&mut self.requests.lock().unwrap())
    }
}

fn serve(stream: UnixStream, api: &str, log: &Mutex<Vec<String>>) {
    let mut writer = stream.try_clone().unwrap();
    let mut reader = BufReader::new(stream);
    let max: Vec<u32> = api.split('.').map(|n| n.parse().unwrap()).collect();
    let mut line = String::new();
    // One request per iteration; the client keeps the connection alive.
    while reader.read_line(&mut line).is_ok_and(|n| n > 0) {
        let path = line.split(' ').nth(1).unwrap_or("").to_owned();
        loop {
            let mut header = String::new();
            if reader.read_line(&mut header).unwrap_or(0) == 0 || header == "\r\n" {
                break;
            }
        }
        let path = path.split('?').next().unwrap().to_owned();
        log.lock().unwrap().push(path.clone());
        let (versioned, unversioned) = match path.strip_prefix("/v").and_then(|p| p.split_once('/'))
        {
            Some((version, rest)) => (Some(version.to_owned()), format!("/{rest}")),
            None => (None, path),
        };
        let too_new = versioned.is_some_and(|v| {
            let v: Vec<u32> = v.split('.').map(|n| n.parse().unwrap()).collect();
            v > max
        });
        let (status, body) = if too_new {
            (
                "400 Bad Request",
                format!(
                    r#"{{"message":"client version is too new. Maximum supported API version is {api}"}}"#
                ),
            )
        } else {
            match unversioned.as_str() {
                "/_ping" => ("200 OK", "OK".to_owned()),
                "/version" => (
                    "200 OK",
                    format!(
                        r#"{{"Version":"25.0.0","ApiVersion":"{api}","Os":"linux","Arch":"amd64"}}"#
                    ),
                ),
                _ => ("200 OK", "{}".to_owned()),
            }
        };
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nApi-Version: {api}\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        if writer.write_all(response.as_bytes()).is_err() {
            return;
        }
        line.clear();
    }
}

#[tokio::test]
async fn engine_25_connects_and_later_polls_only_ping() {
    let engine = FakeEngine::start("engine25", "1.44");
    let service = DockerService::new();
    let status = service
        .connection_status_for(endpoint(&engine.socket))
        .await;
    let ConnectionStatus::Connected { engine: info, .. } = status else {
        panic!("expected a connection, got {status:?}");
    };
    assert_eq!(info.server_version, "25.0.0");
    assert_eq!(info.api_version, "1.44");
    engine.take_requests();

    let status = service
        .connection_status_for(endpoint(&engine.socket))
        .await;
    assert!(
        matches!(status, ConnectionStatus::Connected { .. }),
        "{status:?}"
    );
    assert_eq!(engine.take_requests(), ["/_ping"]);
}

#[tokio::test]
async fn engine_below_minimum_api_is_unsupported() {
    let engine = FakeEngine::start("engine24", "1.43");
    let error = error_for(&engine.socket).await;
    assert_eq!(error.kind, ErrorKind::ApiVersionUnsupported, "{error:?}");
}
