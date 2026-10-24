# M2: Monitoring

- Status: **Approved** 2026-10-24. Decisions in §10.
- Date: 2026-10-24
- Related: [Architecture overview](../architecture/overview.md), [ADR 0002](../architecture/decisions/0002-m1-navigation-logs-and-live-updates.md), [M1 spec](m1-live-details-logs.md)

## 1. Goal

Docktide shows what each running container is using right now and over the
last few minutes: CPU, memory, network, block I/O, and processes. The numbers
match `docker stats`. The list answers "which container is busy?" and the new
Stats tab answers "what has it been doing?".

## 2. Scope

In scope:

1. A stats supervisor in `docktide-core` that follows Docker's stats stream for
   every running container (§10, D2) and turns raw samples into rates and
   percentages.
2. One app-wide `Channel` that delivers all containers' samples, at most one
   message per second.
3. A bounded history per container in the frontend: 10 minutes at Docker's 1 s
   interval (§10, D3, D4).
4. CPU and memory columns on the container list.
5. A Stats tab on the details page with four live charts, drawn by a small
   chart component of our own (§10, D1).

Non-goals (explicitly deferred):

- Host-wide totals or a dashboard page. Per-process (`top`) views.
- Sorting the list by CPU or memory (the list has no sorting yet; it comes with
  general column sorting).
- Sparklines in the list. Configurable history length or sampling interval.
- History that survives a restart of Docktide, or covers time Docktide was closed.
- Alerts, thresholds, notifications, tray indicators.
- Pausing sampling while the window is hidden or minimized.

## 3. Functional requirements

### FR-1 Stats sources

Request: `GET /containers/{id}/stats?stream=1`, one per running or paused
container. Docker sends one sample per second and never ends the stream by itself.
Observed on Engine 29.9.2 (2026-10-24, cgroup v2):

- The first sample has no `precpu_stats.system_cpu_usage`.
- After the container stops, the stream keeps sending samples with
  `read: "0001-01-01T00:00:00Z"` and all counters zero. A stream opened on a
  stopped container sends only such samples.
- A `docker restart` that completes between two samples sends no zero sample:
  the same stream simply continues with samples from the new run, 1 s apart.
- A paused container keeps sending samples; CPU counters do not move.
- With no memory limit, `memory_stats.limit` is the host's memory.
- cgroup v2 block I/O is `blkio_stats.io_service_bytes_recursive` with ops
  `read` and `write`; cgroup v1 uses `Read` and `Write`.

The supervisor learns about container lifecycle from the events supervisor,
which fans out each relevant container event (id and action) inside
`DockerService` before coalescing. There is no second events stream, so stats
need live updates to be subscribed, which the app always is.

Stream lifecycle, per container:

| Situation                                                      | Behaviour                                                                                                            |
| -------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| Subscription starts, or the client generation changes          | Close all streams; list containers; open a stream for each one that is running or paused                             |
| A `start` event                                                | Close the container's stream if it has one, and open a new one. Each run of a container therefore has its own stream |
| A `die` or `destroy` event                                     | Close the container's stream                                                                                         |
| A sample has the zero `read` time                              | Close the stream (a safety net for a missed `die`); emit nothing for it                                              |
| The stream answers `notFound`                                  | Close the stream                                                                                                     |
| The stream fails or ends otherwise, while connected            | Reopen it after a capped backoff of 1, 2, 4, 8, 16, then 30 s, reset by the first sample. Log each failure at debug  |
| Disconnected (connection-level failure)                        | Close all streams and report through `DockerService::failed`; the next generation change reopens them (first row)    |
| Subscription cancelled or replaced, or its sink reports closed | Close every stream                                                                                                   |

A stream opened on a stopped container closes on its first zero sample, so
reopening never loops on a stopped container. Every stream is a child of the
subscription's `CancellationToken`, so cancelling the subscription closes every
Engine connection within one read.

### FR-2 Derived values

Computed in Rust per sample, with the same formulas as the Docker CLI
(`cli/command/container/stats_helpers.go`, checked 2026-10-24), so values match
`docker stats`:

- **CPU %**: `cpuDelta / systemDelta × onlineCpus × 100`, where the deltas are
  `cpu_stats` minus `precpu_stats` from the same sample. `onlineCpus` falls
  back to the length of `percpu_usage`, then 1 (§10, D5: 100 % is one core).
  - `0` when `cpuDelta` is 0 and `systemDelta` is positive (an idle or paused
    container).
  - `null` when `precpu_stats.system_cpu_usage` is missing (a stream's first
    sample), when `systemDelta` is not positive, or when `cpuDelta` is negative.
- **Memory usage**, as the CLI's `calculateMemUsageUnixNoCache`: if
  `stats.total_inactive_file` is present and less than `usage` (cgroup v1),
  `usage − total_inactive_file`; otherwise, if `stats.inactive_file` (0 when
  absent) is less than `usage` (cgroup v2), `usage − inactive_file`; otherwise
  `usage`. `null` when `memory_stats.usage` is absent (for example rootless
  Docker without cgroup delegation).
- **Memory limit**: `memory_stats.limit`; `null` when absent or 0.
- **Network rx/tx**: sum of all interfaces' `rx_bytes` / `tx_bytes`. **Block
  read/write**: sum of the `read` / `write` entries, case-insensitive.
- **Rates** (network, block): difference to the previous sample of the same
  stream divided by the difference of `read` times, in bytes per second.
  `null` for a stream's first sample, after a counter went backwards, or when
  the source is absent (a container with `network_mode: host` has no networks).
- **PIDs**: `pids_stats.current`, or `null`.
- **Stream start**: `true` on the first sample of every stream (FR-1 opens a new
  stream per run, after a reconnect, and after a retry).

### FR-3 Delivery

Command `subscribe_stats(onStats: Channel<StatsSample[]>) -> null`.

- Called once at startup next to `subscribe_live_updates`. A second call
  replaces the previous subscriber.
- Samples from all streams are collected and sent together once per second,
  on a fixed 1 s tick. An empty tick sends nothing.
- With no running containers, there are no stats requests to Docker, and M1's
  "no requests while idle" (M1 AC-4) still holds.

### FR-4 History in the frontend

- A stats store outside React, like the logs session (overview §4), receives
  every batch and keeps a time series per container id.
- While the store holds any samples, a 1 s timer in the frontend (it makes no
  Docker requests) drops samples older than 10 minutes, deletes series without
  samples, and advances the store's "now". Charts slide with that "now", not
  with the latest sample, so they keep moving after the last container stops.
  The timer stops when the store is empty. A removed container's history
  therefore disappears by itself within 10 minutes; there is no removal message.
- Sample times are Docker's clock and "now" is the frontend's; both are the
  same host's clock, since only local endpoints are supported.
- A series breaks, and charts draw no line across the break, before every
  sample with `streamStart`, and wherever consecutive samples are more than
  2.5 s apart.
- A container's **current** sample is its latest one if that is at most 3 s
  older than "now"; otherwise it has none, and current values show `—`. A
  stalled or failed stream never leaves a frozen value on screen.
- Components read the store with `useSyncExternalStore` and a version counter,
  and re-render at most once per batch or timer tick.
- A page reload starts with empty history (§9).

## 4. IPC types

New types, generated into `src/bindings/`:

```ts
type StatsSample = {
  id: string; // full container id
  time: string; // Docker's `read`, RFC 3339 nano
  streamStart: boolean; // first sample of a stream; charts break before it
  cpuPercent: number | null; // 100 = one core (FR-2)
  memoryUsage: number | null; // bytes, without inactive file cache
  memoryLimit: number | null; // bytes; the host's memory when unlimited
  networkRx: number | null; // bytes per second
  networkTx: number | null;
  blockRead: number | null; // bytes per second
  blockWrite: number | null;
  pids: number | null;
};
```

`subscribe_stats` is listed in `src-tauri/build.rs` and granted in
`capabilities/default.json`.

## 5. UI specification

### 5.1 Container list

- Two columns after Status: **CPU** (`12.3 %`, one decimal) and **Memory**
  (`48.2 MiB`, IEC units as `docker stats`, using `formatBytes`). Right-aligned,
  tabular numbers. A tooltip on Memory shows `48.2 MiB of 1 GiB (4.7 %)`.
- Values come from the container's current sample (FR-4). A container with no
  current sample, or a `null` value, shows `—`. The tooltip shows the limit and
  % only when both usage and limit are known.
- The table must not clip the Actions column at 1,024 px window width (the M1
  observation, made worse by two more columns). Image truncates first.

### 5.2 Stats tab

Route `/containers/$id/stats`, tab order Overview, Stats, Logs, Inspect. It
takes part in the "last used tab" rule of M1 §5.3.

- Current values on top: CPU, memory (usage, limit or "Unlimited" from the
  details query, and %), network rx/tx, block read/write, PIDs. Each is `—`
  without a current sample or when `null`; memory % needs both usage and limit.
- Four charts below, each spanning the last 10 minutes and sliding with time:

  | Chart     | Series                 | Y axis                                                    |
  | --------- | ---------------------- | --------------------------------------------------------- |
  | CPU       | CPU %                  | from 0 to at least 100 %, growing with the data           |
  | Memory    | usage; limit as a line | bytes; the limit line only when the container has a limit |
  | Network   | rx, tx                 | bytes per second                                          |
  | Block I/O | read, write            | bytes per second                                          |

- Hovering a chart shows a crosshair and the values at that time, in all four
  charts at once (synchronized cursor). Leaving the chart clears it.
- Colours come from theme tokens and follow light and dark themes.
- Charts update once per batch or store tick and redraw on resize.

The chart (§10, D1) is one component, `TimeSeriesChart`, drawn on a `<canvas>`
and made only for this page:

- Input: a time window `[now − 10 min, now]`, one shared array of sample times,
  and one to three series (values with `null` at breaks, a colour token, a
  label, optionally dashed). A y-axis formatter and a minimum y maximum (100
  for CPU, 0 otherwise).
- Drawing: one full redraw per update at `devicePixelRatio`, sized by a
  `ResizeObserver`. Lines break at `null`; the first series has a light area
  fill. Horizontal grid lines at 3 to 5 "nice" y ticks (1, 2, 2.5, 5 × 10ⁿ,
  or the same over 1024ⁿ for bytes) from 0 to a maximum that only grows while
  the data needs it. Time ticks every 2 minutes, labelled `HH:mm`.
- Cursor: pointer position becomes a time, shared by all charts on the page;
  each chart draws a vertical line at the nearest sample and dots on each
  series. The values at the cursor (or the latest ones) are shown as text in a
  legend above the canvas, not drawn, so they are selectable and readable by
  screen readers. The canvas has `role="img"` and an `aria-label` naming the
  chart.
- No animation, zoom, pan, or tooltips beyond the legend.

States:

| Condition                       | Rendering                                                          |
| ------------------------------- | ------------------------------------------------------------------ |
| Running, no sample yet          | Charts empty with "Waiting for the first sample…"                  |
| Running                         | Live values and charts                                             |
| Paused                          | As running; CPU reads 0 %                                          |
| Not running, history present    | History kept; current values `—`; note "Container is not running." |
| Running, no current sample      | History kept; current values `—`; note "Waiting for stats…"        |
| Not running, no history         | "Container is not running. Stats appear when it starts."           |
| Disconnected, `notFound`, error | The page's states from M1 §5.3                                     |

## 6. Acceptance criteria

| ID    | Criterion                                                                                                                                                                             | Verification                                                   |
| ----- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------- |
| AC-1  | Running containers show CPU and memory in the list within 2 s of opening Docktide                                                                                                     | manual                                                         |
| AC-2  | For a busybox busy loop and a container holding a known amount of memory, CPU and memory are within 5 % (relative) of `docker stats --no-stream`                                      | integration test on the formulas + manual comparison           |
| AC-3  | A container started with the CLI shows stats within 2 s; after a stop, its Engine stats connection closes within 2 s and the list shows `—`                                           | `ss -x` on the socket or fake Engine connection count + manual |
| AC-4  | The Stats tab shows the history since the container started or the last 10 minutes, whichever is shorter, as soon as it opens; a stop and start, and a `docker restart`, show a break | manual                                                         |
| AC-5  | With 20 running test containers, at most one stats message per second crosses the bridge and the UI stays responsive (scrolling, tabs, actions, logs)                                 | Channel message count + manual                                 |
| AC-6  | Disconnect and reconnect: stats resume after "Connected" without a refresh, with a gap in the charts                                                                                  | simulated as in M1 AC-3                                        |
| AC-7  | Reloading the page or closing the window closes every stats connection                                                                                                                | `ss -x` or fake Engine connection count                        |
| AC-8  | With no running containers and nothing happening, Docktide makes no Docker requests                                                                                                   | proxy request log over 60 s, as M1 AC-4                        |
| AC-9  | The list does not clip at 1,024 px window width                                                                                                                                       | manual                                                         |
| AC-10 | Every M1 acceptance criterion still holds                                                                                                                                             | re-run of the M1 checklist                                     |
| AC-11 | `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `vp check`, `vp test run`, and `pnpm typecheck` pass                          | command output                                                 |
| AC-12 | README and the architecture overview describe monitoring as built                                                                                                                     | review                                                         |

Any criterion not verified in the implementation session is reported as
**unverified** with the reason.

## 7. Test plan

### 7.1 Unit tests (`cargo test`, no Docker)

From constructed `ContainerStatsResponse` values:

- CPU: normal case, zero CPU delta with positive system delta (`0`), negative
  CPU delta (`null`), missing `precpu_stats` (first sample), zero system delta,
  `online_cpus` missing (falls back to `percpu_usage`).
- Memory: cgroup v2 `inactive_file`, v1 `total_inactive_file`, neither, cache
  equal to and larger than usage (falls back to `usage`), absent
  `memory_stats` (`null` usage and limit), limit 0 (`null`).
- `streamStart` on the first sample only.
- Rates: first sample, normal delta, counter reset, no networks, v1 and v2 op names.
- The zero `read` time closes the stream.
- Retry backoff schedule and its reset after a sample.
- Batching (paused Tokio time): samples from several streams leave as one
  message per tick; an empty tick sends nothing.
- Serialization of `StatsSample` to the shape in §4.

### 7.2 Docker integration tests (`cargo test -- --ignored`)

Same rules as M1 §7.2: only containers labelled `io.docktide.test=true`.

- A busybox busy loop reports CPU above 50 % within 3 s.
- A stopped test container's stream closes; starting it again opens a new one.
- `docker restart` of a test container yields a sample with `streamStart` from
  the new run.

### 7.3 Fake Engine tests (`tests/failures.rs`)

- Cancelling the subscription, or a closed sink, closes every stats connection.
- A new client generation re-lists and reopens streams.
- A stats request answered with 404 closes that stream only.
- One stats connection cut while the events connection stays up is reopened
  after the backoff, and its next sample has `streamStart`.
- A `start` event arriving while the old stream is still open (it has not yet
  sent its zero sample) replaces that stream; the container keeps a stream
  after the old one would have closed.
- A `die` event closes the stream without waiting for a zero sample.

### 7.4 Frontend (`vp test`)

- Stats store (fake timers): trimming by time, deleting empty series, version
  counter; ten minutes passing with no batch empties the store and stops the
  timer; the current sample expires after 3 s.
- Breaks: before a `streamStart` sample 1 s after the previous one (a quick
  restart), and independently on a time gap over 2.5 s.
- Chart data shaping: samples to aligned columns with `null` at breaks.
- Formatting of CPU, memory, and rates; `—` for `null`; memory % only with
  usage and a limit.

## 8. Implementation plan

Each slice ends with a working, checked state. Stop for review after S2.

| Slice                        | Work                                                                                                                                          | Done when                        |
| ---------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------- |
| **S1 Core: stats**           | FR-2 formulas; per-container streams with retry; supervisor with event fan-out, generation restarts, 1 s batching; unit and fake Engine tests | §7.1, §7.3 pass                  |
| **S2 Delivery and list**     | `subscribe_stats`; stats store; list columns and the 1,024 px layout fix                                                                      | AC-1 to AC-3, AC-5, AC-7 to AC-9 |
| **S3 Stats tab**             | `TimeSeriesChart` (D1); route and tab; current values; four synchronized charts; states                                                       | AC-4, AC-6                       |
| **S4 Docs and verification** | README, overview §3, §4, §7, §10, ADR 0003 for §10; run the AC checklist and record results here                                              | AC-10 to AC-12                   |

## 9. Known limitations accepted for M2

- Docker samples once per second; Docktide cannot sample faster or slower.
- A stream's first sample has no CPU value, so CPU appears about 1 s after memory.
- History lives in the page: a reload, or a restart of Docktide, starts it over.
- Sampling continues while the window is hidden. Each running container costs one
  Engine connection and some `dockerd` work per second, as `docker stats` does.
- Rootless Docker without cgroup v2 controller delegation reports no CPU or
  memory; values show `—`.
- A failed stream is retried with a backoff of up to 30 s; meanwhile its values
  show `—`.
- Containers using the host network have no network values.
- Memory % uses the host's memory when the container has no limit, as `docker stats`.

## 10. Decisions

Resolved on 2026-10-24; to be recorded in ADR 0003 (S4).

1. **D1 Charts:** **our own canvas chart component** (§5.2), made for exactly
   these four live time series, with no dependency. Chosen over uPlot, which
   was recommended, and over Recharts (SVG, re-renders the whole tree each
   tick, much larger).
2. **D2 What is sampled:** **every running container while Docktide is
   connected**, like `docker stats`, so the list shows numbers and the Stats
   tab opens with history. Rejected: only the container whose Stats tab is open.
3. **D3 Where history lives:** **the frontend store** (FR-4). It receives every
   sample anyway; a Rust ring buffer would be a second copy whose only benefit
   is surviving a page reload. Changes overview §4, which planned Rust ring
   buffers; Rust stays the authority for derived values.
4. **D4 History length:** **10 minutes** (600 samples, roughly 50 KB per container).
5. **D5 CPU scale:** **as `docker stats`**, 100 % per core, so values match the CLI.

Implementation calls (not separately approved):

- One Channel and one supervisor for all containers, not one per container: a
  second subscriber would add nothing, and one batch per second bounds rendering.
- Lifecycle comes from the events supervisor's container events, fanned out
  before coalescing, so there is no polling and no second events stream.
- A new stream per run (`start` replaces it), because Docker's stream carries
  on across a quick restart with no boundary; `streamStart` marks the boundary.
- `die` closes a stream, and the zero `read` time is kept as a safety net,
  because Docker never ends a stats stream itself.
- The memory chart shows the limit line only for containers with a limit, since
  the host's memory would flatten the usage line.

## 11. Verification

Not run yet.
