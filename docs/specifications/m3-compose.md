# M3: Compose

- Status: **Approved** 2026-10-25. Decisions in §10.
- Date: 2026-10-25
- Related: [Architecture overview](../architecture/overview.md), [ADR 0002](../architecture/decisions/0002-m1-navigation-logs-and-live-updates.md), [M1 spec](m1-live-details-logs.md), [M2 spec](m2-monitoring.md)

## 1. Goal

People who run Compose projects see each project's containers together in the
container list. They can also tail one merged log of the whole project, the
way `docker compose logs -f` does, mainly to catch errors across services
without opening each container's Logs tab.

## 2. Scope

In scope:

1. Compose membership on every container, read from the labels `docker compose`
   sets (FR-1).
2. The container list grouped by project: a collapsible header row per project
   with a summary and a **Logs** button, followed by standalone containers (§5.1).
3. A project logs page: one stream merged in time order from every container
   in the project, with a service tag on each line and a chip per service to
   hide or show it (FR-2, §5.2).

Non-goals (explicitly deferred):

- Starting, stopping, or restarting a whole project or service. Each
  container in a group keeps its own buttons. Doing this properly needs
  `depends_on` ordering, which is separate work (§10, D3).
- Reading `compose.yaml`, running `docker compose up`/`down`, or showing
  projects that have no containers.
- An "errors only" switch or log-level detection. The text filter covers
  this (confirmed by the user).
- A project page with tabs other than logs (overview, stats). A link from a
  container's details page to its project.
- Remembering collapsed groups across restarts of Docktide.

Anti-goals: anything that makes the list slower or noisier for people who do
not use Compose. Without Compose containers, the list looks and behaves exactly
as in M2.

## 3. Functional requirements

### FR-1 Compose membership

`docker compose` labels every container it creates. Seen on Compose 5.5.1
(2026-10-25):

| Label                                 | Use                                 |
| ------------------------------------- | ----------------------------------- |
| `com.docker.compose.project`          | project name; the grouping key      |
| `com.docker.compose.service`          | service name                        |
| `com.docker.compose.container-number` | replica number, `1` when not scaled |
| `com.docker.compose.oneoff`           | `True` for `docker compose run`     |

- The list API already returns labels, so no extra Docker request is needed.
  `ContainerSummary` gains `compose: ComposeInfo | null` (§4).
- A container is in a project when it has a non-empty `project` label. It is
  `null` otherwise, including when `project` is set but `service` is missing.
- `number` is the parsed `container-number`, or `null` when it is absent or not
  a number. `oneOff` is true only for the value `True` (case-insensitive).
- A project exists while at least one of its containers exists. Its name is
  the label value. Project names are unique per Engine (Compose enforces this),
  so the name alone identifies a project.

### FR-2 Project logs

The project logs page follows every container whose `compose.project` is
the page's project, using the existing `open_logs` / `close_logs` per
container (§10, D2). There is no new IPC.

- **Membership** comes from the containers query (TanStack Query, refreshed
  by M1's live updates). A container that joins the project while the page is
  open, for example after `docker compose up`, gets a stream. A container that
  leaves (removed) has its stream closed. Its lines already shown stay.
- **Opening:** each container's stream opens with the page's tail setting
  (default 1,000), per container, as `docker compose logs --tail`. A container
  already seen in this session opens with `after` = its own last timestamp
  instead (resume without duplicates, as M1 FR-4).
- Streams open for stopped containers too, so a crashed service's last lines
  appear. Docker sends their tail and then ends the stream.
- **Restart:** when a container's stream has ended and a containers query
  result fetched after that `ended` shows it `running`, its stream reopens
  with `after`. A container reopens at most once per such result, so stale data
  cannot cause a reopen loop. Every `ended` invalidates the containers query,
  so such a result always arrives, even when a quick restart's events landed
  before the `ended`.
- **Disconnect:** a connection-level error on any stream marks the page
  disconnected and invalidates the connection query, as the Logs tab does. On
  `connected`, every stream reopens with `after`.
- **Other errors** affect only that container's stream (§5.2 states).
- **Merge order:** every line is placed by its timestamp, across and within
  containers. Arrival order is kept only between lines with equal timestamps.
  Timestamps are compared as instants (Docker trims trailing zeros from the
  fraction, so the raw strings do not sort). Docker can deliver one
  container's stdout and stderr out of order (M1 §9), and a late batch such as
  a newly opened container's tail arrives after newer lines; both are placed by
  timestamp, not appended. A container's resume point is the newest timestamp
  seen from it, not the last one to arrive.
- **Buffer:** the latest 10,000 lines across the project, as the Logs tab
  (M1 §5.4). The "Older lines are no longer shown" notice applies.
- **Session:** the lines, per-container resume points, tail, and hidden
  services of the most recently viewed project live outside React (like M1's
  `LogSession`), so leaving and returning resumes instead of refetching.
  Changing the tail starts over. Opening another project replaces the session.
- **Rate:** each container's stream is batched in Rust as today (M1 FR-4), so
  N containers can deliver N times M1's 10,000 lines/s cap. Rendering stays at
  one update per animation frame, but parsing and merging run per batch, so
  the page must stay responsive under a flood from several containers at once
  (AC-12).

## 4. IPC types

Changed, regenerated into `src/bindings/`:

```ts
type ContainerSummary = {
  // ...unchanged fields
  compose: ComposeInfo | null;
};

type ComposeInfo = {
  project: string;
  service: string;
  number: number | null; // replica, from container-number
  oneOff: boolean; // created by `docker compose run`
};
```

`open_logs`, `close_logs`, `LogEvent`, and `LogLine` are unchanged.

## 5. UI specification

### 5.1 Container list

Layout (mockup A in the brainstorm, chosen by the user):

- Projects come first, sorted by name (case-insensitive). Standalone
  containers follow, in M2's order (running first, then by name).
- Each project is a **header row** spanning the table:
  - Name column: a disclosure chevron, the project name, and
    `N services` in muted text (distinct service names).
  - State column: `R/T running`, where R counts running containers and T counts
    all of them. Green style when all are running, muted when none are, and
    amber in between.
  - CPU and Memory columns: sums over the containers' current samples (M2
    FR-4). `—` when no container has a value.
  - Actions column: a **Logs** button linking to the project logs page.
- Container rows of a project follow its header, indented, ordered by service
  name, then replica number, with one-off containers last. A container row is
  otherwise the M2 row unchanged, with its own actions and link.
- Clicking the header (or pressing Enter or Space on it) toggles collapse. The
  chevron has `aria-expanded`. Groups start expanded. Collapsed state is kept
  per project name for the session, in Zustand (session only, like the
  details tab).
- **Filters:** the text filter matches a container's name, image, or id as
  today, **or its project name**. The state filter applies to each container.
  A group is shown when at least one of its containers matches, and then shows
  only the matching ones. Header counts and sums always cover the whole
  project, not only the filtered rows. "N of M" still counts containers.
- Skeleton, empty, and error states are unchanged.

### 5.2 Project logs page

Route `/projects/$project/logs` (project name URL-encoded). The page title is
the project name. Next to it are `R/T running` and a "Containers" back link.
The sidebar's Containers entry stays highlighted.

- **Toolbar**, as on the Logs tab (M1 §5.4): filter, timestamps, wrap, clear,
  copy, tail (100, 1,000, 10,000, All; per container), Live indicator, and
  "Jump to latest" while scrolled up. Filter and copy work on the line's
  plain text. Copied lines are prefixed with their tag, as `api | text`.
- **Service chips**, one per service that has a container in the project or
  has lines in the view, sorted by name, each in its service colour. A
  service whose containers were all removed keeps its chip while its lines
  remain, so hidden lines can always be shown again. Clicking a chip hides or shows that service's lines (all
  replicas). It only affects the view: streams keep running, so showing a
  service again shows its lines. A chip has `aria-pressed`. "N of M lines"
  counts after both chips and filter.
- **Each line** has a tag column before the text: the service name, or
  `service-N` when that service currently has more than one container, or
  the container name for one-off containers. Tags are padded to the longest
  tag, end with `|`, and use the service colour. stderr keeps the red left
  border and the accessible "stderr" label. Timestamps (when on) come before
  the tag.
- **Service colours:** a stable hash of the service name picks one of the ANSI
  theme colours other than red, which stays reserved for stderr: green, yellow,
  blue, magenta, cyan, and their bright variants. They are readable in both
  themes. Two services may share a colour.
- **Gap markers** name their container: "N lines skipped from api (output too fast)".

States:

| Condition                                       | Rendering                                                                       |
| ----------------------------------------------- | ------------------------------------------------------------------------------- |
| Opening, no lines yet                           | Spinner in place of the lines                                                   |
| At least one stream live                        | Lines; "Live" indicator                                                         |
| All streams ended, no container running         | Lines kept; footer "No containers running. Logs resume when one starts."        |
| A stream failed (not connection-level)          | Lines kept; footer "Logs for `<tag>` failed: `<message>`" with Retry (that one) |
| Disconnected                                    | Lines kept; footer "Docker is not reachable." with Retry; reopens on connected  |
| No lines at all                                 | "No log output yet"                                                             |
| Project's containers all removed, lines remain  | Lines kept; footer "No containers in project `<name>`." with a Containers link  |
| Project has no containers and no lines          | "No containers in project `<name>`." with a link back to Containers             |
| Docker disconnected before the page loaded data | The connection panel, as on the Containers page                                 |

## 6. Acceptance criteria

| ID    | Criterion                                                                                                                                                                                                                                                       | Verification                                     |
| ----- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------ |
| AC-1  | A Compose project with three services shows as one group with the right `R/T running`, service count, and summed CPU and memory; standalone containers follow it                                                                                                | manual (test compose project)                    |
| AC-2  | Without Compose containers, the list looks and behaves as in M2                                                                                                                                                                                                 | manual                                           |
| AC-3  | `docker compose up -d` and `down` of a project make its group appear and disappear within about 1 s                                                                                                                                                             | manual                                           |
| AC-4  | Collapse, filters (including by project name), and the state filter behave as in §5.1                                                                                                                                                                           | component/unit tests + manual                    |
| AC-5  | Project logs show the tail of every container in time order with service tags, then new lines from all of them as they arrive                                                                                                                                   | integration test on merge + manual               |
| AC-6  | A service chip hides and shows that service's lines without reopening streams                                                                                                                                                                                   | unit test + manual                               |
| AC-7  | `docker compose restart api` while the page is open resumes api's logs without duplicates                                                                                                                                                                       | manual                                           |
| AC-8  | `docker compose up -d --scale worker=2` while the page is open adds `worker-2` to the stream                                                                                                                                                                    | manual                                           |
| AC-9  | Leaving the page and returning resumes without refetching or duplicating lines                                                                                                                                                                                  | unit test on the session + manual                |
| AC-10 | Leaving the page or reloading closes every log connection the page opened                                                                                                                                                                                       | `ss -x` or fake Engine connection count          |
| AC-11 | Disconnect and reconnect: the page shows "Docker is not reachable." and resumes after "Connected" without duplicates                                                                                                                                            | simulated as in M1 AC-3                          |
| AC-12 | Three containers each writing lines as fast as they can for 60 s, with the page open: filter, scroll, Jump to latest, and navigation stay responsive; the view holds at most 10,000 entries with gap markers; leaving the page closes all three log connections | manual + `ss -x` or fake Engine connection count |
| AC-13 | Every M2 acceptance criterion still holds                                                                                                                                                                                                                       | re-run of the M2 checklist                       |
| AC-14 | `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `pnpm check`, `pnpm test`, and `pnpm typecheck` pass                                                                                                    | command output                                   |
| AC-15 | README and the architecture overview describe Compose support as built                                                                                                                                                                                          | review                                           |

Any criterion not verified in the implementation session is reported as
**unverified** with the reason.

## 7. Test plan

### 7.1 Unit tests (`cargo test`, no Docker)

- `ComposeInfo` from labels: full set; no labels (`null`); project without
  service (`null`); non-numeric `container-number` (`null` number); `oneoff`
  `True`, `true`, `False`, absent.
- Serialization of `ContainerSummary` with and without `compose`.

### 7.2 Docker integration tests (`cargo test -- --ignored`)

Same rules as M1 §7.2: only containers labelled `io.docktide.test=true`.

- A test container created with Compose labels lists with the matching
  `compose`. Compose itself is not required; the labels are set directly.

### 7.3 Frontend (`vp test`)

- Grouping: projects first and sorted, standalone after in M2 order; row
  order within a group; header counts and sums cover the whole project under
  a filter; a filter matching the project name shows the whole group; a
  group with no matching containers is hidden.
- Merge buffer: interleaving by instant across containers with differently
  trimmed fractions; a late tail merges into place; equal timestamps keep
  arrival order; one container delivering timestamps 3 then 1 while another
  delivers 2 shows 1, 2, 3; the resume point is the newest timestamp, not the
  last arrived; capacity trims the oldest across all containers; per-container
  last timestamp survives `clear`.
- Tags: `service`, `service-N` only while scaled, container name for one-off.
- Chips hide by service; counts after chips and filter; a hidden service
  whose last container is removed keeps its chip while its lines remain.
- Session reopen rule: an ended container reopens once per newer running
  query result, not for a result fetched before `ended`.
- Service colour is stable for a name and never red.

## 8. Implementation plan

Each slice ends with a working, checked state. Stop for review after S2.

| Slice                        | Work                                                                                                     | Done when       |
| ---------------------------- | -------------------------------------------------------------------------------------------------------- | --------------- |
| **S1 Core: membership**      | `ComposeInfo` from labels, `ContainerSummary.compose`, bindings, tests                                   | §7.1, §7.2 pass |
| **S2 Grouped list**          | Grouping and filters, header row with summary and collapse, Zustand collapse store                       | AC-1 to AC-4    |
| **S3 Project logs**          | Route and page; multi-container session with merge buffer, membership, reopen rules; tags, chips, states | AC-5 to AC-12   |
| **S4 Docs and verification** | README, overview (§4, §5, §10), ADR 0004 for §10; run the AC checklist and record results here           | AC-13 to AC-15  |

## 9. Known limitations accepted for M3

- One Engine log connection per container in the open project, as
  `docker compose logs -f`.
- Lines from different containers arriving within the same instant may show
  in either order. Ordering relies on each container's clock stamp from Docker,
  which is one host's clock.
- Containers of a project that was renamed or recreated under a different
  project name show as a different project.
- Collapsed groups reset when Docktide restarts.
- A container whose first line has an empty timestamp sorts by another
  container's last key until it has a timestamp of its own, so that line can
  land after its own later lines.

## 10. Decisions

Resolved on 2026-10-25; to be recorded in ADR 0004 (S4).

1. **D1 Layout:** groups inside the containers list (mockup A), not a
   separate Projects page. Chosen by the user.
2. **D2 Merging in the frontend:** the project logs page opens one existing
   `open_logs` stream per container and merges them in the frontend buffer.
   The alternative was a Rust supervisor that merges and follows lifecycle
   events, as the M2 stats supervisor does. It was rejected for M3 because it
   duplicates M1's resume and batching logic per container, and the
   frontend already knows membership from the containers query. Revisit this
   if restart detection from the list proves unreliable.
3. **D3 No project lifecycle:** Start, Stop, and Restart for a whole project
   are deferred. The user wanted grouping and logs, and doing lifecycle
   properly needs `depends_on` ordering.
4. **D4 Project identity:** the `com.docker.compose.project` label alone. The
   working directory and config files labels are ignored.

Implementation calls (not separately approved):

- The tail applies per container, like `docker compose logs --tail`.
- "Running" in `R/T running`, the header colour, the "No containers running"
  footer, and the reopen rule means `state === "running"` only. The list's
  Running filter also includes paused and restarting, so a filtered group can
  show more rows than its running count.
- The sidebar's disabled "Compose (Coming soon)" entry is removed, since
  projects live in the containers list (D1).
- Hiding a service is a view filter, not a stream close, so showing it again is
  instant and loses nothing.
- Service colours come from the existing ANSI theme colours, without red.
- Collapse state is session-only, matching the other UI stores.

## 11. Verification

Run on 2026-10-25 on the dev host: Ubuntu 26.04.1, kernel 7.0.0-38-generic,
niri (Wayland), Docker Engine 29.9.2 (rootful, default socket, cgroup v2), Rust
1.99.1, Node 24.19.0, pnpm 11.28.6. The GUI could not be seen or driven from the
implementation session, and the app was not launched; checks that need the
running app are marked **Unverified, pending manual check** unless the user did
them or a test covers them. Only containers labelled `io.docktide.test=true`
were used by the tests.

| ID    | Result                                                                         | Evidence                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| ----- | ------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| AC-1  | Verified by the user (manual, 2026-10-25)                                      | The user confirmed the grouped list looked right in the running app at the S2 stop ("looks good").                                                                                                                                                                                                                                                                                                                                             |
| AC-2  | Verified by the user (manual, 2026-10-25)                                      | Same S2 check; standalone containers follow the groups. Grouping tests: `projects first, sorted case-insensitively; standalone after in input order`.                                                                                                                                                                                                                                                                                          |
| AC-3  | Verified by the user (manual, 2026-10-25)                                      | Same S2 check. Appearance and removal follow M1's invalidation-on-event path, unchanged.                                                                                                                                                                                                                                                                                                                                                       |
| AC-4  | Verified by the user (manual, 2026-10-25); unit tests                          | `counts cover the whole project under a filter`, `a filter matching the project name shows the whole group`, `a group with no matching members is left out`, `text matches the compose project name`, `running includes paused and restarting; stopped is everything else`, `text and state filters combine`. No component test for the collapse toggle (see Deviations).                                                                      |
| AC-5  | Verified by the user (manual, 2026-10-25)                                      | The user ran the project logs checklist against a three-service test project and reported "works". Merge: `interleaves sources by instant despite trimmed fractions`, `a late tail merges into place`, `equal timestamps keep arrival order`; session: `opens one stream per member with the tail, then resumes with after`; tags: `plain service tag unless the service is scaled`. `list_reports_compose_labels` (Docker) covers membership. |
| AC-6  | Verified by the user (manual, 2026-10-25)                                      | `toggleService hides and shows without reopening`, `hidden sources are filtered, with their gaps`. The user ran the project logs checklist against a three-service test project and reported "works".                                                                                                                                                                                                                                          |
| AC-7  | Verified by the user (manual, 2026-10-25)                                      | `an ended container reopens once per newer running result`, `an exited result never reopens`, `resume point is the newest timestamp per source, not the last arrived, and survives clear`. The user ran the project logs checklist against a three-service test project and reported "works". (`docker compose restart api`).                                                                                                                  |
| AC-8  | Verified by the user (manual, 2026-10-25)                                      | `a new member opens with the tail`, `plain service tag unless the service is scaled`. The user ran the project logs checklist against a three-service test project and reported "works". (`--scale worker=2`).                                                                                                                                                                                                                                 |
| AC-9  | Verified by the user (manual, 2026-10-25)                                      | `projectSession keeps one session for the most recent project`. The user ran the project logs checklist against a three-service test project and reported "works".                                                                                                                                                                                                                                                                             |
| AC-10 | **Unverified, pending manual check** (`ss -x` on leave and reload)             | `a stream that resolves after close is closed at once`, `a member that leaves is closed; its lines and source info stay`, and the M1 fake-Engine close tests, which still pass. The page uses the same `open_logs`/`close_logs` mechanism as the Logs tab (M1 AC-11).                                                                                                                                                                          |
| AC-11 | **Unverified, pending manual check**                                           | `ended calls onEnded; a connection-level error calls onConnectionLost and marks that stream disconnected`, `another error marks only that stream`. Not simulated in the app. See the AC-11 reading under Deviations.                                                                                                                                                                                                                           |
| AC-12 | **Unverified, pending manual check** (60 s load, `ss -x`, scroll while paused) | `capacity trims the oldest across all sources` and `drops the oldest beyond capacity and remembers that it did`. No load test was run and responsiveness was not judged.                                                                                                                                                                                                                                                                       |
| AC-13 | Partly verified by the user (manual, 2026-10-25)                               | The container Logs tab looked and behaved as before in the user's checklist pass ("works"). The rest of the M2 checklist was not re-run by hand; all M2 tests still pass (gate below). Outstanding: M2 AC-2, AC-4 to AC-8.                                                                                                                                                                                                                     |
| AC-14 | Verified                                                                       | `cargo fmt --all --check` and `cargo clippy --workspace --all-targets -- -D warnings` clean. `cargo test --workspace`: 136 unit + 31 fake-Engine tests passed, 14 ignored. `cargo test --workspace -- --ignored`: 14 passed. `pnpm check`: 100 files formatted, 0 warnings or errors in 78. `pnpm test`: 16 files, 126 tests passed. `pnpm typecheck` passes.                                                                                  |
| AC-15 | Verified, pending review                                                       | README (status, Features, spec link), overview (§4, §5, §10), and ADR 0004 describe Compose as built.                                                                                                                                                                                                                                                                                                                                          |

Deviations from the spec and plan made during implementation:

- The merge buffer orders lines by timestamp, and the container Logs tab uses
  the same buffer, so its stdout and stderr lines now sort by timestamp rather
  than arrival. Lines with equal timestamps keep arrival order.
- The header reads "1 service" for one service, not "1 services".
- Replica tags count distinct replica numbers, not containers, so the old and
  new `api` during a force-recreate keep the plain `api` tag and old lines are
  not retagged.
- The project page's link reads "Back to containers", as the existing
  not-found pages do, rather than "a Containers link".
- There are no component or hook tests for the project logs page or the
  grouped table: the repo has no DOM test environment and adding one needs a
  dependency. Session, buffer, tag, and grouping logic is unit tested.
- AC-11 reading: on a disconnect the connection panel replaces the view, as on
  M1's Logs tab. The "Docker is not reachable." footer covers the window before
  the connection query flips.
- `LogView` shows "N of M lines" whenever the filter is non-empty or any
  service is hidden; N counts lines after both.
- The session does not use `compareMembers`; its service list is sorted by name.

Observations, not fixed in M3:

- A mid-buffer insert while the view is paused may move the scroll position;
  to check in the AC-12 pass.
- A late batch that sorts mid-buffer copies the whole buffer; merging only the
  tail is the upgrade if AC-12 janks.
- The first visit can flash "No log output yet" for a frame before "Opening
  logs…".
- Removed containers keep the tag from their last sync (a service later scaled
  shows its old plain tag on those lines).
- The project page's footer priority logic (`overallStatus` and `StatusFooter`
  in `project-logs-page.tsx`) is pure but untested. `Footer` lives in
  `log-view.tsx`; `RetryButton` is defined in `project-logs-page.tsx`.
- A stream that fails with a connection-level error while the connection check
  still reports connected stays "disconnected" until the user clicks Retry, as
  in M1's Logs tab.
- Service names compare case-sensitively while project names are lowercased;
  Compose allows only lowercase project names, so this is moot in practice.
