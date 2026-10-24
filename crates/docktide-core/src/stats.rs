//! Stats sampling and derived values (spec M2 FR-1, FR-2).

// Byte and nanosecond counters stay far below 2^52, so u64 to f64 is exact in practice.
#![allow(clippy::cast_precision_loss)]

use bollard::models::{ContainerCpuStats, ContainerStatsResponse};
use serde::Serialize;

use crate::logs::{normalize_timestamp, unix_seconds};

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
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "used by the stats supervisor (M2 Task 3)")
)]
pub(crate) struct Deriver {
    previous: Option<Previous>,
}

#[cfg_attr(
    not(test),
    expect(dead_code, reason = "used by the stats supervisor (M2 Task 3)")
)]
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
