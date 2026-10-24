# M2 Monitoring Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Live CPU, memory, network, block I/O, and PID stats for every running container, shown as list columns and as four live charts on a new Stats tab.

**Architecture:** A stats supervisor in `docktide-core` follows `GET /containers/{id}/stats?stream=1` per running container, learns lifecycle from the existing events supervisor (fanned out before coalescing), derives values with the Docker CLI's formulas, and sends one batch per second over one app-wide Tauri `Channel`. The frontend keeps 10 minutes of history in a store outside React and draws it with our own canvas chart component.

**Tech Stack:** Rust (Bollard 0.21.1, Tokio, tokio-util), Tauri 2 `Channel`, React 19, TanStack Router/Query, Zustand, Vitest via `vp test`. No new dependencies.

**Spec:** `docs/specifications/m2-monitoring.md` (approved 2026-10-24). Read it first; FR/AC numbers below refer to it.

## Global Constraints

- No new Rust or npm dependencies (§10 D1: the chart is ours).
- Work on branch `m2-monitoring`; one commit per task, message style as in `git log` (`Add … (M2 slice Sn)`).
- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings` (pedantic is on), `cargo test --workspace`, `pnpm check`, `pnpm test`, `pnpm typecheck` pass at the end of every task.
- DTOs derive `ts_rs::TS` under `#[cfg_attr(test, derive(ts_rs::TS))]`, use `#[serde(rename_all = "camelCase")]`, and are added to the `bindings` test in `crates/docktide-core/src/lib.rs`; regenerate with `DOCKTIDE_UPDATE_BINDINGS=1 cargo test -p docktide-core bindings`.
- New commands are listed in `src-tauri/build.rs` and granted in `src-tauri/capabilities/default.json`; `invoke()` is called only from `src/lib/ipc.ts`.
- Docker integration tests only touch containers labelled `io.docktide.test=true`, removed by a drop guard; they are `#[ignore = "requires Docker"]`.
- Constants from the spec: batch tick 1 s; history 10 min (600 000 ms); break when samples are more than 2 500 ms apart; current sample valid for 3 000 ms; stream retry backoff 1, 2, 4, 8, 16, then 30 s (reuse `events::Backoff`).
- Display: `—` (U+2014) for missing values; CPU `12.3 %` (one decimal, trailing `.0` dropped, plain space before `%`); bytes via `formatBytes` (IEC); rates `${formatBytes(v)}/s`.
- Stream data never goes into TanStack Query or Zustand (overview §4); the chart cursor is UI state and may use Zustand.

## Review Focus

1. A `docker compose up` that starts 20 containers within a second → exactly one stream per container (each `start` replaces, never duplicates) and still one message per second. Test in Task 3: `a_burst_of_starts_opens_one_stream_per_container`.
2. The internal event broadcast lags (a burst larger than its capacity) → the supervisor re-lists running containers instead of missing a start. Test in Task 3: `a_lagged_event_receiver_relists`.
3. Samples arriving out of order or duplicated around a stream replacement (the old stream's last sample flushed after the new stream's first) → the store keeps per-container times strictly increasing. Test in Task 4: `drops_samples_not_newer_than_the_last`.
4. A narrow window (Stats tab at 360 px chart width) → time labels never overlap; the tick step widens from 2 to 5 or 10 minutes. Test in Task 6: `time_ticks_widen_when_narrow`.
5. Pause and unpause → no new stream (only `start` opens one), no break in the chart, CPU reads `0`. Tests in Task 1 (`zero_cpu_delta_is_zero`) and Task 3 (`pause_and_unpause_keep_the_stream`).

---

### Task 1: Stats derivation (S1)

Pure conversion of Bollard samples into `StatsSample`, FR-2.

**Files:**

- Create: `crates/docktide-core/src/stats.rs`
- Modify: `crates/docktide-core/src/lib.rs` (module, re-export, bindings list)
- Modify: `crates/docktide-core/src/logs.rs` (make `normalize_timestamp` and `unix_seconds` `pub(crate)`)
- Generated: `src/bindings/StatsSample.ts`

**Interfaces:**

- Produces:
  - `pub struct StatsSample { id: String, time: String, stream_start: bool, cpu_percent: Option<f64>, memory_usage: Option<u64>, memory_limit: Option<u64>, network_rx: Option<f64>, network_tx: Option<f64>, block_read: Option<f64>, block_write: Option<f64>, pids: Option<u64> }` — `Serialize`, `Clone`, `Debug`, `PartialEq`; TS shape exactly as spec §4 (`u64` fields must export as `number`: annotate with `#[cfg_attr(test, ts(type = "number | null"))]`).
  - `pub(crate) struct Deriver` (one per stream; `Default`): `fn next(&mut self, id: &str, raw: &ContainerStatsResponse) -> Option<StatsSample>`. Returns `None` for a sample whose `read` is the zero time (`0001-01-01T00:00:00Z`) or missing: the caller closes the stream. The first `Some` has `stream_start: true`.

- [ ] **Step 0: Branch and commit the spec and plan**

```bash
git switch -c m2-monitoring
git add docs/specifications/m2-monitoring.md docs/superpowers/plans/2026-10-24-m2-monitoring.md
git commit -m "Add M2 specification and plan: monitoring"
```

- [ ] **Step 1: Write the failing unit tests in `stats.rs`**

Build inputs with `serde_json::from_value::<ContainerStatsResponse>(json!({...}))` through a helper `fn raw(value: serde_json::Value) -> ContainerStatsResponse`. Tests and their assertions:

```rust
// CPU (cpu_stats vs precpu_stats in the same sample)
cpu_percent_matches_the_cli          // total 2e9 vs 1e9, system 20e9 vs 10e9, online_cpus 4 → Some(40.0)
zero_cpu_delta_is_zero               // total equal, system delta 1e9 → Some(0.0)
negative_cpu_delta_is_null           // total 1e9 vs precpu 2e9 → None
first_sample_has_no_cpu              // precpu_stats without system_cpu_usage → None
zero_system_delta_is_null            // system equal → None
online_cpus_falls_back_to_percpu     // no online_cpus, percpu_usage len 2, deltas 1e9/10e9 → Some(20.0)
// Memory (CLI calculateMemUsageUnixNoCache)
memory_v2_subtracts_inactive_file    // usage 100 MiB, inactive_file 30 MiB → Some(70 MiB)
memory_v1_subtracts_total_inactive   // usage 100, total_inactive_file 10, inactive_file 30 → Some(90)
cache_not_below_usage_keeps_usage    // usage 100, inactive_file 100 → Some(100); inactive_file 150 → Some(100)
absent_memory_is_null                // no memory_stats → usage None, limit None
zero_limit_is_null                   // limit 0 → None; limit 1 GiB → Some(1 GiB)
// Rates (Deriver keeps the previous counters)
rates_need_a_previous_sample         // first sample: network_rx/tx, block_read/write None
rates_divide_by_read_time            // reads 2 s apart, rx 1000 → 5000 → Some(2000.0); block "read" 0 → 4096 → Some(2048.0)
counter_reset_is_null                // rx 5000 → 1000 → None
no_networks_is_null                  // networks absent on both samples → None
block_ops_are_case_insensitive       // cgroup v1 "Read"/"Write" entries are summed like v2
// Lifecycle
zero_read_time_ends_the_stream       // read "0001-01-01T00:00:00Z" → next() returns None
stream_start_only_on_the_first       // [true, false, false] over three samples
pids_are_reported                    // pids_stats.current 7 → Some(7)
serializes_to_the_ipc_shape          // serde_json::to_value has keys id, time, streamStart, cpuPercent, memoryUsage, memoryLimit, networkRx, networkTx, blockRead, blockWrite, pids
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p docktide-core stats::`
Expected: compile failure (`StatsSample`, `Deriver` not defined).

- [ ] **Step 3: Implement `StatsSample` and `Deriver` in `stats.rs`**

Formulas exactly as spec FR-2. `onlineCpus` falls back to `percpu_usage.len()`, then 1. The elapsed time is the difference of `read` converted to Unix seconds as `f64`: `unix_seconds(t)` plus the 9-digit fraction of `normalize_timestamp(t)`; a non-positive elapsed time gives `None` rates. Keep `time` as Docker sent it. Module doc: `//! Stats sampling and derived values (spec M2 FR-1, FR-2).`

- [ ] **Step 4: Export and regenerate bindings**

`pub mod stats;` and `pub use stats::StatsSample;` in `lib.rs`; add `StatsSample` to the bindings test list; run `DOCKTIDE_UPDATE_BINDINGS=1 cargo test -p docktide-core bindings`. Check `src/bindings/StatsSample.ts` matches spec §4 (all numbers `number`, nullable ones `number | null`).

- [ ] **Step 5: Run the tests**

Run: `cargo test -p docktide-core && cargo clippy --workspace --all-targets -- -D warnings`
Expected: PASS, no warnings.

- [ ] **Step 6: Commit**

```bash
git add crates/docktide-core src/bindings
git commit -m "Add stats derivation (M2 slice S1)"
```

---

### Task 2: Container event fan-out (S1)

The events supervisor publishes each relevant container event, before coalescing, to in-process subscribers.

**Files:**

- Modify: `crates/docktide-core/src/connection.rs` (field in `Shared`, accessor)
- Modify: `crates/docktide-core/src/events.rs` (`forward` publishes; `Backoff` becomes `pub(crate)`)

**Interfaces:**

- Produces:
  - `#[derive(Debug, Clone, PartialEq, Eq)] pub(crate) struct ContainerEvent { pub id: String, pub action: String }` in `events.rs`.
  - `pub(crate) const CONTAINER_EVENT_CAPACITY: usize = 256;` and `Shared::container_events: tokio::sync::broadcast::Sender<ContainerEvent>` created with it.
  - `DockerService::container_events(&self) -> broadcast::Receiver<ContainerEvent>`.
  - `pub(crate) struct Backoff` with `pub(crate) fn next(&mut self) -> Duration` and `pub(crate) fn reset(&mut self)` (unchanged behaviour).

- [ ] **Step 1: Write the failing test** in `events.rs` tests: `forward_publishes_each_relevant_event_before_coalescing` — feed `forward` a stream of `start` (id a), `exec_start` (id a), `die` (id b); assert the published events are exactly `[("a","start"), ("b","die")]` in order, and the sink still receives one `ContainersChanged { ids: ["a","b"] }`.

- [ ] **Step 2: Run it to see it fail**

Run: `cargo test -p docktide-core events::`
Expected: compile failure (`ContainerEvent` not defined).

- [ ] **Step 3: Implement.** `forward` takes an extra `publish: &(dyn Fn(ContainerEvent) + Send + Sync)`; `relevant_id` becomes `relevant_event(event) -> Option<ContainerEvent>` (same allowlist). `supervise` passes `|e| { let _ = service.shared.container_events.send(e); }` (no receivers is not an error).

- [ ] **Step 4: Run all core tests**

Run: `cargo test -p docktide-core && cargo test -p docktide-core --test failures`
Expected: PASS (existing events tests unchanged in behaviour).

- [ ] **Step 5: Commit**

```bash
git add crates/docktide-core
git commit -m "Fan out container events to in-process subscribers (M2 slice S1)"
```

---

### Task 3: Stats supervisor (S1)

FR-1 lifecycle and FR-3 batching, tested against the fake Engine.

**Files:**

- Modify: `crates/docktide-core/src/stats.rs` (supervisor)
- Modify: `crates/docktide-core/src/streams.rs` (stats slot)
- Modify: `crates/docktide-core/tests/failures.rs` (fake Engine: container list and stats streams; tests)
- Modify: `crates/docktide-core/tests/docker.rs` (integration tests)

**Interfaces:**

- Consumes: `Deriver`, `StatsSample` (Task 1); `ContainerEvent`, `container_events()`, `Backoff` (Task 2); `stream_failure` (`events.rs`), `list_containers`, `ContainerState`, `generation()`, `streams()`.
- Produces:
  - `pub fn subscribe_stats(&self, sink: impl Fn(Vec<StatsSample>) -> bool + Send + Sync + 'static) -> SubscriptionId` on `DockerService`. Replaces a previous stats subscriber; runs until cancelled via `cancel_stream`, `cancel_all_streams`, or `sink` returning false. Must be called within a Tokio runtime.
  - `StreamRegistry::replace_stats(&self) -> (SubscriptionId, CancellationToken)`, sharing a private helper with `replace_live_updates`.
  - `pub(crate) const BATCH_TICK: Duration = Duration::from_secs(1);`

Behaviour (spec FR-1 table):

- The supervisor owns `HashMap<String, Running>` where `Running` holds the stream's child `CancellationToken` and a serial number. Each stream task sends its samples through an `mpsc::UnboundedSender<StatsSample>` and reports its exit as `(id, serial)`, so the map forgets only that exact stream.
- On start and on every `generation.changed()`: cancel all streams, `list_containers()`, open one stream for each `Running` or `Paused` container. A failed list waits for the next generation change.
- `start` → cancel the old stream (if any) and open a new one. `die` / `destroy` → cancel. Other actions → ignore. `RecvError::Lagged` → behave as a generation change (re-list).
- Stream task: `docker.stats(id, Some(StatsOptions { stream: true, one_shot: false }))`, each item through its `Deriver`; `None` (zero read) → exit. `notFound` → exit. A connection-level error (`stream_failure(...).is_connection_level()`) → exit (the client is dropped, the generation changes, the supervisor re-lists). Any other error or end of stream → sleep `Backoff::next()` and reopen with a fresh `Deriver`; the backoff resets after a sample.
- Batching: a `tokio::time::interval(BATCH_TICK)` with `MissedTickBehavior::Delay`; each tick drains the mpsc receiver into one `Vec`; empty → send nothing; `sink` false → return (dropping the parent token cancels every stream).

- [ ] **Step 1: Extend `FakeEngine`**

- `set_containers(&self, json: &str)`: body served for `/containers/json` (default `[]`).
- `missing(&self, id: &str)`: `/containers/{id}/stats` answers `404 {"message":"No such container: {id}"}`.
- `/containers/{id}/stats` otherwise opens a chunked `application/json` response kept in `stats.open: Mutex<Vec<(String, UnixStream)>>`, counting closes like `LogStreams`.
- `stats_sample(&self, id: &str, read: &str, cpu: u64, system: u64)`: writes one sample chunk (with `precpu_stats` of the previous call for that id, or none) to that id's open streams.
- `cut_stats(&self, id: &str)`: shuts down that id's stats connections only.
- `wait_for_stats_streams(&self, id: &str, count: usize)` and `wait_for_closed_stats(&self, count: usize)`, polling like `wait_for_log_streams`.
- A container list entry helper `fn container_json(id: &str, state: &str) -> String` (`Id`, `Names`, `Image`, `State`, `Status`, `Ports: []`).

- [ ] **Step 2: Write the failing fake Engine tests** (subscribe with a sink that forwards batches into an `mpsc`, as `log_events()` does):

```rust
running_containers_get_a_stream_on_subscribe   // list [a running, b paused, c exited] → streams for a and b only
samples_arrive_batched_once_per_second         // a and b each send 3 samples over ~2 s → every batch ≥ 1 s apart, all 6 samples delivered, first of each id streamStart
a_zero_read_sample_closes_the_stream           // a sends a sample with read "0001-01-01T00:00:00Z" → its connection closes; nothing emitted for it
a_start_event_replaces_an_open_stream          // a's stream open, emit("start","a") before any zero sample → old connection closed, a new one open; next sample has streamStart
a_die_event_closes_the_stream                  // emit("die","a") → connection closes without a zero sample
a_cut_stats_stream_is_reopened                 // cut_stats("a") while /events stays open → reopened after ~1 s; next sample streamStart
a_missing_container_closes_only_its_stream     // missing("x"); emit("start","x") → one 404 request, no retry; a's stream unaffected
a_new_generation_relists                       // close_event_streams(false) → after reconnect, /containers/json requested again and streams reopened
cancelling_closes_every_stats_connection       // cancel_stream(subscription) → all stats connections closed
a_closed_sink_closes_every_stats_connection    // sink returns false on first batch → all closed
a_burst_of_starts_opens_one_stream_per_container  // 20 start events for 10 ids within 100 ms → exactly 10 open stats connections
pause_and_unpause_keep_the_stream              // emit("pause","a"), emit("unpause","a") → no new stats request for a
a_lagged_event_receiver_relists                // emit 300 start events for one id faster than the supervisor reads → /containers/json requested again
```

Spec §7.1 asks for batching under paused Tokio time; `samples_arrive_batched_once_per_second` covers it against the fake Engine instead (real time, ~2 s), which also exercises the real streams. Record this deviation in spec §11 (Task 9).

For the lag test, the supervisor must be blocked long enough for the receiver to lag; if that is not reliably reachable, cover `Lagged → relist` with a unit test on the event-handling function instead and note it in the commit message.

- [ ] **Step 3: Run them to see them fail**

Run: `cargo test -p docktide-core --test failures stats`
Expected: compile failure (`subscribe_stats` not defined).

- [ ] **Step 4: Implement the supervisor** in `stats.rs` per the behaviour above. Structure the event handling as a function `fn on_event(&mut self, event: Result<ContainerEvent, RecvError>) -> Action` (enum `Open(id)`, `Close(id)`, `Relist`, `Ignore`) so it is unit-testable; add unit tests `start_opens`, `die_and_destroy_close`, `other_actions_are_ignored`, `lag_relists`.

- [ ] **Step 5: Run the tests**

Run: `cargo test -p docktide-core && cargo test -p docktide-core --test failures`
Expected: PASS.

- [ ] **Step 6: Write the Docker integration tests** in `tests/docker.rs` (reuse its test-container guard and `busybox:latest`):

```rust
a_busy_loop_reports_cpu        // `sh -c 'while :; do :; done'` → a sample with cpuPercent > 50.0 within 3 s
stop_closes_and_start_reopens  // stop → no samples for the id after 2 s; start → samples again
restart_starts_a_new_stream    // docker restart → a later sample with streamStart == true
```

Run: `cargo test -p docktide-core --test docker -- --ignored stats`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/docktide-core
git commit -m "Add stats supervisor (M2 slice S1)"
```

---

### Task 4: Delivery and stats store (S2)

**Files:**

- Modify: `src-tauri/src/lib.rs`, `src-tauri/build.rs`, `src-tauri/capabilities/default.json`
- Modify: `src/lib/ipc.ts`
- Create: `src/features/stats/stats-store.ts`, `src/features/stats/stats-store.test.ts`
- Create: `src/features/stats/format.ts`, `src/features/stats/format.test.ts`
- Create: `src/features/stats/use-stats.ts`
- Modify: `src/app/providers.tsx`

**Interfaces:**

- Consumes: `subscribe_stats` (Task 3), `StatsSample` binding.
- Produces:
  - Command `subscribe_stats(on_stats: Channel<Vec<StatsSample>>) -> Result<(), IpcError>`, shaped like `subscribe_live_updates`.
  - `subscribeStats(onStats: (samples: StatsSample[]) => void): Promise<null>` in `ipc.ts`.
  - `stats-store.ts`:
    - `export const HISTORY_MS = 600_000, BREAK_MS = 2_500, CURRENT_MS = 3_000;`
    - `export type StatsPoint = StatsSample & { t: number; breakBefore: boolean }` (`t` = `Date.parse(time)`; `breakBefore` = `streamStart` or more than `BREAK_MS` after the previous point).
    - `export class StatsStore { now: number; version: number; push(samples: StatsSample[], now?: number): void; tick(now?: number): void; series(id: string): readonly StatsPoint[]; current(id: string): StatsPoint | null; subscribe(listener: () => void): () => void; }` with `constructor(timers = { setInterval, clearInterval })` so tests inject fake timers. `push` and `tick` trim points older than `now − HISTORY_MS`, delete empty series, bump `version`, notify. A 1 s interval runs while any series exists and is cleared when the store becomes empty. `current` returns the last point if `now − t ≤ CURRENT_MS`, else `null`.
    - `export const statsStore = new StatsStore();`
  - `use-stats.ts`: `useStatsSubscription(): void` (like `useLiveUpdates`: subscribes once, pushes into `statsStore`, toasts a rejected subscribe) and `useStatsSeries(id: string): { points: readonly StatsPoint[]; current: StatsPoint | null; now: number }` via `useSyncExternalStore(statsStore.subscribe, () => statsStore.version)`.
  - `format.ts`: `formatPercent(v: number | null): string`, `formatMemory(v: number | null): string`, `formatRate(v: number | null): string`, `memoryPercent(usage: number | null, limit: number | null): number | null` (null unless both present and limit > 0). Reuses `formatBytes` from `@/features/container-details/format`.

- [ ] **Step 1: Write the failing tests**

`stats-store.test.ts` (`vi.useFakeTimers()` from `vite-plus/test`):

```ts
it("keeps samples per container in time order");
it("drops samples not newer than the last"); // same or older `time` for an id is ignored
it("marks a break before streamStart even 1 s after the previous sample");
it("marks a break when samples are more than 2.5 s apart"); // 2_400 ms apart → no break; 2_600 → break
it("trims samples older than 10 minutes on push");
it("empties the store after 10 minutes without a batch and stops its timer"); // advance 601_000 ms → series("a") empty, vi.getTimerCount() === 0
it("advances now every second while it holds samples"); // version increases on each 1 s tick
it("expires the current sample after 3 s"); // current("a") non-null at +3_000, null at +3_001
it("notifies subscribers once per push or tick");
```

`format.test.ts`:

```ts
expect(formatPercent(12.345)).toBe("12.3 %");
expect(formatPercent(0)).toBe("0 %");
expect(formatPercent(null)).toBe("—");
expect(formatMemory(50_544_640)).toBe("48.2 MiB");
expect(formatMemory(null)).toBe("—");
expect(formatRate(2048)).toBe("2 KiB/s");
expect(formatRate(null)).toBe("—");
expect(memoryPercent(512, 1024)).toBe(50);
expect(memoryPercent(512, null)).toBeNull();
expect(memoryPercent(null, 1024)).toBeNull();
expect(memoryPercent(512, 0)).toBeNull();
```

(`formatPercent` uses one decimal and drops a trailing `.0`, like `formatBytes`.)

- [ ] **Step 2: Run them to see them fail**

Run: `pnpm test src/features/stats`
Expected: FAIL (modules not found).

- [ ] **Step 3: Implement** the command and registrations, `subscribeStats`, `StatsStore`, `format.ts`, `use-stats.ts`; mount `useStatsSubscription()` in `Providers` next to `useLiveUpdates()` (a `Stats` null component like `LiveUpdates`).

- [ ] **Step 4: Run all checks**

Run: `pnpm check && pnpm test && pnpm typecheck && cargo clippy --workspace --all-targets -- -D warnings`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src src-tauri
git commit -m "Deliver stats to a frontend store (M2 slice S2)"
```

---

### Task 5: List columns and layout (S2)

Spec §5.1, AC-1, AC-3, AC-9.

**Files:**

- Modify: `src/features/containers/container-table.tsx`
- Modify: `src/features/containers/containers-page.tsx` (only if the layout fix needs it)

**Interfaces:**

- Consumes: `useStatsSeries`, `formatPercent`, `formatMemory`, `memoryPercent` (Task 4); `Tooltip` from `@/components/ui/tooltip`.

- [ ] **Step 1: Add the columns.** `CPU` and `Memory` after `Status`, right-aligned, `tabular-nums`. A small `StatsCells({ id })` component per row reads `useStatsSeries(id).current` so only rows re-render. Memory tooltip text: `` `${formatMemory(usage)} of ${formatBytes(limit)} (${formatPercent(memoryPercent(usage, limit))})` `` when `memoryPercent` is non-null, otherwise no tooltip. Skeleton rows get two more cells.

- [ ] **Step 2: Fix the clipping.** At a 1,024 px window the Actions column must be fully visible; the Image cell truncates first (`max-w` reduced, `truncate`, full value in `title`), Status may wrap. Do not hide columns.

- [ ] **Step 3: Verify in the app**

Run: `pnpm tauri dev`, with one labelled busybox busy loop and one `sleep` container running. Expected: CPU near 100 % for the loop, memory values within 5 % of `docker stats --no-stream`; `—` for stopped containers; values appear within 2 s of starting a container with the CLI and turn to `—` within 4 s of stopping it; at 1,024 px width nothing is clipped. Remove the test containers afterwards.

- [ ] **Step 4: Run checks and commit**

```bash
pnpm check && pnpm test && pnpm typecheck
git add src
git commit -m "Show CPU and memory in the container list (M2 slice S2)"
```

**Stop for review here (spec §8: after S2).**

---

### Task 6: Chart scales (S3)

Pure helpers for `TimeSeriesChart`, spec §5.2.

**Files:**

- Create: `src/features/stats/chart-scale.ts`, `src/features/stats/chart-scale.test.ts`

**Interfaces:**

- Produces:
  - `niceScale(dataMax: number, minMax: number, kind: "decimal" | "bytes"): { max: number; step: number }` — step from {1, 2, 2.5, 5} × 10ⁿ (decimal) or × 1024ⁿ (bytes), 3 to 5 ticks from 0 to `max ≥ max(dataMax, minMax)`; `dataMax` 0 and `minMax` 0 give `{ max: 1, step: 0.25 }` for decimal and `{ max: 1024, step: 256 }` for bytes.
  - `growMax(previous: number, next: number): number` — the y maximum only grows while the visible data needs it: returns `next` if larger than `previous`, else `previous` unless the data fell below a quarter of it (then `next`).
  - `timeTicks(start: number, end: number, width: number): number[]` — local-time minute boundaries at a step from [2, 5, 10] min, the smallest whose labels are at least 64 px apart at `width`.
  - `nearestIndex(times: readonly number[], t: number): number` — binary search; `-1` for an empty array.
  - `chartColumns(points: readonly StatsPoint[], pick: ((p: StatsPoint) => number | null)[]): { times: number[]; values: (number | null)[][] }` — one column per picker; before each point with `breakBefore` (except the first) insert a time halfway to the previous point with `null` in every column.

- [ ] **Step 1: Write the failing tests**

```ts
expect(niceScale(87, 100, "decimal")).toEqual({ max: 100, step: 25 });
expect(niceScale(230, 100, "decimal")).toEqual({ max: 250, step: 50 });
expect(niceScale(3 * 1024 ** 2, 0, "bytes")).toEqual({ max: 4 * 1024 ** 2, step: 1024 ** 2 });
expect(niceScale(0, 0, "decimal")).toEqual({ max: 1, step: 0.25 });
it("time ticks every 2 minutes on a wide chart"); // width 900, 10-minute window → 5 ticks on even minutes
it("time ticks widen when narrow"); // width 360 → step 5 min (2 ticks); width 150 → 10 min
expect(nearestIndex([0, 1000, 2000], 1400)).toBe(1);
expect(nearestIndex([], 5)).toBe(-1);
it("chart columns put a null point at each break"); // points t=0,1000,(break)5000 → times [0,1000,3000,5000], values [[a,b,null,c]]
expect(growMax(100, 80)).toBe(100);
expect(growMax(100, 120)).toBe(120);
expect(growMax(400, 80)).toBe(80);
```

- [ ] **Step 2: Run them to see them fail**

Run: `pnpm test src/features/stats/chart-scale`
Expected: FAIL (module not found).

- [ ] **Step 3: Implement** the five functions.

- [ ] **Step 4: Run the tests**

Run: `pnpm test src/features/stats`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/features/stats
git commit -m "Add chart scales (M2 slice S3)"
```

---

### Task 7: `TimeSeriesChart` (S3)

Spec §5.2 "The chart". Canvas drawing has no unit tests (no canvas in the test environment); everything computable lives in Task 6.

**Files:**

- Create: `src/features/stats/time-series-chart.tsx`
- Create: `src/features/stats/chart-cursor.ts`
- Modify: `src/index.css` (chart colour tokens)

**Interfaces:**

- Consumes: Task 6 helpers.
- Produces:
  - `chart-cursor.ts`: `useChartCursor = create<{ time: number | null; setTime: (t: number | null) => void }>()` (Zustand, UI state).
  - `type ChartSeries = { label: string; color: string /* CSS custom property name, e.g. "--chart-1" */; values: readonly (number | null)[]; dashed?: boolean }`.
  - `function TimeSeriesChart(props: { title: string; times: readonly number[]; series: ChartSeries[]; start: number; end: number; kind: "decimal" | "bytes"; minMax: number; format: (v: number | null) => string; height?: number /* default 160 */ }): JSX.Element`.

- [ ] **Step 1: Tokens.** Give `--chart-1`, `--chart-2`, `--chart-3` distinct hues in `:root` and `.dark` (they are unused today; currently greys). Use hex values: canvas `fillStyle` must accept them, and an `oklch()` string is not guaranteed in the WebKitGTK canvas.

- [ ] **Step 2: Implement the component.**
  - Layout: title and legend (DOM text: per series a colour swatch, label, and `format(value at cursor or latest)`), then a `<canvas role="img" aria-label={title}>` filling the width.
  - Size with a `ResizeObserver`; backing store `width × devicePixelRatio`.
  - One full redraw in a `useLayoutEffect` whenever props, size, or cursor time change: grid lines and y labels from `niceScale` (with `growMax` kept in a ref), time labels from `timeTicks` formatted `HH:mm` (local, `Intl.DateTimeFormat` with `hour: "2-digit", minute: "2-digit", hourCycle: "h23"`), lines with a path break at `null`, a 12 % alpha area fill under the first series, `setLineDash([4, 4])` for dashed series. Colours from `getComputedStyle(canvas).getPropertyValue(token)`; grid and text from `--border` and `--muted-foreground` resolved the same way (if those resolve to `oklch()` and the canvas ignores them, add hex `--chart-grid` and `--chart-text` tokens instead).
  - Pointer: `pointermove` maps x to a time and calls `setTime`; `pointerleave` calls `setTime(null)`. When the cursor time is set and inside the window, draw a vertical line at `times[nearestIndex(times, time)]` and a dot per non-null series value there.

- [ ] **Step 3: Verify visually.** Temporarily render one chart with synthetic data on the Stats route stub, or proceed to Task 8 and verify there. Check: crisp lines at 1× and 2× scale, breaks drawn as gaps, labels not overlapping at 360 px, cursor line synced across two charts.

- [ ] **Step 4: Run checks and commit**

```bash
pnpm check && pnpm test && pnpm typecheck
git add src
git commit -m "Add TimeSeriesChart (M2 slice S3)"
```

---

### Task 8: Stats tab (S3)

Spec §5.2, AC-4, AC-6.

**Files:**

- Create: `src/features/stats/stats-tab.tsx`
- Modify: `src/stores/details-tab.ts` (`DetailsTab` adds `"stats"`)
- Modify: `src/app/router.tsx` (route, `TAB_PATHS`), `src/app/router.test.ts`
- Modify: `src/features/container-details/container-page.tsx` (tab link order Overview, Stats, Logs, Inspect)

**Interfaces:**

- Consumes: `useStatsSeries`, format helpers (Task 4); `chartColumns` (Task 6); `TimeSeriesChart` (Task 7); `useContainer` from `@/features/container-details/use-container` for `state` and `memoryLimit`.

- [ ] **Step 1: Write the failing router test** in `router.test.ts`: `it("redirects /containers/$id to the stats tab when it was used last")` — set `useDetailsTab.getState().setTab("stats")`, navigate to `/containers/abc`, expect pathname `/containers/abc/stats`.

- [ ] **Step 2: Run it to see it fail**

Run: `pnpm test src/app/router`
Expected: FAIL (type error or no route).

- [ ] **Step 3: Add the route** `statsRoute` (path `stats`, component `StatsTab`, `onEnter` sets `"stats"`) and the tab link.

- [ ] **Step 4: Implement `StatsTab`.**
  - Current values row: CPU, memory (`formatMemory(usage)`, limit as `formatBytes(details.memoryLimit)` or "Unlimited", `formatPercent(memoryPercent(...))`), network rx/tx, block read/write (`formatRate`), PIDs; each `—` without a current sample.
  - Four `TimeSeriesChart`s with `start = now − HISTORY_MS`, `end = now` from `useStatsSeries`: CPU (`kind "decimal"`, `minMax 100`, `--chart-1`); Memory (usage `--chart-1`, plus a dashed limit series `--chart-3` only when `details.memoryLimit` is non-null, `kind "bytes"`); Network (rx `--chart-1`, tx `--chart-2`, `kind "bytes"`, `formatRate`); Block I/O (read `--chart-1`, write `--chart-2`).
  - States from spec §5.2, with the page's existing disconnected, `notFound`, and error handling from `container-page.tsx` left as is: running and no points → "Waiting for the first sample…"; running, points, no current → note "Waiting for stats…"; not running with points → note "Container is not running."; not running and no points → "Container is not running. Stats appear when it starts."

- [ ] **Step 5: Verify in the app** (labelled test containers only):
  - Busy loop: CPU chart near 100 %; history present on first opening after the container ran for a minute (AC-4).
  - `docker stop` then `docker start`, and separately `docker restart -t0`: a visible break each time.
  - Hovering one chart moves the cursor in all four; leaving clears it; the legend shows the hovered values.
  - Cut Docker access as in M1 AC-3 (socket proxy): charts show a break and resume after "Connected" (AC-6).
  - Resize the window: charts redraw; at narrow width, time labels do not overlap.

- [ ] **Step 6: Run checks and commit**

```bash
pnpm check && pnpm test && pnpm typecheck
git add src
git commit -m "Add Stats tab (M2 slice S3)"
```

---

### Task 9: Docs and verification (S4)

**Files:**

- Modify: `README.md` (status line, Features: list columns and Stats tab)
- Modify: `docs/architecture/overview.md` (§3.1 `stats` row → M2 with its responsibility as built; §3.3 `features/stats/`; §4 metrics row → derived in Rust, history in the frontend store; §5 streams list adds `subscribe_stats`; §7 pacing adds "stats: one batch per second for all containers"; §10 M2 done)
- Create: `docs/architecture/decisions/0003-m2-monitoring.md` (D1 to D5 from spec §10, ADR 0002 format; supersedes the overview §4 plan for Rust ring buffers; notes ADR 0001 D2: monitoring stays in `docktide-core`, no `docktide-monitor` crate yet)
- Modify: `docs/specifications/m2-monitoring.md` §11 (results table like M1 §11, deviations, observations)

- [ ] **Step 1: Run the full gate**

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace && cargo test --workspace -- --ignored && pnpm check && pnpm test && pnpm typecheck`
Expected: all pass; record the test counts.

- [ ] **Step 2: Run the AC checklist** (spec §6) on the dev host with labelled containers only: AC-5 with 20 busybox containers (count Channel messages per second via a temporary `console.count` or the store's version), AC-7 with `ss -x` before and after a reload, AC-8 with the socket proxy request log over 60 s and no running containers. Mark anything not exercised as **unverified** with the reason.

- [ ] **Step 3: Write the docs** listed above.

- [ ] **Step 4: Commit**

```bash
git add README.md docs
git commit -m "Document M2 and record verification (M2 slice S4)"
```
