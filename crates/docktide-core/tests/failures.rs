//! Error classification against real socket failures (spec M0 §7.3), API
//! negotiation, the events supervisor, log streams (spec M1 §7.3), and the stats
//! supervisor (spec M2 §7.3) against a fake Engine.
//! No Docker needed.

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Write};
use std::net::Shutdown;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use docktide_core::{
    ConnectionStatus, DockerService, EndpointSource, ErrorKind, IpcError, LiveUpdate, LogEvent,
    LogStream, ResolvedEndpoint, StatsSample, SubscriptionId,
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
/// `/events` responses stay open until `close_event_streams`; log and stats
/// responses stay open until the client closes them.
struct FakeEngine {
    _scratch: Option<Scratch>,
    socket: PathBuf,
    requests: Arc<Mutex<Vec<String>>>,
    event_streams: Arc<Mutex<Vec<UnixStream>>>,
    logs: Arc<LogStreams>,
    stats: Arc<StatsStreams>,
}

/// Open log responses, and how many the client has closed.
#[derive(Default)]
struct LogStreams {
    open: Mutex<Vec<UnixStream>>,
    closed: AtomicUsize,
}

/// The container list, open stats responses by container id, and how many the
/// client has closed.
struct StatsStreams {
    list: Mutex<String>,
    list_delay: Mutex<Duration>,
    missing: Mutex<HashSet<String>>,
    open: Mutex<Vec<(String, UnixStream)>>,
    closed: AtomicUsize,
    /// The last CPU counters sent per id, for `precpu_stats`.
    previous: Mutex<HashMap<String, (u64, u64)>>,
}

impl Default for StatsStreams {
    fn default() -> Self {
        Self {
            list: Mutex::new("[]".to_owned()),
            list_delay: Mutex::default(),
            missing: Mutex::default(),
            open: Mutex::default(),
            closed: AtomicUsize::default(),
            previous: Mutex::default(),
        }
    }
}

/// A `/containers/json` entry.
fn container_json(id: &str, state: &str) -> String {
    format!(
        r#"{{"Id":"{id}","Names":["/{id}"],"Image":"busybox","State":"{state}","Status":"","Ports":[]}}"#
    )
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
        let logs = Arc::new(LogStreams::default());
        let stats = Arc::new(StatsStreams::default());
        let (log, streams, log_streams, stats_streams) = (
            Arc::clone(&requests),
            Arc::clone(&event_streams),
            Arc::clone(&logs),
            Arc::clone(&stats),
        );
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let (log, streams, log_streams, stats_streams) = (
                    Arc::clone(&log),
                    Arc::clone(&streams),
                    Arc::clone(&log_streams),
                    Arc::clone(&stats_streams),
                );
                std::thread::spawn(move || {
                    serve(stream, api, &log, &streams, &log_streams, &stats_streams);
                });
            }
        });
        Self {
            _scratch: None,
            socket,
            requests,
            event_streams,
            logs,
            stats,
        }
    }

    fn take_requests(&self) -> Vec<String> {
        std::mem::take(&mut self.requests.lock().unwrap())
    }

    /// How many requests so far had a path ending in `suffix`.
    fn requested(&self, suffix: &str) -> usize {
        let requests = self.requests.lock().unwrap();
        requests
            .iter()
            .filter(|path| path.ends_with(suffix))
            .count()
    }

    /// Sends a container event to every open `/events` response. Returns how
    /// many accepted it.
    fn emit(&self, action: &str, id: &str) -> usize {
        self.emit_burst(action, id, 1)
    }

    /// Sends `count` copies of a container event in one write. (Many small
    /// writes from the test's thread would fill the socket buffer and block it.)
    fn emit_burst(&self, action: &str, id: &str, count: usize) -> usize {
        let body = format!(
            r#"{{"Type":"container","Action":"{action}","Actor":{{"ID":"{id}","Attributes":{{}}}},"scope":"local"}}"#
        ) + "\n";
        let body = body.repeat(count);
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

    /// Writes a multiplexed stdout frame to every open log response.
    fn log_line(&self, text: &str) {
        let payload = format!("2026-10-22T04:38:47.825201379Z {text}\n");
        let mut frame = vec![1, 0, 0, 0];
        frame.extend_from_slice(&u32::try_from(payload.len()).unwrap().to_be_bytes());
        frame.extend_from_slice(payload.as_bytes());
        let mut chunk = format!("{:x}\r\n", frame.len()).into_bytes();
        chunk.extend_from_slice(&frame);
        chunk.extend_from_slice(b"\r\n");
        let mut open = self.logs.open.lock().unwrap();
        open.retain_mut(|stream| stream.write_all(&chunk).is_ok());
    }

    /// Waits until `count` log responses are open.
    async fn wait_for_log_streams(&self, count: usize) {
        for _ in 0..200 {
            if self.logs.open.lock().unwrap().len() >= count {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("no logs request arrived");
    }

    /// Waits until the client has closed `count` log connections in total.
    async fn wait_for_closed_logs(&self, count: usize) {
        for _ in 0..200 {
            if self.logs.closed.load(Ordering::SeqCst) >= count {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("the client kept the logs connection open");
    }

    /// Serves `json` for `/containers/json`.
    fn set_containers(&self, json: &str) {
        json.clone_into(&mut self.stats.list.lock().unwrap());
    }

    /// Answers `/containers/json` only after `delay`.
    fn delay_lists(&self, delay: Duration) {
        *self.stats.list_delay.lock().unwrap() = delay;
    }

    /// Answers stats requests for `id` with 404.
    fn missing(&self, id: &str) {
        self.stats.missing.lock().unwrap().insert(id.to_owned());
    }

    /// Writes one stats sample to the open stats responses for `id`. Its
    /// `precpu_stats` are the counters of the previous call for `id`.
    fn stats_sample(&self, id: &str, read: &str, cpu: u64, system: u64) {
        let cpu_stats = |(cpu, system)| {
            format!(
                r#"{{"cpu_usage":{{"total_usage":{cpu}}},"system_cpu_usage":{system},"online_cpus":1}}"#
            )
        };
        let previous = self
            .stats
            .previous
            .lock()
            .unwrap()
            .insert(id.to_owned(), (cpu, system));
        let body = format!(
            r#"{{"read":"{read}","cpu_stats":{},"precpu_stats":{}}}"#,
            cpu_stats((cpu, system)),
            previous.map_or_else(|| "{}".to_owned(), cpu_stats)
        ) + "\n";
        let chunk = format!("{:x}\r\n{body}\r\n", body.len());
        let mut open = self.stats.open.lock().unwrap();
        open.retain_mut(|(open_id, stream)| {
            open_id != id || stream.write_all(chunk.as_bytes()).is_ok()
        });
    }

    /// Cuts the stats connections for `id`, and only those.
    fn cut_stats(&self, id: &str) {
        for (open_id, stream) in self.stats.open.lock().unwrap().iter() {
            if open_id == id {
                let _ = stream.shutdown(Shutdown::Both);
            }
        }
    }

    /// Ids of the open stats responses, sorted.
    fn open_stats(&self) -> Vec<String> {
        let open = self.stats.open.lock().unwrap();
        let mut ids: Vec<String> = open.iter().map(|(id, _)| id.clone()).collect();
        ids.sort();
        ids
    }

    /// Waits until `count` stats responses for `id` are open.
    async fn wait_for_stats_streams(&self, id: &str, count: usize) {
        for _ in 0..200 {
            if self.open_stats().iter().filter(|open| *open == id).count() >= count {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("no stats request for {id} arrived");
    }

    /// Waits until the client has closed `count` stats connections in total.
    async fn wait_for_closed_stats(&self, count: usize) {
        for _ in 0..200 {
            if self.stats.closed.load(Ordering::SeqCst) >= count {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("the client kept a stats connection open");
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
    logs: &LogStreams,
    stats_streams: &StatsStreams,
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
                path if path.starts_with("/containers/") && path.ends_with("/logs") => {
                    let headers = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/vnd.docker.multiplexed-stream\r\nApi-Version: {api}\r\nTransfer-Encoding: chunked\r\n\r\n"
                    );
                    if writer.write_all(headers.as_bytes()).is_ok() {
                        logs.open.lock().unwrap().push(writer);
                    }
                    // The client sends nothing more; EOF means it closed the connection.
                    let mut rest = Vec::new();
                    let _ = std::io::Read::read_to_end(&mut reader, &mut rest);
                    logs.closed.fetch_add(1, Ordering::SeqCst);
                    return;
                }
                path if path.starts_with("/containers/") && path.ends_with("/stats") => {
                    let id = &path["/containers/".len()..path.len() - "/stats".len()];
                    if stats_streams.missing.lock().unwrap().contains(id) {
                        (
                            "404 Not Found",
                            format!(r#"{{"message":"No such container: {id}"}}"#),
                        )
                    } else {
                        serve_stats(id, api, writer, &mut reader, stats_streams);
                        return;
                    }
                }
                "/containers/json" => {
                    let delay = *stats_streams.list_delay.lock().unwrap();
                    std::thread::sleep(delay);
                    ("200 OK", stats_streams.list.lock().unwrap().clone())
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

/// Keeps a stats response open until the client closes the connection.
fn serve_stats(
    id: &str,
    api: &str,
    mut writer: UnixStream,
    reader: &mut BufReader<UnixStream>,
    stats: &StatsStreams,
) {
    let headers = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nApi-Version: {api}\r\nTransfer-Encoding: chunked\r\n\r\n"
    );
    let fd = writer.as_raw_fd();
    if writer.write_all(headers.as_bytes()).is_ok() {
        stats.open.lock().unwrap().push((id.to_owned(), writer));
    }
    // As for logs, EOF means the client closed the connection.
    let mut rest = Vec::new();
    let _ = std::io::Read::read_to_end(reader, &mut rest);
    stats
        .open
        .lock()
        .unwrap()
        .retain(|(_, s)| s.as_raw_fd() != fd);
    stats.closed.fetch_add(1, Ordering::SeqCst);
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

/// Collects log events from a subscription.
fn log_events() -> (
    mpsc::UnboundedSender<LogEvent>,
    mpsc::UnboundedReceiver<LogEvent>,
) {
    mpsc::unbounded_channel()
}

#[tokio::test]
async fn closing_logs_closes_the_engine_connection() {
    let engine = FakeEngine::start("logs-close", "1.53");
    let service = DockerService::for_endpoint(endpoint(&engine.socket));
    let (tx, mut rx) = log_events();
    let id = service
        .open_logs("abc", Some(1000), None, move |event| tx.send(event).is_ok())
        .await
        .unwrap();
    engine.wait_for_log_streams(1).await;
    engine.log_line("hello");
    let event = tokio::time::timeout(SOON, rx.recv())
        .await
        .unwrap()
        .unwrap();
    let LogEvent::Lines { lines } = event else {
        panic!("expected lines, got {event:?}");
    };
    assert_eq!(
        (lines[0].stream, lines[0].text.as_str()),
        (LogStream::Stdout, "hello")
    );

    service.cancel_stream(id);
    engine.wait_for_closed_logs(1).await;
    assert!(
        matches!(tokio::time::timeout(SOON, rx.recv()).await, Ok(None)),
        "cancelling drops the sink without further events"
    );
}

#[tokio::test]
async fn a_closed_sink_closes_the_engine_connection() {
    let engine = FakeEngine::start("logs-sink", "1.53");
    let service = DockerService::for_endpoint(endpoint(&engine.socket));
    service
        .open_logs("abc", None, None, |_| false)
        .await
        .unwrap();
    engine.wait_for_log_streams(1).await;
    engine.log_line("nobody is listening");
    engine.wait_for_closed_logs(1).await;
}

#[tokio::test]
async fn cancel_all_closes_every_log_connection() {
    let engine = FakeEngine::start("logs-all", "1.53");
    let service = DockerService::for_endpoint(endpoint(&engine.socket));
    for _ in 0..3 {
        service
            .open_logs("abc", Some(10), None, |_| true)
            .await
            .unwrap();
    }
    engine.wait_for_log_streams(3).await;
    service.cancel_all_streams();
    engine.wait_for_closed_logs(3).await;
}

/// Stats batches received by a test subscriber.
struct Batches {
    id: SubscriptionId,
    rx: mpsc::UnboundedReceiver<Vec<StatsSample>>,
}

impl Batches {
    fn subscribe(service: &DockerService) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let id = service.subscribe_stats(move |batch| tx.send(batch).is_ok());
        Self { id, rx }
    }

    async fn next(&mut self, within: Duration) -> Vec<StatsSample> {
        tokio::time::timeout(within, self.rx.recv())
            .await
            .expect("no stats batch in time")
            .expect("the stats supervisor stopped")
    }

    /// Whether no batch arrives within `within`.
    async fn quiet(&mut self, within: Duration) -> bool {
        tokio::time::timeout(within, self.rx.recv()).await.is_err()
    }
}

/// A fake Engine listing `containers` (id, state), and a service with live
/// updates subscribed, which feed the stats supervisor its container events.
async fn stats_engine(
    name: &str,
    containers: &[(&str, &str)],
) -> (FakeEngine, DockerService, Updates) {
    let engine = FakeEngine::start(name, "1.53");
    let list: Vec<String> = containers
        .iter()
        .map(|(id, state)| container_json(id, state))
        .collect();
    engine.set_containers(&format!("[{}]", list.join(",")));
    let service = DockerService::for_endpoint(endpoint(&engine.socket));
    let updates = Updates::subscribe(&service);
    engine.wait_for_event_stream().await;
    (engine, service, updates)
}

fn read_time(second: u32) -> String {
    format!("2026-10-24T10:00:{second:02}.5Z")
}

const ZERO_READ: &str = "0001-01-01T00:00:00Z";

#[tokio::test]
async fn running_containers_get_a_stream_on_subscribe() {
    let containers = [("a", "running"), ("b", "paused"), ("c", "exited")];
    let (engine, service, _updates) = stats_engine("stats-list", &containers).await;
    let _batches = Batches::subscribe(&service);
    engine.wait_for_stats_streams("a", 1).await;
    engine.wait_for_stats_streams("b", 1).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(engine.open_stats(), ["a", "b"]);
}

#[tokio::test]
async fn samples_arrive_batched_once_per_second() {
    let containers = [("a", "running"), ("b", "running")];
    let (engine, service, _updates) = stats_engine("stats-batch", &containers).await;
    let mut batches = Batches::subscribe(&service);
    engine.wait_for_stats_streams("a", 1).await;
    engine.wait_for_stats_streams("b", 1).await;
    let feed = async {
        for i in 0..3 {
            engine.stats_sample("a", &read_time(i), u64::from(i) * 100, u64::from(i) * 1000);
            engine.stats_sample("b", &read_time(i), u64::from(i) * 100, u64::from(i) * 1000);
            tokio::time::sleep(Duration::from_millis(700)).await;
        }
    };
    let collect = async {
        let mut received = Vec::new();
        while received
            .iter()
            .map(|(_, b): &(Instant, Vec<StatsSample>)| b.len())
            .sum::<usize>()
            < 6
        {
            let batch = batches.next(Duration::from_secs(3)).await;
            received.push((Instant::now(), batch));
        }
        received
    };
    let ((), received) = tokio::join!(feed, collect);
    assert!(received.len() >= 2, "{received:?}");
    for pair in received.windows(2) {
        let gap = pair[1].0 - pair[0].0;
        // The tick is exact; delivery to the test adds a little jitter.
        assert!(gap >= Duration::from_millis(950), "batches {gap:?} apart");
    }
    let samples: Vec<StatsSample> = received.into_iter().flat_map(|(_, b)| b).collect();
    for id in ["a", "b"] {
        let of_id: Vec<(&str, bool)> = samples
            .iter()
            .filter(|s| s.id == id)
            .map(|s| (s.time.as_str(), s.stream_start))
            .collect();
        let expected: Vec<(String, bool)> = (0..3).map(|i| (read_time(i), i == 0)).collect();
        let expected: Vec<(&str, bool)> = expected.iter().map(|(t, s)| (t.as_str(), *s)).collect();
        assert_eq!(of_id, expected, "{id}");
    }
}

#[tokio::test]
async fn a_zero_read_sample_closes_the_stream() {
    let (engine, service, _updates) = stats_engine("stats-zero", &[("a", "running")]).await;
    let mut batches = Batches::subscribe(&service);
    engine.wait_for_stats_streams("a", 1).await;
    engine.stats_sample("a", ZERO_READ, 0, 0);
    engine.wait_for_closed_stats(1).await;
    assert!(batches.quiet(Duration::from_millis(1500)).await);
    assert_eq!(engine.open_stats(), Vec::<String>::new());
}

/// Sends a sample for `id` and returns it as delivered.
async fn sample_of(
    engine: &FakeEngine,
    batches: &mut Batches,
    id: &str,
    second: u32,
) -> StatsSample {
    engine.stats_sample(id, &read_time(second), 0, 0);
    let batch = batches.next(Duration::from_secs(2)).await;
    let [sample] = batch.as_slice() else {
        panic!("expected one sample, got {batch:?}");
    };
    assert_eq!(sample.id, id);
    sample.clone()
}

#[tokio::test]
async fn a_start_event_replaces_an_open_stream() {
    let (engine, service, _updates) = stats_engine("stats-start", &[("a", "running")]).await;
    let mut batches = Batches::subscribe(&service);
    engine.wait_for_stats_streams("a", 1).await;
    assert!(sample_of(&engine, &mut batches, "a", 0).await.stream_start);
    assert!(!sample_of(&engine, &mut batches, "a", 1).await.stream_start);
    engine.emit("start", "a");
    engine.wait_for_closed_stats(1).await;
    engine.wait_for_stats_streams("a", 1).await;
    assert_eq!(engine.open_stats(), ["a"]);
    assert!(sample_of(&engine, &mut batches, "a", 2).await.stream_start);
}

#[tokio::test]
async fn a_die_event_closes_the_stream() {
    let (engine, service, _updates) = stats_engine("stats-die", &[("a", "running")]).await;
    let _batches = Batches::subscribe(&service);
    engine.wait_for_stats_streams("a", 1).await;
    engine.emit("die", "a");
    engine.wait_for_closed_stats(1).await;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(engine.open_stats(), Vec::<String>::new());
}

#[tokio::test]
async fn a_cut_stats_stream_is_reopened() {
    let (engine, service, _updates) = stats_engine("stats-cut", &[("a", "running")]).await;
    let mut batches = Batches::subscribe(&service);
    engine.wait_for_stats_streams("a", 1).await;
    assert!(sample_of(&engine, &mut batches, "a", 0).await.stream_start);
    let lists = engine.requested("/containers/json");
    let cut = Instant::now();
    engine.cut_stats("a");
    engine.wait_for_closed_stats(1).await;
    engine.wait_for_stats_streams("a", 1).await;
    assert!(
        cut.elapsed() >= Duration::from_millis(900),
        "reopened without the backoff"
    );
    assert_eq!(
        engine.requested("/containers/json"),
        lists,
        "re-listed instead of retrying"
    );
    assert_eq!(
        engine.emit("pause", "other"),
        1,
        "the events stream was dropped"
    );
    assert!(sample_of(&engine, &mut batches, "a", 1).await.stream_start);
}

#[tokio::test]
async fn a_missing_container_closes_only_its_stream() {
    let (engine, service, _updates) = stats_engine("stats-missing", &[("a", "running")]).await;
    let mut batches = Batches::subscribe(&service);
    engine.wait_for_stats_streams("a", 1).await;
    engine.missing("x");
    engine.emit("start", "x");
    // Past the first retry delay.
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(engine.requested("/containers/x/stats"), 1);
    assert_eq!(engine.open_stats(), ["a"]);
    sample_of(&engine, &mut batches, "a", 0).await;
}

#[tokio::test]
async fn a_new_generation_relists() {
    let (engine, service, _updates) = stats_engine("stats-relist", &[("a", "running")]).await;
    let mut batches = Batches::subscribe(&service);
    engine.wait_for_stats_streams("a", 1).await;
    assert!(sample_of(&engine, &mut batches, "a", 0).await.stream_start);
    let lists = engine.requested("/containers/json");
    engine.close_event_streams(false);
    engine.wait_for_closed_stats(1).await;
    engine.wait_for_stats_streams("a", 1).await;
    assert!(engine.requested("/containers/json") > lists);
    assert!(sample_of(&engine, &mut batches, "a", 1).await.stream_start);
}

#[tokio::test]
async fn cancelling_closes_every_stats_connection() {
    let containers = [("a", "running"), ("b", "running")];
    let (engine, service, _updates) = stats_engine("stats-cancel", &containers).await;
    let mut batches = Batches::subscribe(&service);
    engine.wait_for_stats_streams("a", 1).await;
    engine.wait_for_stats_streams("b", 1).await;
    service.cancel_stream(batches.id);
    engine.wait_for_closed_stats(2).await;
    assert!(
        matches!(
            tokio::time::timeout(SOON, batches.rx.recv()).await,
            Ok(None)
        ),
        "cancelling drops the sink"
    );
}

#[tokio::test]
async fn a_closed_sink_closes_every_stats_connection() {
    let containers = [("a", "running"), ("b", "running")];
    let (engine, service, _updates) = stats_engine("stats-sink", &containers).await;
    service.subscribe_stats(|_| false);
    engine.wait_for_stats_streams("a", 1).await;
    engine.wait_for_stats_streams("b", 1).await;
    engine.stats_sample("a", &read_time(0), 0, 0);
    engine.wait_for_closed_stats(2).await;
}

#[tokio::test]
async fn a_burst_of_starts_opens_one_stream_per_container() {
    let (engine, service, _updates) = stats_engine("stats-burst", &[]).await;
    let _batches = Batches::subscribe(&service);
    // The supervisor subscribes to container events once it runs.
    tokio::time::sleep(Duration::from_millis(100)).await;
    let ids: Vec<String> = (0..10).map(|i| format!("c{i}")).collect();
    for _ in 0..2 {
        for id in &ids {
            engine.emit("start", id);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    for id in &ids {
        engine.wait_for_stats_streams(id, 1).await;
    }
    // Replaced streams close shortly after the new ones open.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(engine.open_stats(), ids);
}

#[tokio::test]
async fn pause_and_unpause_keep_the_stream() {
    let (engine, service, _updates) = stats_engine("stats-pause", &[("a", "running")]).await;
    let mut batches = Batches::subscribe(&service);
    engine.wait_for_stats_streams("a", 1).await;
    engine.emit("pause", "a");
    engine.emit("unpause", "a");
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(engine.requested("/containers/a/stats"), 1);
    assert_eq!(engine.open_stats(), ["a"]);
    assert!(sample_of(&engine, &mut batches, "a", 0).await.stream_start);
}

#[tokio::test]
async fn a_lagged_event_receiver_relists() {
    let (engine, service, _updates) = stats_engine("stats-lag", &[("a", "running")]).await;
    // The supervisor reads no events while it waits for the list.
    engine.delay_lists(Duration::from_millis(500));
    let _batches = Batches::subscribe(&service);
    for _ in 0..50 {
        if engine.requested("/containers/json") == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(engine.requested("/containers/json"), 1, "no list request");
    // More than the event buffer holds.
    engine.emit_burst("start", "a", 300);
    for _ in 0..200 {
        if engine.requested("/containers/json") > 1 {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the supervisor did not re-list");
}
