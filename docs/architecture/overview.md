# Docktide architecture overview

Status: **Accepted** (living document) · Last updated: 2026-10-22

This document describes how Docktide is structured and why. Individual decisions
are recorded as ADRs in [`decisions/`](decisions/). Feature-level behaviour lives
in [`../specifications/`](../specifications/).

## 1. Purpose

Docktide is a Linux-first desktop application for managing a local Docker Engine:
containers, images, Compose projects, volumes, and networks, with monitoring,
logs, and terminals built into those workflows. It is not a Docker Desktop
replacement in scope; it is a fast, native front end to an Engine the user
already runs.

## 2. System context

```text
┌──────────────────────────── Docktide process (runs as the user) ────────────────────────────┐
│                                                                                              │
│  WebView (WebKitGTK)                 src-tauri (adapter)               docktide-core          │
│  ┌──────────────────────┐  invoke   ┌──────────────────────┐  calls   ┌──────────────────┐   │
│  │ React UI             │ ────────▶ │ #[tauri::command]s   │ ───────▶ │ DockerService    │   │
│  │ TanStack Query cache │ ◀──────── │ AppState, lifecycle  │ ◀─────── │ endpoint, errors │   │
│  │ Zustand (UI only)    │  result / │ error → IPC mapping  │  domain  │ containers, ...  │   │
│  └──────────────────────┘  Channel  └──────────────────────┘  types   └────────┬─────────┘   │
│                           (streams)                                            │ Bollard     │
└────────────────────────────────────────────────────────────────────────────────┼─────────────┘
                                                                                 │ HTTP over Unix socket
                                                                       ┌─────────▼─────────┐
                                                                       │  Docker Engine    │
                                                                       │  (rootful or      │
                                                                       │   rootless)       │
                                                                       └───────────────────┘
                                   `docker compose` CLI, invoked with fixed argv (later, Compose orchestration only)
```

Privileged access (the Docker socket) is reachable only from Rust. The WebView
never sees the socket path as a capability, never runs shell commands, and can
only call the explicitly registered Tauri commands.

## 3. Components

### 3.1 `crates/docktide-core` (library, no Tauri dependency)

Owns everything that talks to Docker and every rule about Docker data.

| Module       | Responsibility                                                                  | Milestone |
| ------------ | ------------------------------------------------------------------------------- | --------- |
| `endpoint`   | Resolve which daemon to talk to (`DOCKER_HOST`, contexts, rootless fallback)    | M0        |
| `connection` | Build the Bollard client, ping, negotiate API version, report connection status | M0        |
| `error`      | Typed `DockerError` with stable, UI-relevant kinds; classification of failures  | M0        |
| `system`     | Engine info and version                                                         | M0        |
| `containers` | List, map to domain types, lifecycle operations                                 | M0        |
| `streams`    | Registry of running producers and their cancellation tokens                     | M1        |
| `events`     | Supervised events stream, relevance filter, coalesced invalidation hints        | M1        |
| `inspect`    | Container details from `inspect`, plus the raw JSON                             | M1        |
| `logs`       | Log streaming: line assembly, escape filtering, paced batches, resume           | M1        |
| `stats`      | Metrics sampling, bounded history buffers                                       | later     |
| `exec`       | Interactive exec sessions                                                       | later     |
| `compose`    | Project discovery from labels; CLI orchestration with fixed argv                | later     |

Dependencies: `bollard`, `tokio`, `tokio-util`, `serde`, `thiserror`, `tracing`, `futures-util`.
It must compile and test without a display server, WebKitGTK, or Tauri.

The proposed `docktide-monitor` crate is **deferred** (see ADR 0001). Monitoring
starts as modules inside `docktide-core` and moves into its own crate once it has
real code and a second reason to exist, such as compile time or a second consumer
like a CLI or TUI.

### 3.2 `src-tauri` (binary, the adapter)

- Registers commands. Each command is a thin translation: deserialize arguments,
  call `docktide-core`, map `DockerError` to the IPC error shape.
- Holds the `DockerService` (cheap to clone; its state is behind an `Arc`) via
  `tauri::Manager::manage`.
- Bridges streams: each streaming command takes a `Channel` and passes a sink
  closure to `docktide-core`. Because a `Channel` does not report a reloaded or
  closed page, the adapter cancels every stream when a page starts loading and
  when the window is destroyed.
- Initializes `tracing-subscriber`, the window, and later the tray, notifications,
  and window-state persistence.
- Contains no Docker logic and no business rules.

### 3.3 `src/` (React frontend)

```text
src/
├── app/                 # shell, router (TanStack Router, hash history), providers, sidebar
├── features/
│   ├── connection/      # status indicator, connection error panel
│   ├── containers/      # list page, filter, state badge, lifecycle actions
│   ├── container-details/ # details page, Overview and Inspect tabs, display helpers
│   ├── logs/            # Logs tab, ANSI parser, log buffer, stream session
│   └── live-updates/    # events subscription and hint → query invalidation
├── lib/
│   ├── ipc.ts           # typed wrappers around invoke(); the only place invoke() is called
│   └── errors.ts        # IPC error type guards and user-facing messages
├── bindings/            # generated TypeScript types from Rust (checked in)
├── components/ui/       # shadcn/ui components (generated, owned by us)
└── stores/              # Zustand stores: UI preferences and transient UI state only
```

Routes are code-based (ADR 0002 D1): `/containers`, `/containers/$id/overview`,
`/containers/$id/logs`, `/containers/$id/inspect`. Pages load data through
TanStack Query, not route loaders.

## 4. State ownership

There is exactly one authority for each piece of state.

| State                                   | Authority                                                              | Cached / displayed by                                        |
| --------------------------------------- | ---------------------------------------------------------------------- | ------------------------------------------------------------ |
| Containers, images, volumes, networks   | Docker Engine                                                          | TanStack Query (frontend), no Rust cache                     |
| Connection status, resolved endpoint    | `docktide-core`                                                        | TanStack Query                                               |
| Log lines in view                       | Docker Engine (the log file)                                           | the logs session in `features/logs` (stream data, not Query) |
| Metrics history (later)                 | `docktide-core` ring buffers (derived data that only Rust can produce) | TanStack Query / channel subscribers                         |
| Filter text, last used tab, preferences | Zustand                                                                | n/a                                                          |
| Window geometry (later)                 | Tauri window-state plugin                                              | n/a                                                          |

Consequences:

- Rust does not mirror Docker resources. The events stream pushes **invalidation
  hints** ("these containers changed", "connected", "disconnected") and the
  frontend refetches through TanStack Query (ADR 0002 D7). A Rust-side reconciled
  store is added only if refetch-on-hint proves too expensive.
- Zustand never holds server data. A container list in a Zustand store is a bug.
- Stream data (log lines) lives in plain objects owned by the feature, outside
  TanStack Query and Zustand, and is mutated in place with a version counter.

## 5. IPC contract

- **Request/response:** Tauri commands, `async`, returning `Result<T, IpcError>`.
- **Streams:** `tauri::ipc::Channel<T>` per subscription, not global events.
  Rust owns rate limiting and coalescing before data crosses the bridge.
  `subscribe_live_updates` (one per app; a new call replaces the old) and
  `open_logs` / `close_logs` (one per log view, returning a subscription id).
- **Naming:** commands are `snake_case`. JSON fields are `camelCase`
  (`#[serde(rename_all = "camelCase")]`).
- **Errors:** a stable, serializable shape:

  ```ts
  type IpcError = {
    kind:
      | "daemonUnavailable"
      | "permissionDenied"
      | "unsupportedEndpoint"
      | "invalidConfiguration"
      | "apiVersionUnsupported"
      | "notFound"
      | "conflict"
      | "timeout"
      | "internal";
    message: string; // human-readable, already safe to show
    hint?: string; // actionable next step, if any
  };
  ```

  The frontend switches on `kind` and never parses `message`.

- **Types:** DTO TypeScript types are generated from Rust (proposed: `ts-rs`, see
  ADR 0001 §D8) into `src/bindings/` and checked in. A test fails when they are stale.

## 6. Docker connectivity

### 6.1 Endpoint resolution

This mirrors the Docker CLI's precedence, so Docktide talks to the same daemon
as `docker` in the user's terminal:

1. `DOCKER_HOST` environment variable.
2. `DOCKER_CONTEXT` environment variable → context metadata.
3. `currentContext` in `$DOCKER_CONFIG/config.json` (default `~/.docker/config.json`)
   → context metadata at `contexts/meta/<sha256(name)>/meta.json`, field
   `Endpoints.docker.Host`.
4. Default `unix:///var/run/docker.sock`.
5. If (4) does not exist, fall back to the rootless socket
   `$XDG_RUNTIME_DIR/docker.sock`.

The fallback in step 5 is not CLI behaviour. It exists because desktop launchers
often do not inherit `DOCKER_HOST` from shell startup files, which is exactly how
rootless setups are usually configured.

Bollard does **not** implement Docker contexts (verified in 0.21.1 source), so
resolution is Docktide code. It is a pure function over injected inputs
(environment, file reader), which keeps it unit-testable.

The resolved endpoint and its source (for example "context `rootless`" or "default
socket") are shown in the UI so users can diagnose the "wrong daemon" case.

M0 supports `unix://` endpoints only. `tcp://` and `ssh://` endpoints resolve but
report `unsupportedEndpoint`. Remote daemons are a later decision with security
implications (TLS material, SSH agents).

### 6.2 API version negotiation

- Connect with Bollard's default client version (1.53 in 0.21.1), then call
  `negotiate_version()`. This lowers the version to the server's when the server is
  older and never raises it (verified in source).
- Docktide declares a **minimum supported API version** (proposed 1.44 / Engine 25;
  an open decision). Below it, the connection status reports `apiVersionUnsupported`
  instead of failing in odd places.
- The negotiated version is shown in the UI and logged.
- Code that depends on a field added after the minimum version must handle its absence.

### 6.3 Image store neutrality

The development host runs the **containerd image store**
(`driver-type: io.containerd.snapshotter.v1`), which is the default for new
Engine 29 installs. Docktide must not assume classic graph-driver semantics: image
sizes, shared size, multi-platform indexes, and `GraphDriver` fields differ.
This matters from the Images milestone onward and is called out in that spec.

### 6.4 Connection lifecycle

- The client is created and verified lazily, on first use, and cached. A
  connection-level failure drops it, and the next request resolves the endpoint
  and connects again. A generation counter (`tokio::sync::watch`) is bumped
  whenever the cached client is replaced or dropped.
- The events supervisor (`events.rs`) keeps one stream open on the current
  client. It reports `connected` after a ping succeeds and `disconnected` when
  the stream ends or fails, then retries with a capped backoff (1, 2, 4, 8, 16,
  then 30 s). A client change, including a successful status check elsewhere,
  restarts it at once.
- There is no periodic poll (ADR 0002 D5). Status is re-checked on window focus,
  and on `connected` the frontend refetches everything, because events during a
  gap are lost.

## 7. Streams and concurrency

As built in M1; stats and exec follow the same rules.

- Every long-running task gets a `tokio_util::sync::CancellationToken` and is
  held in the `StreamRegistry` in `DockerService`, keyed by subscription id.
  Cancelling drops the Docker stream at once, so no producer outlives its
  subscription by more than one read.
- A sink that reports a closed `Channel` stops its producer. The adapter also
  cancels every stream when a page starts loading and when a window is destroyed.
- Rust paces what crosses the bridge, because a `Channel` send never blocks:
  - events: relevant ids coalesced for 250 ms, at most one hint per window;
  - logs: batches of up to 500 lines, at most one per 50 ms; a 10,000-line
    queue drops the oldest and reports a `gap`.
- The frontend renders stream data at most once per animation frame.
- Log subscriptions are per view and not shared. Producers whose output is the
  same for every subscriber (stats, later) are keyed, for example
  `stats:<container-id>`, so a second subscriber shares the existing producer.

## 8. Security model

- Docker socket access is **root-equivalent**. Docktide runs as the desktop
  user and never asks to run as root, never calls `sudo` or `pkexec`, and never
  writes to the socket permissions.
- Permission-denied guidance explains the trade-off: the `docker` group is
  root-equivalent, and rootless Docker is the safer option.
- No generic command execution. There is no Tauri shell plugin and no "run
  arbitrary docker CLI" command. The Compose CLI, when added, is spawned with a fixed
  program path and an argv vector (no shell), built from validated inputs.
- Tauri capabilities are minimal: core defaults plus Docktide's commands. No `fs`,
  `shell`, or `http` plugins. A strict CSP is set and no remote content is loaded.
- Secrets (registry credentials, later) stay in Rust and are delegated to Docker
  credential helpers where possible. They are never stored in frontend storage.

## 9. Testing strategy

| Layer                                                                  | Tooling                                                                  | Runs by default | Needs Docker |
| ---------------------------------------------------------------------- | ------------------------------------------------------------------------ | --------------- | ------------ |
| Core domain logic (mapping, endpoint resolution, error classification) | `cargo test`                                                             | yes             | no           |
| Core against a real Engine                                             | `cargo test -- --ignored` (tests marked `#[ignore = "requires Docker"]`) | no              | yes          |
| Tauri adapter                                                          | compile and clippy; logic is too thin to unit test                       | yes             | no           |
| Frontend logic (filters, action availability, error mapping)           | `vp test` (Vitest)                                                       | yes             | no           |
| Frontend static checks                                                 | `vp check` (Oxfmt, Oxlint, type check on TypeScript 7)                   | yes             | no           |
| End to end                                                             | manual checklist per milestone (automated E2E deferred)                  | n/a             | yes          |

Docker integration tests must create and remove **their own** containers, labelled
`io.docktide.test=true`, from a small image. They must never stop, restart, or
remove containers they did not create. The development host runs real workloads.

## 10. Roadmap (tentative)

| Milestone | Theme                                                                      |
| --------- | -------------------------------------------------------------------------- |
| M0        | Foundation: shell, connection, container list, start/stop/restart (done)   |
| M1        | Live updates via events stream, container details and inspect, logs (done) |
| M2        | Monitoring: stats streams, bounded history, charts                         |
| M3        | Compose: project discovery, grouping, service lifecycle                    |
| M4        | Images: list, inspect, pull, remove, create container                      |
| M5        | Volumes and networks                                                       |
| M6        | Interactive exec terminal (xterm.js)                                       |
| ongoing   | Desktop integration (tray, notifications, window state) and packaging      |

The order after M1 is open for discussion. Each milestone's decisions are in its
spec (§10) and, where they bind later work, in an ADR.
