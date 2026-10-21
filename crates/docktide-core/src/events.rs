//! Docker events supervisor and live update hints (spec M1 FR-1).

use std::collections::HashMap;
use std::pin::pin;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bollard::Docker;
use bollard::errors::Error as BollardError;
use bollard::models::{EventMessage, EventMessageTypeEnum};
use bollard::query_parameters::EventsOptions;
use futures_util::{Stream, StreamExt};
use serde::Serialize;
use tokio::time::{Instant, sleep_until};

use crate::connection::DockerService;
use crate::endpoint::ResolvedEndpoint;
use crate::error::{DockerError, IpcError};
use crate::streams::SubscriptionId;

/// Tells the frontend what to refetch. Carries no Docker data itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum LiveUpdate {
    /// The events stream is open; anything may have changed while it was not.
    Connected,
    Disconnected {
        error: IpcError,
    },
    ContainersChanged {
        ids: Vec<String>,
    },
}

/// Relevant events are collected this long before one hint leaves Rust.
const COALESCE_WINDOW: Duration = Duration::from_millis(250);

/// Container actions that change what the list or a details page shows. An
/// allowlist, so actions added by newer Engines do not cause refetches.
fn is_relevant(action: &str) -> bool {
    const ACTIONS: [&str; 12] = [
        "create", "start", "restart", "stop", "die", "kill", "oom", "pause", "unpause", "destroy",
        "rename", "update",
    ];
    ACTIONS.contains(&action) || action.starts_with("health_status")
}

/// Container id of an event that should trigger a refetch.
fn relevant_id(event: EventMessage) -> Option<String> {
    if event.typ != Some(EventMessageTypeEnum::CONTAINER)
        || !event.action.as_deref().is_some_and(is_relevant)
    {
        return None;
    }
    event.actor?.id
}

/// Retry delays of 1, 2, 4, 8, and 16 s, then 30 s.
#[derive(Debug, Default)]
struct Backoff {
    attempt: u32,
}

impl Backoff {
    fn next(&mut self) -> Duration {
        let secs = (1 << self.attempt.min(5)).min(30);
        self.attempt += 1;
        Duration::from_secs(secs)
    }

    fn reset(&mut self) {
        self.attempt = 0;
    }
}

impl DockerService {
    /// Starts the events supervisor and replaces a previous one. It runs until
    /// cancelled with `cancel_stream` or until `sink` returns false. Must be
    /// called within a Tokio runtime.
    pub fn subscribe_live_updates(
        &self,
        sink: impl Fn(LiveUpdate) -> bool + Send + Sync + 'static,
    ) -> SubscriptionId {
        let (id, cancel) = self.streams().replace_live_updates();
        let service = self.clone();
        tokio::spawn(async move {
            tokio::select! {
                () = supervise(&service, &sink) => {}
                () = cancel.cancelled() => {}
            }
            service.streams().remove(id);
        });
        id
    }

    /// A client whose Engine answered just now.
    async fn checked_client(&self) -> Result<(ResolvedEndpoint, Docker), DockerError> {
        let (endpoint, docker) = self.docker().await?;
        docker
            .ping()
            .await
            .map_err(|err| self.failed(&err, &endpoint))?;
        Ok((endpoint, docker))
    }
}

/// Keeps one events stream open on the current client and reports its state.
/// Returns only when the sink is closed.
async fn supervise(service: &DockerService, sink: &(dyn Fn(LiveUpdate) -> bool + Send + Sync)) {
    let mut generation = service.generation();
    let mut backoff = Backoff::default();
    // Only state changes are reported; `None` until the first attempt.
    let mut connected = None;
    loop {
        // Asking for events since before the check closes the gap between the
        // check, the frontend's refetch, and the stream actually opening.
        let since = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let result = service.checked_client().await;
        generation.mark_unchanged();
        let error = match result {
            Ok((endpoint, docker)) => {
                if !sink(LiveUpdate::Connected) {
                    return;
                }
                connected = Some(true);
                backoff.reset();
                tracing::info!(endpoint = %endpoint.uri, "Docker events stream open");
                tokio::select! {
                    end = forward(docker.events(Some(container_events(since))), sink) => {
                        let error = match end {
                            End::SinkClosed => return,
                            End::Failed(err) => stream_failure(service, &err, endpoint),
                            End::Eof => DockerError::DaemonUnavailable { endpoint: endpoint.uri },
                        };
                        // The next status check reconnects and reports the truth.
                        service.drop_client();
                        error
                    }
                    // A new endpoint, or a reconnect by another request.
                    _ = generation.changed() => continue,
                }
            }
            Err(error) => error,
        };
        generation.mark_unchanged();
        if connected != Some(false) {
            tracing::info!(%error, "Docker events stream down");
            if !sink(LiveUpdate::Disconnected {
                error: error.into(),
            }) {
                return;
            }
            connected = Some(false);
        }
        // A successful request elsewhere connects a client and ends the wait.
        tokio::select! {
            () = tokio::time::sleep(backoff.next()) => {}
            _ = generation.changed() => {}
        }
    }
}

/// A transport error while reading an open stream means the connection is gone,
/// even when no `io::Error` says so (hyper reports a cut body without one).
fn stream_failure(
    service: &DockerService,
    err: &BollardError,
    endpoint: ResolvedEndpoint,
) -> DockerError {
    match service.failed(err, &endpoint) {
        DockerError::Internal { .. }
            if !matches!(err, BollardError::DockerResponseServerError { .. }) =>
        {
            DockerError::DaemonUnavailable {
                endpoint: endpoint.uri,
            }
        }
        error => error,
    }
}

fn container_events(since: Duration) -> EventsOptions {
    EventsOptions {
        since: Some(format!("{}.{:09}", since.as_secs(), since.subsec_nanos())),
        until: None,
        filters: Some(HashMap::from([(
            "type".to_owned(),
            vec!["container".to_owned()],
        )])),
    }
}

/// Why `forward` returned.
#[derive(Debug)]
enum End {
    /// The Engine closed the stream.
    Eof,
    Failed(BollardError),
    /// The subscriber is gone.
    SinkClosed,
}

/// Sends coalesced `ContainersChanged` hints for `events` until it ends.
/// Pending ids are flushed before returning on `Eof` or `Failed`.
async fn forward(
    events: impl Stream<Item = Result<EventMessage, BollardError>>,
    sink: &(dyn Fn(LiveUpdate) -> bool + Send + Sync),
) -> End {
    let mut events = pin!(events);
    let mut ids: Vec<String> = Vec::new();
    let mut flush_at: Option<Instant> = None;
    let flush = |ids: &mut Vec<String>| {
        ids.is_empty()
            || sink(LiveUpdate::ContainersChanged {
                ids: std::mem::take(ids),
            })
    };
    loop {
        let window = async {
            match flush_at {
                Some(at) => sleep_until(at).await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            item = events.next() => match item {
                Some(Ok(event)) => {
                    if let Some(id) = relevant_id(event) {
                        if !ids.contains(&id) {
                            ids.push(id);
                        }
                        flush_at.get_or_insert_with(|| Instant::now() + COALESCE_WINDOW);
                    }
                }
                Some(Err(err)) => {
                    return if flush(&mut ids) { End::Failed(err) } else { End::SinkClosed };
                }
                None => return if flush(&mut ids) { End::Eof } else { End::SinkClosed },
            },
            () = window => {
                flush_at = None;
                if !flush(&mut ids) {
                    return End::SinkClosed;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use bollard::models::EventActor;
    use futures_util::stream;
    use serde_json::json;
    use tokio::sync::mpsc;
    use tokio::time::Instant;

    use super::*;
    use crate::error::DockerError;

    fn event(typ: EventMessageTypeEnum, action: &str, id: &str) -> EventMessage {
        EventMessage {
            typ: Some(typ),
            action: Some(action.into()),
            actor: Some(EventActor {
                id: Some(id.into()),
                attributes: None,
            }),
            ..Default::default()
        }
    }

    #[expect(clippy::unnecessary_wraps, reason = "stream items are results")]
    fn container(action: &str, id: &str) -> Result<EventMessage, BollardError> {
        Ok(event(EventMessageTypeEnum::CONTAINER, action, id))
    }

    fn changed(ids: &[&str]) -> LiveUpdate {
        LiveUpdate::ContainersChanged {
            ids: ids.iter().map(|&id| id.to_owned()).collect(),
        }
    }

    /// Records hints with the time they were sent.
    #[derive(Clone, Default)]
    struct Recorder(Arc<Mutex<Vec<(Duration, LiveUpdate)>>>);

    impl Recorder {
        fn sink(&self, start: Instant) -> impl Fn(LiveUpdate) -> bool + Send + Sync + use<> {
            let log = Arc::clone(&self.0);
            move |update| {
                log.lock().unwrap().push((start.elapsed(), update));
                true
            }
        }

        fn updates(&self) -> Vec<LiveUpdate> {
            self.0
                .lock()
                .unwrap()
                .iter()
                .map(|(_, u)| u.clone())
                .collect()
        }

        fn times(&self) -> Vec<Duration> {
            self.0.lock().unwrap().iter().map(|(t, _)| *t).collect()
        }
    }

    /// A stream fed by the test; ends when the sender is dropped.
    fn channel() -> (
        mpsc::UnboundedSender<Result<EventMessage, BollardError>>,
        impl Stream<Item = Result<EventMessage, BollardError>>,
    ) {
        let (tx, mut rx) = mpsc::unbounded_channel();
        (tx, stream::poll_fn(move |cx| rx.poll_recv(cx)))
    }

    #[test]
    fn state_changing_actions_are_relevant() {
        for action in [
            "create", "start", "restart", "stop", "die", "kill", "oom", "pause", "unpause",
            "destroy", "rename", "update",
        ] {
            assert!(is_relevant(action), "{action}");
        }
    }

    #[test]
    fn health_status_changes_are_relevant() {
        assert!(is_relevant("health_status: healthy"));
        assert!(is_relevant("health_status: unhealthy"));
        assert!(is_relevant("health_status"));
    }

    #[test]
    fn healthcheck_runs_and_other_actions_are_ignored() {
        for action in [
            "exec_create: /bin/sh -c true",
            "exec_start: /bin/sh -c true",
            "exec_die",
            "attach",
            "resize",
            "top",
            "archive-path",
            "extract-to-dir",
            "export",
            "commit",
            "",
        ] {
            assert!(!is_relevant(action), "{action}");
        }
    }

    #[test]
    fn only_container_events_with_an_id_count() {
        assert_eq!(
            relevant_id(event(EventMessageTypeEnum::CONTAINER, "start", "abc")),
            Some("abc".into())
        );
        assert_eq!(
            relevant_id(event(EventMessageTypeEnum::IMAGE, "delete", "abc")),
            None
        );
        let mut anonymous = event(EventMessageTypeEnum::CONTAINER, "start", "abc");
        anonymous.actor = None;
        assert_eq!(relevant_id(anonymous), None);
        assert_eq!(
            relevant_id(event(EventMessageTypeEnum::CONTAINER, "exec_die", "abc")),
            None
        );
    }

    #[test]
    fn backoff_doubles_up_to_thirty_seconds() {
        let mut backoff = Backoff::default();
        let delays: Vec<u64> = (0..8).map(|_| backoff.next().as_secs()).collect();
        assert_eq!(delays, [1, 2, 4, 8, 16, 30, 30, 30]);
    }

    #[test]
    fn backoff_restarts_after_reset() {
        let mut backoff = Backoff::default();
        backoff.next();
        backoff.next();
        backoff.reset();
        assert_eq!(backoff.next(), Duration::from_secs(1));
    }

    #[test]
    fn live_updates_serialize_to_the_ipc_shape() {
        let error: IpcError = DockerError::Timeout.into();
        assert_eq!(
            serde_json::to_value(LiveUpdate::Connected).unwrap(),
            json!({ "type": "connected" })
        );
        assert_eq!(
            serde_json::to_value(LiveUpdate::Disconnected { error }).unwrap(),
            json!({
                "type": "disconnected",
                "error": { "kind": "timeout", "message": "Docker did not respond in time." },
            })
        );
        assert_eq!(
            serde_json::to_value(changed(&["a", "b"])).unwrap(),
            json!({ "type": "containersChanged", "ids": ["a", "b"] })
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_burst_becomes_one_hint_after_the_window() {
        let recorder = Recorder::default();
        let sink = recorder.sink(Instant::now());
        let (tx, events) = channel();
        let task = tokio::spawn(async move { forward(events, &sink).await });
        tx.send(container("start", "a")).unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        tx.send(container("die", "b")).unwrap();
        tx.send(container("stop", "a")).unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(
            recorder.updates().is_empty(),
            "sent before the window ended"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
        drop(tx);
        assert!(matches!(task.await.unwrap(), End::Eof));
        assert_eq!(recorder.updates(), [changed(&["a", "b"])]);
        assert_eq!(recorder.times(), [COALESCE_WINDOW]);
    }

    #[tokio::test(start_paused = true)]
    async fn separate_bursts_become_separate_hints() {
        let recorder = Recorder::default();
        let sink = recorder.sink(Instant::now());
        let (tx, events) = channel();
        let task = tokio::spawn(async move { forward(events, &sink).await });
        tx.send(container("start", "a")).unwrap();
        tokio::time::sleep(Duration::from_secs(1)).await;
        tx.send(container("start", "b")).unwrap();
        tokio::time::sleep(Duration::from_secs(1)).await;
        drop(tx);
        task.await.unwrap();
        assert_eq!(recorder.updates(), [changed(&["a"]), changed(&["b"])]);
    }

    #[tokio::test(start_paused = true)]
    async fn irrelevant_events_send_nothing() {
        let recorder = Recorder::default();
        let sink = recorder.sink(Instant::now());
        let events = stream::iter([
            container("exec_start: /bin/sh -c true", "a"),
            Ok(event(EventMessageTypeEnum::IMAGE, "pull", "busybox")),
        ]);
        assert!(matches!(forward(events, &sink).await, End::Eof));
        assert_eq!(recorder.updates(), []);
    }

    #[tokio::test(start_paused = true)]
    async fn pending_ids_are_flushed_when_the_stream_fails() {
        let recorder = Recorder::default();
        let sink = recorder.sink(Instant::now());
        let events = stream::iter([
            container("start", "a"),
            Err(BollardError::RequestTimeoutError),
        ]);
        assert!(matches!(
            forward(events, &sink).await,
            End::Failed(BollardError::RequestTimeoutError)
        ));
        assert_eq!(recorder.updates(), [changed(&["a"])]);
    }

    #[tokio::test(start_paused = true)]
    async fn a_closed_sink_stops_forwarding() {
        let (tx, events) = channel();
        tx.send(container("start", "a")).unwrap();
        let end = forward(events, &|_| false).await;
        assert!(matches!(end, End::SinkClosed));
    }
}
