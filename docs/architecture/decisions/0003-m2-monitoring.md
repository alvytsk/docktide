# ADR 0003: Monitoring (M2)

- Status: **Accepted**
- Date: 2026-10-24 (decisions taken 2026-10-24)
- Deciders: project maintainer
- Related: [M2 spec](../../specifications/m2-monitoring.md) §10, [ADR 0001](0001-initial-architecture.md), [ADR 0002](0002-m1-navigation-logs-and-live-updates.md)

## Context

M2 adds live container metrics: CPU, memory, network, block I/O, and PIDs, in
the list and on a Stats tab. It needed choices about how to draw time series,
which containers to sample, where history lives, how long it is, and what 100 %
CPU means. The overview (§4) had planned Rust ring buffers for metrics history;
this ADR supersedes that plan.

Facts established while deciding and implementing (Engine 29.9.2, cgroup v2):

| Area                                  | Finding                                                                                                                                                                                                                                                      |
| ------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `GET /containers/{id}/stats?stream=1` | One sample per second; the stream never ends by itself. The first sample has no `precpu_stats.system_cpu_usage`. After a stop it keeps sending zero samples (`read: "0001-01-01T00:00:00Z"`). A `docker restart` continues the same stream with no boundary. |
| Docker CLI                            | CPU % is `cpuDelta / systemDelta × onlineCpus × 100`; memory excludes inactive file cache (`calculateMemUsageUnixNoCache`). Using the same formulas should give the same values as `docker stats`.                                                           |
| Tauri `Channel`                       | A send never blocks, so Rust must pace what it sends (ADR 0002).                                                                                                                                                                                             |

## Decisions

### D1: Our own canvas chart component

`TimeSeriesChart` draws on a `<canvas>`, for exactly four live time series with
one to three lines each. It has no dependency (no new Rust or npm packages).
Alternatives: uPlot (recommended, small and fast, but a dependency with its own
styling and theming to bridge), and Recharts (SVG, re-renders the whole tree
each tick, much larger). Colours come from theme tokens, resolved to hex for the
canvas. The legend is DOM text, so values are selectable and readable by screen
readers.

### D2: Sample every running container while connected

Like `docker stats`, one stream per running or paused container, so the list
shows numbers and the Stats tab opens with history. Rejected: sampling only the
container whose Stats tab is open, which leaves the list empty and the tab
without history.

### D3: History lives in the frontend store

The stats store (outside React, `useSyncExternalStore` with a version counter)
keeps a time series per container. It receives every sample anyway, and a Rust
ring buffer would be a second copy whose only benefit is surviving a page
reload. This supersedes the overview §4 plan for Rust ring buffers. Rust stays
the authority for derived values (CPU %, memory, rates). Stream data stays out
of TanStack Query and Zustand.

### D4: Ten minutes of history

600 samples at Docker's 1 s interval, roughly 50 KB per container. Not
configurable.

### D5: CPU on the `docker stats` scale

100 % is one core, so a busy two-core container reads 200 % and values use
the CLI's scale.

### Implementation calls (not separately approved)

- One `Channel` and one supervisor for all containers, with one batch per 1 s
  tick, which bounds rendering.
- Lifecycle comes from the events supervisor: container events are fanned out
  in-process (`broadcast`, capacity 256) before coalescing. There is no second
  events stream; a lagged receiver re-lists.
- A new stream per run: `start` replaces the container's stream, `die` and
  `destroy` close it, and a zero `read` time is a safety net. `streamStart`
  marks the boundary for charts.
- Monitoring stays in `docktide-core` (`stats.rs`); the `docktide-monitor` crate
  remains deferred (ADR 0001 D2).

## Consequences

- History is lost on a page reload or restart of Docktide, and covers only time
  Docktide was open (spec §9).
- Sampling continues while the window is hidden. Each running container costs
  one Engine connection and some `dockerd` work per second, as `docker stats`.
- With no running containers there are no stats requests, so M1's idle
  guarantee holds.
- Per-stream failures other than a lost connection are retried with a 1, 2, 4,
  8, 16, then 30 s backoff. Connection-level failures go through
  `DockerService::failed` and streams reopen on the next client generation.
- Chart code is ours to maintain: scale, ticks, cursor, and theming. It is small
  and has unit tests for the pure parts; canvas rendering is checked by eye.
- Any later view of metrics (a dashboard, sparklines) reads the same store and
  must work with 10 minutes of in-memory history.
