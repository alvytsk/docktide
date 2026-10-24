//! Stats sampling and derived values (spec M2 FR-1, FR-2).

// Byte and nanosecond counters stay far below 2^52, so u64 to f64 is exact in practice.
#![allow(clippy::cast_precision_loss)]

use std::collections::HashMap;
use std::pin::pin;
use std::time::Duration;

use bollard::models::{ContainerCpuStats, ContainerStatsResponse};
use bollard::query_parameters::StatsOptions;
use futures_util::StreamExt;
use serde::Serialize;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::mpsc;
use tokio::time::MissedTickBehavior;
use tokio_util::sync::CancellationToken;

use crate::connection::DockerService;
use crate::containers::ContainerState;
use crate::error::DockerError;
use crate::events::{Backoff, ContainerEvent};
use crate::logs::{normalize_timestamp, unix_seconds};
use crate::streams::SubscriptionId;

/// Samples of all streams leave together once per tick.
pub(crate) const BATCH_TICK: Duration = Duration::from_secs(1);

/// One derived sample for one container; `None` fields serialize as `null`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct StatsSample {
    pub id: String,
    /// Docker's `read` time, as sent.
    pub time: String,
    /// First sample of a stream; charts break the line here.
    pub stream_start: bool,
    /// 100 is one core.
    pub cpu_percent: Option<f64>,
    #[cfg_attr(test, ts(type = "number | null"))]
    pub memory_usage: Option<u64>,
    #[cfg_attr(test, ts(type = "number | null"))]
    pub memory_limit: Option<u64>,
    /// Bytes per second.
    pub network_rx: Option<f64>,
    pub network_tx: Option<f64>,
    pub block_read: Option<f64>,
    pub block_write: Option<f64>,
    #[cfg_attr(test, ts(type = "number | null"))]
    pub pids: Option<u64>,
}

/// Counters of the previous sample of a stream, for rates.
struct Previous {
    seconds: f64,
    network: Option<(u64, u64)>,
    block: Option<(u64, u64)>,
}

/// Derives samples of one stream; make a new one per stream.
#[derive(Default)]
pub(crate) struct Deriver {
    previous: Option<Previous>,
}

impl Deriver {
    /// `None` when `read` is missing or the zero time: the caller closes the stream.
    pub(crate) fn next(&mut self, id: &str, raw: &ContainerStatsResponse) -> Option<StatsSample> {
        let time = raw.read.as_deref()?;
        if time.starts_with("0001-01-01") {
            return None;
        }
        let fraction = normalize_timestamp(time)
            .get(20..29)
            .and_then(|f| f.parse::<u32>().ok())?;
        let seconds = unix_seconds(time)? as f64 + f64::from(fraction) / 1e9;

        let memory = raw.memory_stats.as_ref();
        let network = raw.networks.as_ref().filter(|n| !n.is_empty()).map(|n| {
            n.values().fold((0, 0), |(rx, tx), i| {
                (rx + i.rx_bytes.unwrap_or(0), tx + i.tx_bytes.unwrap_or(0))
            })
        });
        let block = raw
            .blkio_stats
            .as_ref()
            .and_then(|b| b.io_service_bytes_recursive.as_ref())
            .map(|entries| {
                entries.iter().fold((0, 0), |(read, write), e| {
                    let value = e.value.unwrap_or(0);
                    match e.op.as_deref().map(str::to_ascii_lowercase).as_deref() {
                        Some("read") => (read + value, write),
                        Some("write") => (read, write + value),
                        _ => (read, write),
                    }
                })
            });

        let stream_start = self.previous.is_none();
        let previous = self.previous.replace(Previous {
            seconds,
            network,
            block,
        });
        let elapsed = previous
            .as_ref()
            .map(|p| seconds - p.seconds)
            .filter(|e| *e > 0.0);
        // Bytes per second; `None` without a previous sample or after a reset.
        let rate = |now: u64, before: Option<u64>| {
            let (before, elapsed) = (before?, elapsed?);
            (now >= before).then(|| (now - before) as f64 / elapsed)
        };
        let (prev_network, prev_block) = previous.map_or((None, None), |p| (p.network, p.block));

        Some(StatsSample {
            id: id.to_owned(),
            time: time.to_owned(),
            stream_start,
            cpu_percent: raw
                .cpu_stats
                .as_ref()
                .and_then(|c| cpu_percent(c, raw.precpu_stats.as_ref()?)),
            memory_usage: memory.and_then(|m| {
                let usage = m.usage?;
                let stat = |key| m.stats.as_ref().and_then(|s| s.get(key)).copied();
                let cache = stat("total_inactive_file")
                    .filter(|c| *c < usage)
                    .or_else(|| Some(stat("inactive_file").unwrap_or(0)).filter(|c| *c < usage));
                Some(usage - cache.unwrap_or(0))
            }),
            memory_limit: memory.and_then(|m| m.limit).filter(|l| *l > 0),
            network_rx: network.and_then(|n| rate(n.0, prev_network.map(|p| p.0))),
            network_tx: network.and_then(|n| rate(n.1, prev_network.map(|p| p.1))),
            block_read: block.and_then(|b| rate(b.0, prev_block.map(|p| p.0))),
            block_write: block.and_then(|b| rate(b.1, prev_block.map(|p| p.1))),
            pids: raw.pids_stats.as_ref().and_then(|p| p.current),
        })
    }
}

fn cpu_percent(now: &ContainerCpuStats, before: &ContainerCpuStats) -> Option<f64> {
    let total = |s: &ContainerCpuStats| s.cpu_usage.as_ref()?.total_usage;
    let cpu_delta = total(now)?.checked_sub(total(before)?)? as f64;
    let system_delta = now
        .system_cpu_usage?
        .checked_sub(before.system_cpu_usage?)
        .filter(|d| *d > 0)? as f64;
    let online = now
        .online_cpus
        .filter(|c| *c > 0)
        .map(f64::from)
        .or_else(|| {
            let n = now.cpu_usage.as_ref()?.percpu_usage.as_ref()?.len();
            (n > 0).then_some(n as f64)
        })
        .unwrap_or(1.0);
    Some(cpu_delta / system_delta * online * 100.0)
}

impl DockerService {
    /// Starts the stats supervisor and replaces a previous one. It keeps one
    /// stats stream per running or paused container and sends their samples to
    /// `sink` once per `BATCH_TICK`, until cancelled with `cancel_stream` or
    /// until `sink` returns false. Must be called within a Tokio runtime.
    pub fn subscribe_stats(
        &self,
        sink: impl Fn(Vec<StatsSample>) -> bool + Send + Sync + 'static,
    ) -> SubscriptionId {
        let (id, cancel) = self.streams().replace_stats();
        let service = self.clone();
        tokio::spawn(async move {
            tokio::select! {
                () = supervise(&service, &sink, &cancel) => {}
                () = cancel.cancelled() => {}
            }
            service.streams().remove(id);
        });
        id
    }
}

/// What a container event means for its stream.
#[derive(Debug, PartialEq, Eq)]
enum Action {
    Open(String),
    Close(String),
    Relist,
    Ignore,
}

fn on_event(event: Result<ContainerEvent, RecvError>) -> Action {
    match event {
        Ok(ContainerEvent { id, action }) => match action.as_str() {
            "start" => Action::Open(id),
            "die" | "destroy" => Action::Close(id),
            _ => Action::Ignore,
        },
        // Missed events may include starts.
        Err(RecvError::Lagged(_)) => Action::Relist,
        // The service owns the sender, so this does not happen.
        Err(RecvError::Closed) => Action::Ignore,
    }
}

/// An open stream of one container.
struct Running {
    cancel: CancellationToken,
    /// Tells this stream's exit apart from that of a stream it replaced.
    serial: u64,
}

/// The streams of one subscription. Each is a child of `parent`.
struct Streams<'a> {
    service: &'a DockerService,
    parent: CancellationToken,
    running: HashMap<String, Running>,
    serial: u64,
    samples: mpsc::UnboundedSender<StatsSample>,
    exits: mpsc::UnboundedSender<(String, u64)>,
}

impl Streams<'_> {
    /// Replaces the container's stream, if any, with a new one.
    fn open(&mut self, id: String) {
        self.close(&id);
        self.serial += 1;
        let cancel = self.parent.child_token();
        let running = Running {
            cancel: cancel.clone(),
            serial: self.serial,
        };
        self.running.insert(id.clone(), running);
        let (service, samples, exits, serial) = (
            self.service.clone(),
            self.samples.clone(),
            self.exits.clone(),
            self.serial,
        );
        tokio::spawn(async move {
            tokio::select! {
                () = follow(&service, &id, &samples) => {}
                () = cancel.cancelled() => {}
            }
            let _ = exits.send((id, serial));
        });
    }

    fn close(&mut self, id: &str) {
        if let Some(running) = self.running.remove(id) {
            running.cancel.cancel();
        }
    }

    /// Forgets a stream that ended by itself, unless it was replaced since.
    fn exited(&mut self, id: &str, serial: u64) {
        if self.running.get(id).is_some_and(|r| r.serial == serial) {
            self.running.remove(id);
        }
    }

    /// Closes every stream and opens one per running or paused container.
    async fn relist(&mut self) {
        for (_, running) in self.running.drain() {
            running.cancel.cancel();
        }
        match self.service.list_containers().await {
            Ok(containers) => {
                for container in containers {
                    if matches!(
                        container.state,
                        ContainerState::Running | ContainerState::Paused
                    ) {
                        self.open(container.id);
                    }
                }
            }
            // The next generation change lists again.
            Err(error) => tracing::debug!(%error, "listing containers for stats failed"),
        }
    }
}

/// Follows container lifecycle and sends batches of samples. Returns only when
/// the sink is closed. Every stream closes when this returns or is dropped.
async fn supervise(
    service: &DockerService,
    sink: &(dyn Fn(Vec<StatsSample>) -> bool + Send + Sync),
    cancel: &CancellationToken,
) {
    let mut generation = service.generation();
    let mut events = service.container_events();
    let (samples, mut received) = mpsc::unbounded_channel();
    let (exits, mut exited) = mpsc::unbounded_channel();
    let parent = cancel.child_token();
    let _close_streams = parent.clone().drop_guard();
    let mut streams = Streams {
        service,
        parent,
        running: HashMap::new(),
        serial: 0,
        samples,
        exits,
    };
    let mut tick = tokio::time::interval(BATCH_TICK);
    tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut relist = true;
    loop {
        if relist {
            relist = false;
            // Events from before the list are in it.
            events = events.resubscribe();
            streams.relist().await;
            // A reconnect by the list itself needs no second list.
            generation.mark_unchanged();
        }
        tokio::select! {
            _ = generation.changed() => relist = true,
            event = events.recv() => match on_event(event) {
                Action::Open(id) => streams.open(id),
                Action::Close(id) => streams.close(&id),
                Action::Relist => relist = true,
                Action::Ignore => {}
            },
            Some((id, serial)) = exited.recv() => streams.exited(&id, serial),
            _ = tick.tick() => {
                let mut batch = Vec::new();
                while let Ok(sample) = received.try_recv() {
                    batch.push(sample);
                }
                if !batch.is_empty() && !sink(batch) {
                    return;
                }
            }
        }
    }
}

/// Follows one container's stats, retrying failed streams, until the container
/// stops or is gone, the connection is lost, or the supervisor is gone.
async fn follow(service: &DockerService, id: &str, samples: &mpsc::UnboundedSender<StatsSample>) {
    let mut backoff = Backoff::default();
    loop {
        let Ok((endpoint, docker)) = service.docker().await else {
            return;
        };
        let options = StatsOptions {
            stream: true,
            one_shot: false,
        };
        let mut stats = pin!(docker.stats(id, Some(options)));
        let mut deriver = Deriver::default();
        let failure = loop {
            match stats.next().await {
                Some(Ok(raw)) => {
                    let Some(sample) = deriver.next(id, &raw) else {
                        if let Some(read) = raw
                            .read
                            .as_deref()
                            .filter(|r| !r.is_empty() && !r.starts_with("0001-01-01"))
                        {
                            tracing::debug!(id, read, "unparseable stats read time");
                        }
                        return;
                    };
                    backoff.reset();
                    if samples.send(sample).is_err() {
                        return;
                    }
                }
                Some(Err(err)) => break Some(err),
                None => break None,
            }
        };
        // Not `stream_failure`: one cut stats connection says nothing about the
        // Engine. A retry that cannot connect is classified as connection-level.
        match failure.map(|err| service.failed(&err, &endpoint)) {
            Some(DockerError::NotFound) => return,
            Some(error) if error.is_connection_level() => {
                tracing::debug!(id, %error, "stats stream lost its connection");
                return;
            }
            Some(error) => tracing::debug!(id, %error, "stats stream failed"),
            None => tracing::debug!(id, "stats stream ended"),
        }
        tokio::time::sleep(backoff.next()).await;
    }
}

#[cfg(test)]
#[allow(
    clippy::needless_pass_by_value,
    reason = "json! literals are passed by value"
)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    const MIB: u64 = 1024 * 1024;

    fn raw(value: Value) -> ContainerStatsResponse {
        serde_json::from_value(value).unwrap()
    }

    fn cpu(total: u64, system: Option<u64>, online: Option<u32>, percpu: &[u64]) -> Value {
        let mut v = json!({"cpu_usage": {"total_usage": total, "percpu_usage": percpu}});
        if let Some(s) = system {
            v["system_cpu_usage"] = json!(s);
        }
        if let Some(o) = online {
            v["online_cpus"] = json!(o);
        }
        v
    }

    fn cpu_sample(now: Value, before: Value) -> Option<f64> {
        let raw =
            raw(json!({"read": "2026-10-24T10:00:00Z", "cpu_stats": now, "precpu_stats": before}));
        Deriver::default().next("c", &raw).unwrap().cpu_percent
    }

    fn mem_sample(memory: Value) -> StatsSample {
        let raw = raw(json!({"read": "2026-10-24T10:00:00Z", "memory_stats": memory}));
        Deriver::default().next("c", &raw).unwrap()
    }

    fn usage(usage: u64, stats: Value) -> Option<u64> {
        mem_sample(json!({"usage": usage, "stats": stats})).memory_usage
    }

    #[test]
    fn cpu_percent_matches_the_cli() {
        let got = cpu_sample(
            cpu(2_000_000_000, Some(20_000_000_000), Some(4), &[]),
            cpu(1_000_000_000, Some(10_000_000_000), Some(4), &[]),
        );
        assert_eq!(got, Some(40.0));
    }

    #[test]
    fn zero_cpu_delta_is_zero() {
        let got = cpu_sample(
            cpu(1_000_000_000, Some(11_000_000_000), Some(4), &[]),
            cpu(1_000_000_000, Some(10_000_000_000), Some(4), &[]),
        );
        assert_eq!(got, Some(0.0));
    }

    #[test]
    fn negative_cpu_delta_is_null() {
        let got = cpu_sample(
            cpu(1_000_000_000, Some(20_000_000_000), Some(4), &[]),
            cpu(2_000_000_000, Some(10_000_000_000), Some(4), &[]),
        );
        assert_eq!(got, None);
    }

    #[test]
    fn first_sample_has_no_cpu() {
        let got = cpu_sample(
            cpu(1_000_000_000, Some(20_000_000_000), Some(4), &[]),
            cpu(0, None, None, &[]),
        );
        assert_eq!(got, None);
    }

    #[test]
    fn zero_system_delta_is_null() {
        let got = cpu_sample(
            cpu(2_000_000_000, Some(10_000_000_000), Some(4), &[]),
            cpu(1_000_000_000, Some(10_000_000_000), Some(4), &[]),
        );
        assert_eq!(got, None);
    }

    #[test]
    fn online_cpus_falls_back_to_percpu() {
        let got = cpu_sample(
            cpu(2_000_000_000, Some(20_000_000_000), None, &[1, 2]),
            cpu(1_000_000_000, Some(10_000_000_000), None, &[1, 2]),
        );
        assert_eq!(got, Some(20.0));
    }

    #[test]
    fn memory_v2_subtracts_inactive_file() {
        assert_eq!(
            usage(100 * MIB, json!({"inactive_file": 30 * MIB})),
            Some(70 * MIB)
        );
    }

    #[test]
    fn memory_v1_subtracts_total_inactive() {
        let stats = json!({"total_inactive_file": 10, "inactive_file": 30});
        assert_eq!(usage(100, stats), Some(90));
    }

    #[test]
    fn cache_not_below_usage_keeps_usage() {
        assert_eq!(usage(100, json!({"inactive_file": 100})), Some(100));
        assert_eq!(usage(100, json!({"inactive_file": 150})), Some(100));
    }

    #[test]
    fn absent_memory_is_null() {
        let sample = mem_sample(json!({}));
        assert_eq!((sample.memory_usage, sample.memory_limit), (None, None));
    }

    #[test]
    fn zero_limit_is_null() {
        assert_eq!(mem_sample(json!({"limit": 0})).memory_limit, None);
        assert_eq!(
            mem_sample(json!({"limit": 1024 * MIB})).memory_limit,
            Some(1024 * MIB)
        );
    }

    fn io(
        read: &str,
        rx: u64,
        tx: u64,
        block_read: u64,
        op: (&str, &str),
    ) -> ContainerStatsResponse {
        raw(json!({
            "read": read,
            "networks": {"eth0": {"rx_bytes": rx, "tx_bytes": tx}},
            "blkio_stats": {"io_service_bytes_recursive": [
                {"major": 8, "minor": 0, "op": op.0, "value": block_read},
                {"major": 8, "minor": 0, "op": op.1, "value": 0},
            ]},
        }))
    }

    #[test]
    fn rates_need_a_previous_sample() {
        let s = Deriver::default()
            .next("c", &io("2026-10-24T10:00:00Z", 1, 2, 3, ("read", "write")))
            .unwrap();
        assert_eq!(
            (s.network_rx, s.network_tx, s.block_read, s.block_write),
            (None, None, None, None)
        );
    }

    #[test]
    fn rates_divide_by_read_time() {
        let mut d = Deriver::default();
        d.next(
            "c",
            &io("2026-10-24T10:00:00.5Z", 1000, 0, 0, ("read", "write")),
        );
        let s = d
            .next(
                "c",
                &io("2026-10-24T10:00:02.5Z", 5000, 0, 4096, ("read", "write")),
            )
            .unwrap();
        assert_eq!(s.network_rx, Some(2000.0));
        assert_eq!(s.block_read, Some(2048.0));
        assert_eq!(s.block_write, Some(0.0));
    }

    #[test]
    fn counter_reset_is_null() {
        let mut d = Deriver::default();
        d.next(
            "c",
            &io("2026-10-24T10:00:00Z", 5000, 0, 0, ("read", "write")),
        );
        let s = d
            .next(
                "c",
                &io("2026-10-24T10:00:01Z", 1000, 0, 0, ("read", "write")),
            )
            .unwrap();
        assert_eq!(s.network_rx, None);
    }

    #[test]
    fn no_networks_is_null() {
        let mut d = Deriver::default();
        let plain = |read| raw(json!({"read": read}));
        d.next("c", &plain("2026-10-24T10:00:00Z"));
        let s = d.next("c", &plain("2026-10-24T10:00:01Z")).unwrap();
        assert_eq!((s.network_rx, s.network_tx), (None, None));
    }

    #[test]
    fn block_ops_are_case_insensitive() {
        let mut d = Deriver::default();
        d.next("c", &io("2026-10-24T10:00:00Z", 0, 0, 0, ("Read", "Write")));
        let s = d
            .next(
                "c",
                &io("2026-10-24T10:00:01Z", 0, 0, 100, ("Read", "Write")),
            )
            .unwrap();
        assert_eq!(s.block_read, Some(100.0));
    }

    #[test]
    fn zero_read_time_ends_the_stream() {
        let zero = raw(json!({"read": "0001-01-01T00:00:00Z"}));
        assert_eq!(Deriver::default().next("c", &zero), None);
        assert_eq!(Deriver::default().next("c", &raw(json!({}))), None);
    }

    #[test]
    fn stream_start_only_on_the_first() {
        let mut d = Deriver::default();
        let starts: Vec<bool> = (0..3)
            .map(|i| {
                let r = raw(json!({"read": format!("2026-10-24T10:00:0{i}Z")}));
                d.next("c", &r).unwrap().stream_start
            })
            .collect();
        assert_eq!(starts, [true, false, false]);
    }

    #[test]
    fn pids_are_reported() {
        let r = raw(json!({"read": "2026-10-24T10:00:00Z", "pids_stats": {"current": 7}}));
        assert_eq!(Deriver::default().next("c", &r).unwrap().pids, Some(7));
    }

    #[expect(clippy::unnecessary_wraps, reason = "events arrive as results")]
    fn event(action: &str, id: &str) -> Result<ContainerEvent, RecvError> {
        Ok(ContainerEvent {
            id: id.into(),
            action: action.into(),
        })
    }

    #[test]
    fn start_opens() {
        assert_eq!(on_event(event("start", "a")), Action::Open("a".into()));
    }

    #[test]
    fn die_and_destroy_close() {
        assert_eq!(on_event(event("die", "a")), Action::Close("a".into()));
        assert_eq!(on_event(event("destroy", "a")), Action::Close("a".into()));
    }

    #[test]
    fn other_actions_are_ignored() {
        for action in [
            "create", "stop", "kill", "pause", "unpause", "restart", "rename",
        ] {
            assert_eq!(on_event(event(action, "a")), Action::Ignore, "{action}");
        }
    }

    #[test]
    fn lag_relists() {
        assert_eq!(on_event(Err(RecvError::Lagged(3))), Action::Relist);
    }

    #[test]
    fn serializes_to_the_ipc_shape() {
        let r = raw(json!({"read": "2026-10-24T10:00:00Z"}));
        let v = serde_json::to_value(Deriver::default().next("c", &r).unwrap()).unwrap();
        let mut keys: Vec<_> = v.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        let mut want = [
            "id",
            "time",
            "streamStart",
            "cpuPercent",
            "memoryUsage",
            "memoryLimit",
            "networkRx",
            "networkTx",
            "blockRead",
            "blockWrite",
            "pids",
        ];
        want.sort_unstable();
        assert_eq!(keys, want);
    }
}
