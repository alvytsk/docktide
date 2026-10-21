//! Error classification against real socket failures (spec M0 §7.3), API
//! negotiation, and the events supervisor (spec M1 §7.3) against a fake Engine.
//! No Docker needed.

use std::io::{BufRead, BufReader, Write};
use std::net::Shutdown;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use docktide_core::{
    ConnectionStatus, DockerService, EndpointSource, ErrorKind, IpcError, LiveUpdate,
    ResolvedEndpoint, SubscriptionId,
};
use tokio::sync::mpsc;

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
/// `/events` responses stay open until `close_event_streams`.
struct FakeEngine {
    _scratch: Option<Scratch>,
    socket: PathBuf,
    requests: Arc<Mutex<Vec<String>>>,
    event_streams: Arc<Mutex<Vec<UnixStream>>>,
}

impl FakeEngine {
    fn start(name: &str, api: &'static str) -> Self {
        let scratch = Scratch::new(name);
        let socket = scratch.0.join("docker.sock");
        Self {
            _scratch: Some(scratch),
            ..Self::bind(socket, api)
        }
    }

    fn bind(socket: PathBuf, api: &'static str) -> Self {
        let listener = UnixListener::bind(&socket).unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let event_streams = Arc::new(Mutex::new(Vec::new()));
        let (log, streams) = (Arc::clone(&requests), Arc::clone(&event_streams));
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let (log, streams) = (Arc::clone(&log), Arc::clone(&streams));
                std::thread::spawn(move || serve(stream, api, &log, &streams));
            }
        });
        Self {
            _scratch: None,
            socket,
            requests,
            event_streams,
        }
    }

    fn take_requests(&self) -> Vec<String> {
        std::mem::take(&mut self.requests.lock().unwrap())
    }

    /// Sends a container event to every open `/events` response. Returns how
    /// many accepted it.
    fn emit(&self, action: &str, id: &str) -> usize {
        let body = format!(
            r#"{{"Type":"container","Action":"{action}","Actor":{{"ID":"{id}","Attributes":{{}}}},"scope":"local"}}"#
        ) + "\n";
        let chunk = format!("{:x}\r\n{body}\r\n", body.len());
        let mut streams = self.event_streams.lock().unwrap();
        streams.retain_mut(|stream| stream.write_all(chunk.as_bytes()).is_ok());
        streams.len()
    }

    /// Waits until a client has an `/events` response open.
    async fn wait_for_event_stream(&self) {
        for _ in 0..200 {
            if !self.event_streams.lock().unwrap().is_empty() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("no events request arrived");
    }

    /// Ends every `/events` response, cleanly with a final chunk or abruptly.
    fn close_event_streams(&self, clean: bool) {
        for mut stream in self.event_streams.lock().unwrap().drain(..) {
            if clean {
                let _ = stream.write_all(b"0\r\n\r\n");
            }
            let _ = stream.shutdown(Shutdown::Both);
        }
    }
}

fn serve(
    stream: UnixStream,
    api: &str,
    log: &Mutex<Vec<String>>,
    event_streams: &Mutex<Vec<UnixStream>>,
) {
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
                "/events" => {
                    let headers = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nApi-Version: {api}\r\nTransfer-Encoding: chunked\r\n\r\n"
                    );
                    if writer.write_all(headers.as_bytes()).is_ok() {
                        event_streams.lock().unwrap().push(writer);
                    }
                    return;
                }
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

/// Live updates received by a test subscriber.
struct Updates {
    id: SubscriptionId,
    rx: mpsc::UnboundedReceiver<LiveUpdate>,
}

impl Updates {
    fn subscribe(service: &DockerService) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let id = service.subscribe_live_updates(move |update| tx.send(update).is_ok());
        Self { id, rx }
    }

    async fn next(&mut self, within: Duration) -> LiveUpdate {
        tokio::time::timeout(within, self.rx.recv())
            .await
            .expect("no live update in time")
            .expect("the supervisor stopped")
    }

    /// Whether the supervisor dropped its sink within `within`.
    async fn closes(&mut self, within: Duration) -> bool {
        matches!(tokio::time::timeout(within, self.rx.recv()).await, Ok(None))
    }
}

fn changed(ids: &[&str]) -> LiveUpdate {
    LiveUpdate::ContainersChanged {
        ids: ids.iter().map(|&id| id.to_owned()).collect(),
    }
}

const SOON: Duration = Duration::from_secs(2);

#[tokio::test]
async fn container_events_reach_the_subscriber_as_one_hint() {
    let engine = FakeEngine::start("events-hint", "1.53");
    let service = DockerService::for_endpoint(endpoint(&engine.socket));
    let mut updates = Updates::subscribe(&service);
    assert_eq!(updates.next(SOON).await, LiveUpdate::Connected);
    engine.wait_for_event_stream().await;
    engine.emit("start", "a");
    engine.emit("exec_start: /bin/sh -c true", "b");
    engine.emit("die", "c");
    assert_eq!(updates.next(SOON).await, changed(&["a", "c"]));
}

#[tokio::test]
async fn an_idle_events_stream_outlives_the_request_timeout() {
    let engine = FakeEngine::start("events-idle", "1.53");
    let service = DockerService::for_endpoint(endpoint(&engine.socket)).with_request_timeout(1);
    let mut updates = Updates::subscribe(&service);
    assert_eq!(updates.next(SOON).await, LiveUpdate::Connected);
    engine.wait_for_event_stream().await;
    tokio::time::sleep(Duration::from_millis(2500)).await;
    assert!(
        updates.rx.try_recv().is_err(),
        "the stream did not stay quiet"
    );
    assert_eq!(engine.emit("start", "a"), 1);
    assert_eq!(updates.next(SOON).await, changed(&["a"]));
}

#[tokio::test]
async fn an_ended_events_stream_reports_disconnected_then_reconnects() {
    lost_events_stream_reports_disconnected_then_reconnects("events-eof", true).await;
}

#[tokio::test]
async fn a_cut_events_stream_reports_disconnected_then_reconnects() {
    lost_events_stream_reports_disconnected_then_reconnects("events-cut", false).await;
}

async fn lost_events_stream_reports_disconnected_then_reconnects(name: &str, clean: bool) {
    let engine = FakeEngine::start(name, "1.53");
    let service = DockerService::for_endpoint(endpoint(&engine.socket));
    let mut updates = Updates::subscribe(&service);
    assert_eq!(updates.next(SOON).await, LiveUpdate::Connected);
    engine.wait_for_event_stream().await;
    engine.close_event_streams(clean);
    let LiveUpdate::Disconnected { error } = updates.next(SOON).await else {
        panic!("expected disconnected");
    };
    assert_eq!(error.kind, ErrorKind::DaemonUnavailable, "{error:?}");
    // The first retry comes after 1 s.
    assert_eq!(
        updates.next(Duration::from_secs(3)).await,
        LiveUpdate::Connected
    );
}

#[tokio::test]
async fn the_supervisor_connects_when_the_engine_comes_back() {
    let scratch = Scratch::new("events-late");
    let socket = scratch.0.join("docker.sock");
    let service = DockerService::for_endpoint(endpoint(&socket));
    let mut updates = Updates::subscribe(&service);
    let LiveUpdate::Disconnected { error } = updates.next(SOON).await else {
        panic!("expected disconnected");
    };
    assert_eq!(error.kind, ErrorKind::DaemonUnavailable, "{error:?}");
    let engine = FakeEngine::bind(socket, "1.53");
    assert_eq!(
        updates.next(Duration::from_secs(3)).await,
        LiveUpdate::Connected
    );
    engine.wait_for_event_stream().await;
}

#[tokio::test]
async fn a_successful_status_check_wakes_the_supervisor() {
    let scratch = Scratch::new("events-wake");
    let socket = scratch.0.join("docker.sock");
    let service = DockerService::for_endpoint(endpoint(&socket));
    let mut updates = Updates::subscribe(&service);
    assert!(matches!(
        updates.next(SOON).await,
        LiveUpdate::Disconnected { .. }
    ));
    // Fail again so the supervisor waits 2 s before its next attempt.
    tokio::time::sleep(Duration::from_millis(1200)).await;
    let _engine = FakeEngine::bind(socket, "1.53");
    let status = service.connection_status().await;
    assert!(
        matches!(status, ConnectionStatus::Connected { .. }),
        "{status:?}"
    );
    assert_eq!(
        updates.next(Duration::from_millis(500)).await,
        LiveUpdate::Connected
    );
}

#[tokio::test]
async fn cancelling_stops_the_supervisor_and_closes_the_stream() {
    let engine = FakeEngine::start("events-cancel", "1.53");
    let service = DockerService::for_endpoint(endpoint(&engine.socket));
    let mut updates = Updates::subscribe(&service);
    assert_eq!(updates.next(SOON).await, LiveUpdate::Connected);
    engine.wait_for_event_stream().await;
    service.cancel_stream(updates.id);
    assert!(updates.closes(SOON).await);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        engine.emit("start", "a"),
        0,
        "the events connection is still open"
    );
}

#[tokio::test]
async fn a_new_subscriber_replaces_the_previous_one() {
    let engine = FakeEngine::start("events-replace", "1.53");
    let service = DockerService::for_endpoint(endpoint(&engine.socket));
    let mut first = Updates::subscribe(&service);
    assert_eq!(first.next(SOON).await, LiveUpdate::Connected);
    let mut second = Updates::subscribe(&service);
    assert!(first.closes(SOON).await);
    assert_eq!(second.next(SOON).await, LiveUpdate::Connected);
}
