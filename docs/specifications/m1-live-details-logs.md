# M1: Live updates, container details, and logs

- Status: **Approved** 2026-10-21. Decisions in §10.
- Date: 2026-10-21
- Related: [Architecture overview](../architecture/overview.md), [ADR 0001](../architecture/decisions/0001-initial-architecture.md), [M0 spec](m0-foundation.md)

## 1. Goal

Docktide reflects what happens on the Engine without a refresh, and a container
can be opened to see its configuration and follow its logs. This is the first
milestone with long-running streams, so it also establishes the stream
infrastructure (overview §7) that stats, exec, and Compose build on.

## 2. Scope

In scope:

1. A supervised Docker events stream in `docktide-core` that pushes coalesced
   invalidation hints to the frontend over a Tauri `Channel`.
2. Connection status driven by that stream. The 10 s ping poll is removed (§10, D5).
3. Container details page: overview from `inspect`, raw inspect JSON, and the
   same start/stop/restart actions as the list.
4. Logs: tail plus follow over a `Channel`, stdout/stderr distinction, TTY
   containers, resume after the container restarts or Docker reconnects.
5. Navigation between the list and a container's page.
6. Stream lifecycle: cancellation, bounded buffers, cleanup on page reload.

Non-goals (explicitly deferred):

- Stats, CPU/memory charts (M2). Events for images, volumes, networks (their
  milestones). Exec terminal (M6).
- Pause/unpause, kill, remove, rename, create. Editing container configuration.
- Saving logs to a file (needs a dialog and an `fs` capability), multi-container
  log views, log search across history beyond the view buffer, `until` ranges.
- Remote endpoints, context switcher UI, tray, notifications, packaging, CI.

## 3. Functional requirements

### FR-1 Events supervisor

`DockerService` runs at most one events task. `subscribe_live_updates` starts
it and replaces a previous one; cancelling the subscription, or a closed sink,
stops it.

- Request: `GET /events` with `filters={"type":["container"]}` and `since` set to
  the time just before the connectivity check. Events between the check, the
  frontend's refetch, and the stream actually opening are then replayed rather
  than lost; a duplicate only causes an extra refetch.
- Lifecycle:

  | Situation                                                          | Supervisor behaviour                                                                                                                                                                                                                       |
  | ------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
  | Client available and the Engine answers a ping                     | Emit `connected`, reset backoff, open the stream. (Bollard yields nothing until the first event, so the ping, not the stream, proves connectivity.)                                                                                        |
  | Stream errors or ends                                              | Emit `disconnected` and drop the client, so the next status check reconnects and reports the truth. A clean end, or a transport error without a Docker response (hyper reports a cut body without an `io::Error`), is `daemonUnavailable`. |
  | No client available                                                | Emit `disconnected` once (only state changes are reported), then retry after a capped exponential backoff: 1, 2, 4, 8, 16, 30, 30, … s. Each attempt re-resolves the endpoint.                                                             |
  | The cached client changes (new endpoint, or a reconnect elsewhere) | Restart the stream on the new client and emit `connected`.                                                                                                                                                                                 |
  | A `connection_status` call connects while the supervisor backs off | Retry now instead of waiting out the backoff.                                                                                                                                                                                              |
  | Cancelled                                                          | Drop the stream; emit nothing.                                                                                                                                                                                                             |

  The client-change signal is a generation counter in `DockerService` (a
  `tokio::sync::watch`), bumped whenever the cached client is replaced or dropped.

- Relevant actions (an allowlist, so new Engine actions do not cause refetch storms):
  `create`, `start`, `restart`, `stop`, `die`, `kill`, `oom`, `pause`, `unpause`,
  `destroy`, `rename`, `update`, and any `health_status*`. All others, notably
  `exec_*` (one per healthcheck run), `attach`, `resize`, `top`, `archive-path`,
  `extract-to-dir`, and `export`, are ignored.
- Coalescing: relevant events are collected for 250 ms after the first one in a
  window, then emitted as one `containersChanged` hint with the distinct container
  ids. At most one hint per 250 ms leaves Rust.
- Events that happened while the stream was down are lost; `connected` tells the
  frontend to refetch everything (overview §6.4).
- The stream must outlive Bollard's 120 s request timeout. Bollard applies that
  timeout to receiving the response headers only (verified in 0.21.1 source,
  `execute_request`); a test confirms an idle stream stays open (§7.3).

### FR-2 Live updates in the frontend

Command `subscribe_live_updates(onUpdate: Channel<LiveUpdate>) -> null`.

- Called once at startup by a `useLiveUpdates()` hook in `Providers`.
- A second call replaces the previous subscriber.
- Hint handling (all through the shared `QueryClient`):

  | Hint                     | Invalidates                                      |
  | ------------------------ | ------------------------------------------------ |
  | `connected`              | everything (`invalidateQueries()` with no key)   |
  | `disconnected`           | `["connection"]`                                 |
  | `containersChanged(ids)` | `["containers"]`, and `["container", id]` per id |

- `["connection"]` loses its `refetchInterval`. `refetchOnWindowFocus` stays,
  which is how an endpoint change made while Docktide is running is picked up
  (FR-1 then restarts the stream on the new client).
- Mutations keep invalidating `["containers"]` on settle, so the row's pending
  state still ends when the list shows the new state; TanStack Query dedupes the
  refetch the following event causes.

### FR-3 Container details

Command `get_container({ id }) -> Result<ContainerInspect, IpcError>`, one
`GET /containers/{id}/json` without `size` (computing sizes is slow).

`ContainerInspect.details` is a mapped DTO (§4). Mapping rules:

- `name` without the leading `/`; `state` mapped as in M0 FR-3.
- Times: Docker's zero time `0001-01-01T00:00:00Z` becomes `null`.
- `exitCode` is `null` unless the container has stopped at least once (`finishedAt` set).
- `health` is `null` when no healthcheck is configured.
- `ports` from `NetworkSettings.Ports` with the M0 normalization and order.
- `env` split at the first `=`; an entry without `=` has an empty value. Order as Docker reports it.
- `labels` sorted by key.
- `mounts` sorted by destination; `networks` sorted by name.

`ContainerInspect.raw` is Bollard's `ContainerInspectResponse` re-serialized to
JSON, which uses Docker's field names. It is not byte-identical to
`docker inspect`: fields unknown to Bollard's API model are dropped and unset
fields are omitted (§9).

`notFound` is a normal outcome (the container was removed while its page was
open); the page shows a removed state instead of an error (§5.3).

### FR-4 Logs

Commands:

- `open_logs({ id, tail, after, onEvent: Channel<LogEvent> }) -> Result<number, IpcError>`
  returns a subscription id.
  - `tail`: number of lines, or `null` for all.
  - `after`: the timestamp of the last line already shown, or `null`. When set,
    `tail` is ignored, Docker is asked for `since` = that second, and lines at or
    before `after` are dropped in Rust (Docker's `since` has one-second resolution).
- `close_logs({ subscription }) -> null`. Unknown ids are ignored.

Request: `GET /containers/{id}/logs?follow=1&stdout=1&stderr=1&timestamps=1`.

Line assembly in Rust:

- Non-TTY containers deliver multiplexed frames. Each frame is
  `<RFC 3339 nano timestamp> <payload>`. A frame whose payload does not end in
  `\n` (Docker splits messages over 16 KiB) is continued by the next frame of the
  same stream; the continuation's timestamp is dropped.
- TTY containers deliver one raw stream (`console`) in arbitrary chunks. Bytes are
  buffered and split on `\n`; each line starts with a timestamp.
- Inside a line, a `\r` that is not trailing keeps only the text after the last
  one, as a terminal would show it. Trailing `\r` is removed.
- Text is decoded as lossy UTF-8. SGR sequences (`ESC [ … m`, colours and text
  attributes) are kept for the frontend to render (§10, D3). All other escape
  sequences (cursor movement, erase, OSC titles and links) and other C0 control
  characters except tab are stripped.
- A partial line longer than 64 KiB is emitted as a line to bound memory.
- Leftover partial text is emitted when the stream ends.
- Confirmed in S4 against Engine 29 with the `json-file` driver: a TTY container's
  lines each carry a timestamp, even when a line was printed in parts, and end in
  `\r\r\n` (the terminal's `\r\n` plus Docker's `\r`). A non-TTY message over
  16 KiB arrives as two frames with the same timestamp.

Delivery:

- Lines are batched: a batch leaves 50 ms after its first line, or as soon as it
  holds 500 lines. Batches hold at most 500 lines and leave at most every 50 ms,
  which caps delivery at 10,000 lines/s. A `Channel` send never blocks, so this
  pacing is what protects the webview.
- Lines waiting for delivery sit in a bounded queue of 10,000 lines. When it is
  full, the oldest lines are dropped and a `gap` event with the count precedes
  the next batch.
- When the stream ends, queued lines are sent at once, before `ended` or `error`.
- When Docker closes the stream (the container stopped or was removed), Rust
  sends `ended` and releases the subscription.
- A Docker failure sends `error` with the classified `IpcError` and releases the
  subscription. Connection-level failures also go through `DockerService::failed`.
- A logging driver that cannot be read (for example `none`) is reported as
  `error` with Docker's message.

Log subscriptions are not shared between subscribers; each has its own `tail`
and `after`. There is one view per container in M1, so duplicates do not arise.

### FR-5 Stream registry and cleanup

- Every producer (events, each log subscription) has a
  `tokio_util::sync::CancellationToken` and is held in a registry in
  `DockerService`, keyed by subscription id.
- A Tauri `Channel` does not report a reloaded or closed webview to Rust, so the
  adapter cancels all of a webview's subscriptions when it starts loading a page
  (`Builder::on_page_load`, `PageLoadEvent::Started`) and when its window is
  destroyed. The frontend also calls `close_logs` on unmount.
- A failed `Channel::send` cancels that producer.
- No producer outlives its subscription by more than one Docker read.

## 4. IPC types

New types, generated into `src/bindings/`:

```ts
type LiveUpdate =
  | { type: "connected" }
  | { type: "disconnected"; error: IpcError }
  | { type: "containersChanged"; ids: string[] }; // full ids, deduplicated

type HealthStatus = "starting" | "healthy" | "unhealthy" | "unknown";

type ContainerHealth = {
  status: HealthStatus;
  failingStreak: number;
  lastOutput: string | null; // output of the most recent check, trimmed
};

type MountType = "bind" | "volume" | "tmpfs" | "npipe" | "cluster" | "image" | "unknown";

type MountInfo = {
  type: MountType;
  name: string | null; // volume name
  source: string;
  destination: string;
  readOnly: boolean;
};

type NetworkAttachment = {
  name: string;
  ipAddress: string | null;
  ipv6Address: string | null;
  gateway: string | null;
  macAddress: string | null;
  aliases: string[];
};

type KeyValue = { key: string; value: string };

type ContainerDetails = {
  id: string;
  name: string;
  image: string; // Config.Image, as the user asked for it
  imageId: string; // "sha256:…"
  state: ContainerState;
  createdAt: string; // RFC 3339
  startedAt: string | null;
  finishedAt: string | null;
  exitCode: number | null;
  oomKilled: boolean;
  error: string | null; // State.Error, when non-empty
  health: ContainerHealth | null;
  entrypoint: string[];
  command: string[];
  workingDir: string | null;
  user: string | null;
  tty: boolean;
  restartPolicy: string; // "no", "always", "unless-stopped", "on-failure:5"
  restartCount: number;
  ports: PortMapping[];
  mounts: MountInfo[];
  networks: NetworkAttachment[];
  env: KeyValue[];
  labels: KeyValue[];
  memoryLimit: number | null; // bytes; null when unlimited
  cpuLimit: number | null; // CPUs (NanoCpus / 1e9); null when unlimited
  logDriver: string;
};

type ContainerInspect = { details: ContainerDetails; raw: unknown }; // Docker's JSON, only displayed

type LogStream = "stdout" | "stderr" | "console";

type LogLine = {
  stream: LogStream;
  timestamp: string; // RFC 3339 nano, UTC, as Docker sent it
  text: string; // may contain SGR sequences (FR-4), nothing else
};

type LogEvent =
  | { type: "lines"; lines: LogLine[] }
  | { type: "gap"; dropped: number }
  | { type: "ended" }
  | { type: "error"; error: IpcError };
```

`ErrorKind` is unchanged. The new commands are listed in `src-tauri/build.rs` and
granted in `capabilities/default.json`.

## 5. UI specification

### 5.1 Navigation

- TanStack Router (`@tanstack/react-router`, §10, D1) with code-based routes
  and hash history (`createHashHistory`), since the app is served from bundled
  files. No router Vite plugin.
- Routes:

  | Path                       | Page                                                |
  | -------------------------- | --------------------------------------------------- |
  | `/`                        | redirects to `/containers`                          |
  | `/containers`              | the list                                            |
  | `/containers/$id`          | redirects to the last used tab (default `overview`) |
  | `/containers/$id/overview` | details, Overview tab                               |
  | `/containers/$id/logs`     | details, Logs tab                                   |
  | `/containers/$id/inspect`  | details, Inspect tab                                |
  | anything else              | not-found component with a link to `/containers`    |

- The root route renders the shell (sidebar plus outlet). Links use the router's
  typed `Link`; the router type is registered for type-checked paths and params.
- Data loading stays in TanStack Query inside components; route loaders are not
  used, so the connection gating of M0 keeps working unchanged.
- Back and forward (Alt+←/→, mouse back and forward buttons) work through the webview
  history. WebKitGTK leaves these to browser chrome, so the shell handles them.
- In the list, the container name is a link to its overview. Action buttons do
  not navigate.
- The Containers sidebar entry is active (`aria-current="page"`) on every
  `/containers` route and links back to the list.
- Filter state in Zustand survives the round trip.

### 5.2 Live updates on the list

- The refresh button stays, for reassurance; it is no longer needed.
- No visible change otherwise: rows update in place as hints arrive.

### 5.3 Details page

Header: back link "Containers", name, short id, state badge, Docker's status
text, and the start/stop/restart buttons with M0's `availableActions` and pending
rules. Below it, tabs: Overview, Logs, Inspect. The last used tab is kept in
Zustand for the session and used when another container is opened from the list.

Overview, in sections:

| Section     | Content                                                                                                                                                  |
| ----------- | -------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Status      | Created; started or finished (relative, absolute in a tooltip), exit code, OOM, error, health with last output (while running), restart policy and count |
| Image       | Image reference and image id (short, full in a tooltip)                                                                                                  |
| Process     | Entrypoint and command (shell-quoted for display), working dir, user, TTY                                                                                |
| Ports       | M0 port format                                                                                                                                           |
| Mounts      | Type, source (or volume name), destination, read-only                                                                                                    |
| Networks    | Name, IPv4, IPv6, gateway, MAC, aliases                                                                                                                  |
| Environment | Names with values hidden until "Show values" is pressed (§10, D4); not remembered                                                                        |
| Labels      | Key and value; long values truncated with the full value in a tooltip                                                                                    |
| Resources   | Memory and CPU limits, or "Unlimited"; log driver                                                                                                        |

Empty sections show "None" rather than disappearing.

Inspect: the raw JSON pretty-printed (2-space indent) in a monospace,
selectable block, with a "Copy" button. Values are not masked here (§10, D4).

States:

| Condition    | Rendering                                                                 |
| ------------ | ------------------------------------------------------------------------- |
| Disconnected | Connection panel, as on the list                                          |
| Loading      | Skeleton header and sections                                              |
| `notFound`   | "This container no longer exists." with a link back to the list; no toast |
| Other error  | Inline error panel with message, hint, and "Retry"                        |

The query key is `["container", id]`; live updates keep it fresh (FR-2).

### 5.4 Logs tab

- Opens `open_logs` with `tail: 1000` (§10, D6) when the tab mounts and closes it
  on unmount or when the container id changes.
- Lines are held in a ring buffer of 10,000 lines in the hook (not in TanStack
  Query or Zustand: stream data, overview §4). When lines fall off the front, a
  notice "Older lines are no longer shown" appears at the top.
- Rendering is virtualized (§10, D2) and batched to one update per animation frame.
- Follow: the view sticks to the bottom while it is at the bottom. Scrolling up
  pauses following and shows "Jump to latest" with the count of new lines.
- Toolbar:
  - Filter: case-insensitive substring; non-matching lines are hidden, the count
    shows "N of M lines".
  - Timestamps toggle (local time, `HH:mm:ss.SSS`, full date in a tooltip); off by default.
  - Wrap toggle; on by default.
  - Clear: empties the view only; new lines keep arriving.
  - Copy: copies the lines in view (after filtering) as plain text.
  - Tail selector: 100, 1,000, 10,000, All. Changing it reopens the stream.
- stderr lines have a red left border and an accessible "stderr" label. Console
  (TTY) lines have no marker.
- Colours (§10, D3): a pure `parseAnsi(text)` turns SGR sequences into styled
  segments plus the plain text. Supported: reset, bold, dim, italic, underline,
  inverse, strikethrough, the 16 standard and bright colours, the 256-colour
  palette, and 24-bit colours, for foreground and background. The 16 standard
  colours map to theme tokens chosen to be readable in light and dark themes;
  256 and 24-bit colours are used as given. Unknown SGR codes are ignored.
- Style state resets at the start of every line. Filter, copy, and search work
  on the plain text. Segments are rendered as React elements, never as HTML strings.
- A `gap` event inserts a marker line "N lines skipped (output too fast)".
- Stream states:

  | Condition                                 | Rendering                                                                          |
  | ----------------------------------------- | ---------------------------------------------------------------------------------- |
  | Opening                                   | Spinner in place of the lines                                                      |
  | Following                                 | Lines; a small "Live" indicator                                                    |
  | `ended`, container not running            | Lines kept; a footer "Container stopped. Logs resume when it starts."              |
  | Container running again (details refetch) | Reopens with `after` = last timestamp; footer cleared                              |
  | `ended`, container removed                | The page's `notFound` state takes over                                             |
  | Disconnected                              | Lines kept; footer "Docker is not reachable." On `connected`, reopens with `after` |
  | `error` (not connection-level)            | Lines kept; inline error with message and "Retry"                                  |
  | No lines at all                           | "No log output yet"                                                                |

## 6. Acceptance criteria

| ID    | Criterion                                                                                                                                                            | Verification                                                                                  |
| ----- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| AC-1  | A container started, stopped, created, or removed with the `docker` CLI appears in the list within about 1 s, without refresh or focus change                        | manual + integration test on the events supervisor                                            |
| AC-2  | A healthcheck changing to `healthy` updates the status text; healthcheck `exec_*` events alone cause no refetch                                                      | integration test (busybox with a healthcheck) + log of refetch counts                         |
| AC-3  | Stopping Docker (or switching to a dead endpoint) shows "Disconnected" within about 2 s; after recovery, "Connected" and a fresh list within the backoff cap         | simulated as in M0 AC-8 (scratch `DOCKER_CONFIG`, fake Engine); real daemon only with consent |
| AC-4  | No periodic Docker requests while idle and connected                                                                                                                 | `strace -f -e connect` or a fake Engine request log over 60 s                                 |
| AC-5  | The details page shows the overview fields of §5.3 matching `docker inspect` for a test container with ports, a volume, a bind mount, env, labels, and a healthcheck | integration test on the mapping + manual comparison                                           |
| AC-6  | Inspect shows the JSON and "Copy" puts it on the clipboard                                                                                                           | manual                                                                                        |
| AC-7  | Removing an open container shows the removed state, not an error toast                                                                                               | manual                                                                                        |
| AC-8  | Logs show the last 1,000 lines and follow new output; stderr is marked; a TTY container's output is readable                                                         | integration tests + manual                                                                    |
| AC-9  | Restarting a container while its logs are open resumes them without duplicated or missing lines (lines sharing the boundary timestamp excepted, §9)                  | integration test on `after` + manual                                                          |
| AC-10 | A container printing 5,000 lines/s for 30 s keeps the UI responsive (scrolling, tab switching, actions) and the view at no more than 10,000 lines                    | manual, with a `busybox` loop                                                                 |
| AC-11 | Leaving the logs tab, closing the window, or reloading the page stops the Docker log stream                                                                          | `ss -x` on the socket or the fake Engine connection count                                     |
| AC-12 | Every M0 acceptance criterion still holds, except the 10 s poll in M0 §5.1                                                                                           | re-run of the M0 checklist                                                                    |
| AC-13 | `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `vp check`, `vp test run`, and `pnpm typecheck` pass         | command output                                                                                |
| AC-14 | README and the architecture overview describe live updates, details, logs, and the stream design as built                                                            | review                                                                                        |

Any criterion not verified in the implementation session is reported as
**unverified** with the reason.

## 7. Test plan

### 7.1 Unit tests (`cargo test`, no Docker)

- Event relevance: allowlisted actions, `health_status: healthy` variants, ignored `exec_*`.
- Coalescer (paused Tokio time): bursts become one hint per 250 ms, ids deduplicated.
- Backoff schedule and reset after success.
- Line assembly: frames split mid-line, 16 KiB continuations, CRLF, inner and
  trailing `\r`, invalid UTF-8, SGR kept and other escapes stripped, the 64 KiB cap, partial line at
  end, TTY chunks split inside a timestamp.
- Timestamp normalization and the `after` comparison (Docker trims trailing
  zeros from the fraction, so strings are normalized before comparing).
- Log queue overflow: oldest dropped, `gap` count correct.
- `ContainerDetails` mapping from constructed `ContainerInspectResponse` values:
  zero times, missing health, env without `=`, mount types, port normalization,
  sorting, limits.
- Serialization of `LiveUpdate` and `LogEvent` to the shapes in §4.

### 7.2 Docker integration tests (`cargo test -- --ignored`)

Same rules as M0 §7.2: only containers labelled `io.docktide.test=true`, removed
by a drop guard.

- Events: starting and stopping a test container produces `containersChanged`
  with its id within 2 s.
- Inspect: a test container with a port, a named volume, a bind mount from a
  scratch directory, env, labels, and a healthcheck maps as expected; a removed id
  is `notFound`.
- Logs: tail of a container that printed 20 lines to stdout and stderr; follow
  receives new lines; `ended` when it stops; `after` resumes without duplicates; a
  TTY container's output arrives as `console` lines.

### 7.3 Fake Engine tests (`tests/failures.rs`)

- The events stream ends cleanly or is cut → `disconnected`, then `connected`
  after the first retry; an Engine that appears later → `connected`; a successful
  status check ends the backoff early; a new subscriber replaces the old one.
- An idle events stream stays open past a shortened request timeout.
- `close_logs` and cancellation close the Engine connection.

### 7.4 Frontend (`vp test`)

- Route tree: `/` and `/containers/$id` redirects (the latter to the last used
  tab), unknown paths reach the not-found component (memory history in tests).
- `parseAnsi`: each supported attribute and colour form, resets, unknown codes,
  malformed sequences shown as text, plain text equals input minus escapes.
- Hint → invalidated query keys.
- Log ring buffer: capacity, the "older lines" flag, gap markers, filter counts.
- Follow logic: at-bottom detection and the new-line counter.
- Display helpers: shell quoting of commands, restart policy text, limits.

## 8. Implementation plan

Each slice ends with a working, checked state. Stop for review after S2.

| Slice                        | Work                                                                                                                                            | Done when                                 |
| ---------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------- |
| **S1 Core: streams, events** | `tokio-util`; stream registry; client generation watch; events supervisor with relevance filter, coalescer, backoff; unit and fake Engine tests | §7.1 events parts, §7.3 events tests pass |
| **S2 Live updates**          | `subscribe_live_updates`, page-load cleanup in the adapter, `useLiveUpdates`, hint invalidation, remove the poll                                | AC-1 to AC-4                              |
| **S3 Details**               | `get_container` and mapping; TanStack Router with the route table of §5.1; details page with header actions, Overview, Inspect, states          | AC-5 to AC-7                              |
| **S4 Core: logs**            | line assembly, batching, queue, `after`; `open_logs` / `close_logs`; integration tests including a real TTY container                           | §7.1 logs parts, §7.2 logs tests          |
| **S5 Logs UI**               | log hook and ring buffer, virtualized view (`@tanstack/react-virtual`), `parseAnsi` and colour tokens, follow, toolbar, stream states, resume   | AC-8 to AC-11                             |
| **S6 Docs and verification** | README, overview §4–§7 and §10, ADR 0002 for the decisions in §10; run the AC checklist and record results here                                 | AC-12 to AC-14                            |

## 9. Known limitations accepted for M1

- Raw inspect is Bollard's model re-serialized: fields added to the Engine API
  after 1.53 are missing and key order may differ from `docker inspect`.
- On log resume, a line with exactly the same timestamp as the last shown line is
  dropped. Docker's nanosecond timestamps make this rare.
- An endpoint change made while the window stays focused is noticed on the next
  focus or when the current stream fails.
- Colour state does not carry across line breaks, and cursor movement is dropped,
  so full-screen TTY programs (progress bars redrawn in place) render imperfectly.
- The log view holds 10,000 lines; older output needs a larger tail or the CLI.
- Docker's `stopping` state is still shown as `unknown` (M0 §9).
- Ports come from `NetworkSettings.Ports`, so a stopped container shows none, as
  in `docker ps`; the configured bindings are in Inspect (`HostConfig.PortBindings`).
- Docker's relative status text ("Up 3 minutes") is refreshed only by a refetch;
  it goes stale while nothing changes (already true in M0).

## 10. Decisions

Resolved on 2026-10-21 (to be recorded in ADR 0002 in S6):

1. **D1 Navigation:** TanStack Router with code-based routes and hash history.
   Chosen over an in-app hash router for type-checked routes and params, which
   later milestones (images, Compose, volumes) will need.
2. **D2 Log virtualization:** `@tanstack/react-virtual`.
3. **D3 ANSI in logs:** colours and text attributes are rendered in M1 (SGR
   only; other escapes stripped).
4. **D4 Environment values:** hidden on Overview until "Show values"; shown
   as-is in Inspect, the raw escape hatch like `docker inspect`.
5. **D5 Connection poll:** removed; status follows the events stream plus
   window focus.
6. **D6 Log defaults:** initial tail 1,000 lines, view cap 10,000.

Implementation calls (not separately approved):

- The details page replaces the list rather than splitting the window, so logs
  get the full width.
- Overview is the default tab for the first container opened in a session.
- One events stream per app, with the relevance allowlist in FR-1.
- Log resume uses timestamps rather than Docker's `since` alone.
- Health is hidden on Overview once the container stops: Docker keeps the last
  result, and the CLI hides it too.
- The header's status text comes from the list query (inspect has no equivalent).
