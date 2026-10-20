# M0: Project foundation

- Status: **Approved**, S2 in progress
- Date: 2026-10-20
- Related: [Architecture overview](../architecture/overview.md), [ADR 0001](../architecture/decisions/0001-initial-architecture.md)

## 1. Goal

Prove the whole path end to end: a Tauri 2 window on Linux shows the user's real
containers from their local Docker Engine and can start, stop, and restart them.
Failures to reach Docker are explained, not crashed on.

## 2. Scope

In scope:

1. Tauri 2 + React + TypeScript project with Tailwind CSS 4 and shadcn/ui.
2. Cargo workspace: `crates/docktide-core`, `src-tauri`.
3. Endpoint resolution, connection, API version negotiation.
4. Engine info and version.
5. Container list (running and stopped): name, image, state, status, ports.
6. Start, stop, restart.
7. Typed handling of daemon-unavailable, permission-denied, and related errors.
8. List refresh after mutations.
9. App shell: sidebar, connection indicator, containers page with filter, loading,
   empty, and error states.

Non-goals (explicitly deferred):

- Live updates from Docker events (M1). External changes appear on refresh or
  window focus only.
- Container details, inspect, logs, stats, exec, pause/unpause, create/remove.
- Images, Compose, volumes, networks pages. The navigation entries exist but are
  disabled and marked "Coming soon".
- `tcp://` / `ssh://` endpoints, TLS, context switching UI.
- Tray, notifications, window-state persistence, packaging, CI.

## 3. Functional requirements

### FR-1 Endpoint resolution

`docktide_core::endpoint::resolve(env, fs) -> Result<ResolvedEndpoint, DockerError>`
is a pure function over injected inputs:

| Step | Input                                                                                     | Result source                                          |
| ---- | ----------------------------------------------------------------------------------------- | ------------------------------------------------------ |
| 1    | `DOCKER_HOST` set and non-empty                                                           | `dockerHostEnv`                                        |
| 2    | `DOCKER_CONTEXT` set and not `default`                                                    | `dockerContextEnv` (+ context name)                    |
| 3    | `currentContext` in `$DOCKER_CONFIG/config.json` (default `$HOME/.docker`), not `default` | `configContext` (+ context name)                       |
| 4    | `unix:///var/run/docker.sock` exists                                                      | `default`                                              |
| 5    | else `$XDG_RUNTIME_DIR/docker.sock` exists                                                | `rootlessFallback`                                     |
| 6    | else                                                                                      | `default` (connection will report `daemonUnavailable`) |

- Context metadata path: `$DOCKER_CONFIG/contexts/meta/<sha256-hex(name)>/meta.json`,
  host at `Endpoints.docker.Host`.
- A named context that does not exist, or unparsable config or metadata, yields
  `invalidConfiguration` with the file path in the message.
- A non-`unix://` host yields `unsupportedEndpoint` with the URI.

### FR-2 Connection status

Command `get_connection_status() -> ConnectionStatus` never fails at the IPC level.
Failures are part of the value, so the UI can always render a status.

On each call it resolves the endpoint, connects if no healthy client is cached,
pings, negotiates the API version, and fetches `/info` and `/version`. The client is
cached in `DockerService` and dropped when any request fails with a
connection-level error (`daemonUnavailable`, `permissionDenied`, `timeout`).

If the negotiated API version is below `MIN_API_VERSION` (proposed 1.44), status
is disconnected with `apiVersionUnsupported`.

### FR-3 Container list

Command `list_containers() -> Result<ContainerSummary[], IpcError>` lists all
containers (`all = true`), mapped as follows:

- `name`: first entry of `Names` without the leading `/`. Falls back to the short id.
- `state`: Docker `State` mapped to the enum. Unknown values become `unknown`, not an error.
- `ports`: wildcard host IPs (`0.0.0.0`, `::`) normalize to `hostIp: null` and are
  deduplicated (Docker reports each published port once per IP family). Sorted by
  `privatePort`, then `protocol`, then `publicPort`.
- Order: running first, then by name (case-insensitive).

### FR-4 Lifecycle operations

Commands `start_container`, `stop_container`, `restart_container`, each with
`{ id: string }` and returning `Result<null, IpcError>`.

- Stop and restart use the Engine's default timeout (the container's `StopTimeout`
  or 10 s). No custom timeout in M0.
- Idempotency: Bollard treats HTTP 304 (already started or stopped) as success, so
  the UI treats it as success too.
- HTTP 404 → `notFound`, HTTP 409 → `conflict`.

### FR-5 Error model

| Kind                    | Detected from                                                                          | User message (shape)                                           | Hint                                                                                                       |
| ----------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `daemonUnavailable`     | socket missing (`SocketNotFoundError`, `io::ErrorKind::NotFound`), `ConnectionRefused` | "Docker is not running at `<endpoint>`."                       | `systemctl start docker`, or `systemctl --user start docker` for rootless                                  |
| `permissionDenied`      | `io::ErrorKind::PermissionDenied` in the source chain                                  | "Permission denied for `<endpoint>`."                          | Explains the `docker` group (root-equivalent) vs rootless Docker. Never suggests running Docktide as root. |
| `unsupportedEndpoint`   | non-`unix://` scheme                                                                   | "`<uri>` endpoints are not supported yet."                     | Set `DOCKER_HOST` or a context to a local socket                                                           |
| `invalidConfiguration`  | missing context, malformed config or metadata                                          | "Docker context `<name>` is not configured correctly."         | file path                                                                                                  |
| `apiVersionUnsupported` | negotiated < minimum                                                                   | "Docker Engine API `<v>` is older than the supported `<min>`." | upgrade Engine                                                                                             |
| `notFound`              | HTTP 404                                                                               | "Container no longer exists."                                  | none (list refreshes)                                                                                      |
| `conflict`              | HTTP 409                                                                               | Docker's message                                               | none                                                                                                       |
| `timeout`               | `RequestTimeoutError`                                                                  | "Docker did not respond in time."                              | none                                                                                                       |
| `internal`              | anything else                                                                          | Docker or Bollard message                                      | none; full error logged via `tracing`                                                                      |

Classification walks `std::error::Error::source()` to find an `io::Error`, because
Bollard wraps transport errors. This must be confirmed with real failures (see §7)
and not only with constructed errors.

## 4. IPC types

Generated into `src/bindings/` (ADR 0001 §D8). Shapes:

```ts
type EndpointSource =
  | "dockerHostEnv"
  | "dockerContextEnv"
  | "configContext"
  | "default"
  | "rootlessFallback";

type EndpointInfo = { uri: string; source: EndpointSource; contextName: string | null };

type EngineInfo = {
  serverVersion: string; // "29.9.1"
  apiVersion: string; // negotiated, e.g. "1.53"
  serverApiVersion: string; // server's own max, e.g. "1.56"
  os: string;
  arch: string;
  kernelVersion: string;
  operatingSystem: string;
  rootless: boolean; // SecurityOptions contains "name=rootless"
  imageStore: "containerd" | "classic" | "unknown"; // DriverStatus driver-type
  containersRunning: number;
  containersStopped: number;
  containersPaused: number;
};

type ConnectionStatus =
  | { state: "connected"; endpoint: EndpointInfo; engine: EngineInfo }
  | { state: "disconnected"; endpoint: EndpointInfo | null; error: IpcError };

type ContainerState =
  | "created"
  | "running"
  | "paused"
  | "restarting"
  | "removing"
  | "exited"
  | "dead"
  | "unknown";

type PortMapping = {
  privatePort: number;
  publicPort: number | null;
  hostIp: string | null; // null = all interfaces
  protocol: "tcp" | "udp" | "sctp";
};

type ContainerSummary = {
  id: string; // full id
  name: string;
  image: string;
  state: ContainerState;
  status: string; // Docker's text, e.g. "Up 3 hours (healthy)"
  ports: PortMapping[];
  createdAt: number; // unix seconds
};
```

## 5. UI specification

### 5.1 Shell

- Left sidebar: Containers (active); Images, Compose, Volumes, Networks,
  Monitoring (disabled, tooltip "Coming soon").
- Sidebar footer: connection indicator.
  - Green dot "Connected" plus the Engine version. Tooltip shows the endpoint URI,
    source, negotiated/server API version, image store, and rootless flag.
  - Red dot "Disconnected" plus the error kind. Clicking it opens the
    connection panel.
- Connection status is polled every 10 s and on window focus (TanStack Query
  `refetchInterval`, `refetchOnWindowFocus`).

### 5.2 Containers page

- Toolbar: text filter (case-insensitive substring over name, image, and id
  prefix), state filter segmented control (All / Running / Stopped, where Stopped
  means not running, paused, or restarting), refresh button, and a count
  ("8 of 10").
- Table columns: Name, Image, State (badge), Status, Ports, Actions.
- Port display: `8080→80/tcp`, `127.0.0.1:8080→80/tcp`, unpublished `80/tcp`.
- Filter state lives in Zustand. It is not persisted in M0.

States:

| Condition              | Rendering                                                                              |
| ---------------------- | -------------------------------------------------------------------------------------- |
| Disconnected           | Connection panel: message, hint, endpoint, "Retry" button. The list query is disabled. |
| Loading (first fetch)  | Skeleton rows                                                                          |
| Error on list          | Inline error panel with message, hint, and "Retry"                                     |
| No containers          | Empty state: "No containers yet"                                                       |
| Filter matches nothing | "No containers match" plus a "Clear filters" action                                    |

### 5.3 Actions

| State                         | Start | Stop | Restart |
| ----------------------------- | ----- | ---- | ------- |
| `created`, `exited`           | ✓     |      |         |
| `running`                     |       | ✓    | ✓       |
| `paused`                      |       | ✓    |         |
| `restarting`                  |       | ✓    |         |
| `removing`, `dead`, `unknown` |       |      |         |

- This is a pure function `availableActions(state)`, unit-tested.
- While a mutation is pending for a row, all its action buttons are disabled
  and the triggered one shows a spinner. Other rows stay interactive.
- No confirmation dialog for start, stop, or restart in M0 (they are not
  destructive). Confirmation is required for destructive operations in later milestones.
- On settle (success **or** error), invalidate the `["containers"]` query. On a
  connection-level error, also invalidate `["connection"]`.
- Errors show as a toast with message and hint.

## 6. Acceptance criteria

| ID    | Criterion                                                                                                                                          | Verification                                                                                                                      |
| ----- | -------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| AC-1  | `pnpm tauri dev` opens the app on Linux under Wayland                                                                                              | manual, dev host (Wayland session)                                                                                                |
| AC-2  | App runs under X11                                                                                                                                 | manual with `GDK_BACKEND=x11` (XWayland). A native X11 session is **unverified** unless tested.                                   |
| AC-3  | Connection indicator shows Engine version, negotiated API version, endpoint, and source                                                            | manual + core integration test                                                                                                    |
| AC-4  | List shows running and stopped containers matching `docker ps -a` (names, images, states, ports)                                                   | manual comparison + integration test                                                                                              |
| AC-5  | Start, stop, and restart work, and the list reflects the new state without manual refresh                                                          | integration tests on Docktide-owned test containers + manual                                                                      |
| AC-6  | Unreachable daemon shows `daemonUnavailable` with a hint, without a crash                                                                          | `DOCKER_HOST=unix:///nonexistent.sock pnpm tauri dev`                                                                             |
| AC-7  | Permission problem shows `permissionDenied` with a hint, without a crash                                                                           | `DOCKER_HOST` pointing to a user-owned socket with mode `000` (see §7.3)                                                          |
| AC-8  | Daemon going away mid-session moves status to disconnected within about 10 s, and back to connected after recovery                                 | simulated by switching endpoints; stopping the real daemon is optional, needs maintainer consent, and is otherwise **unverified** |
| AC-9  | App runs as a normal user, never invokes `sudo` or `pkexec`, and requests no elevated privileges                                                   | code review + process check (`ps -o user`)                                                                                        |
| AC-10 | `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` pass                                    | command output                                                                                                                    |
| AC-11 | `pnpm exec vp check` (format, lint, type check on TypeScript 7) and `pnpm exec vp test run` pass; `pnpm exec tsc --noEmit` passes as a cross-check | command output                                                                                                                    |
| AC-12 | README documents prerequisites, setup, dev, test, and Docker-test commands                                                                         | review                                                                                                                            |
| AC-13 | A non-`unix://` `DOCKER_HOST` shows `unsupportedEndpoint`                                                                                          | `DOCKER_HOST=tcp://127.0.0.1:2375`                                                                                                |

Any criterion not verified in the implementation session is reported as **unverified**
with the reason.

## 7. Test plan

### 7.1 Unit tests (`cargo test`, no Docker)

- `endpoint::resolve`: each precedence step; `default` context name; missing
  context; malformed JSON; `DOCKER_CONFIG` override; non-unix scheme; rootless
  fallback; empty `DOCKER_HOST`.
- Context directory hashing: `sha256("desktop-linux")` matches the Docker CLI layout
  (fixture produced with the real CLI via `docker context create`).
- Container mapping: name stripping, missing names, unknown state, port dedup
  and normalization, sort order.
- `EngineInfo` mapping: rootless detection, image store detection, and missing fields.
- Error classification from constructed `io::Error` and `DockerResponseServerError` values.

### 7.2 Docker integration tests (`cargo test -- --ignored`)

- `#[ignore = "requires Docker"]`, in `crates/docktide-core/tests/docker.rs`.
- Each test creates its own container labelled `io.docktide.test=true` from a small
  image (proposed `busybox`, pulled if absent) and removes it in a drop guard.
- Covers ping and negotiate, list containing the test container, and start → stop →
  restart transitions with state assertions, plus `notFound` for a removed id.
- **Never** touches containers without the test label.

### 7.3 Failure simulations (safe on a shared host)

- Daemon unavailable: `DOCKER_HOST=unix:///tmp/docktide-nonexistent.sock`.
- Permission denied: bind a Unix socket in a scratch directory, `chmod 000` it,
  and point `DOCKER_HOST` at it. `connect()` then fails with `EACCES`.
- Connection refused: a socket file whose listener has exited.
- These run as ignored integration tests where practical, and otherwise manually.

### 7.4 Frontend (`vp test`, bundled Vitest)

- `availableActions`, filter predicate, port formatting, and `IpcError` → message/hint mapping.

## 8. Implementation plan

Each slice ends with a working, checked state. Stop for review after S1.

| Slice                        | Work                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      | Done when                                                                        |
| ---------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| **S1 Scaffold**              | `create-tauri-app` (React + TS, pnpm) into the repo; convert to a virtual workspace with `crates/docktide-core`; remove the opener plugin and demo code; replace Vite with Vite+ (`vite-plus` pinned exactly, pnpm `overrides` for `vite`/`vitest`, `lint`/`fmt`/`test` blocks in `vite.config.ts`, `vite-plus/client` types, `paths` without `baseUrl`); TypeScript `~7.0`; scripts `dev`/`build` → `vp dev`/`vp build` for Tauri; Tailwind 4 + `shadcn init` (confirm it works with TS 7 and no `baseUrl`); strict CSP; minimal capabilities; set the bundle identifier | Window opens on Wayland; `vp check`, `vp test run`, and `cargo clippy` are clean |
| **S2 Core: connection**      | `endpoint`, `error`, `connection`, `system` modules, test-first for pure parts                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            | Unit tests pass; ignored ping test passes against the dev host                   |
| **S3 Connection UI**         | `get_connection_status`, ts-rs bindings, `lib/ipc.ts`, QueryClient, shell, sidebar, indicator, connection panel                                                                                                                                                                                                                                                                                                                                                                                                                                                           | AC-3, AC-6, AC-7, AC-13                                                          |
| **S4 Container list**        | `containers::list` + mapping; `list_containers`; page, filter, states                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     | AC-4                                                                             |
| **S5 Lifecycle**             | start, stop, restart in core + integration tests; row actions, pending state, invalidation, toasts                                                                                                                                                                                                                                                                                                                                                                                                                                                                        | AC-5                                                                             |
| **S6 Docs and verification** | README setup; run the full AC checklist; record verified and unverified results in this spec                                                                                                                                                                                                                                                                                                                                                                                                                                                                              | AC-9 to AC-12                                                                    |

## 9. Known limitations accepted for M0

- No live updates: changes made outside Docktide appear on refresh or window focus.
- Connection status uses a 10 s ping poll (replaced by the event stream in M1).
- A single endpoint per app session; no context switcher.
- Long stop timeouts block that row's actions until Docker returns. Bollard's
  default request timeout is 120 s.

## 10. Decisions

Resolved on 2026-10-20 (recorded in ADR 0001):

1. Minimum Engine API version: 1.44 (Engine 25), a default for M0 (D12).
2. Rootless-socket fallback: kept (D12).
3. Type generation: `ts-rs` (D8).
4. Bundle identifier: `io.github.alvytsk.docktide` (D11).
5. License: `MIT OR Apache-2.0` (D10).

Still open:

1. Image for integration tests: `busybox` (proposed) or `alpine`. To be decided in S5.
