# ADR 0004: Compose (M3)

- Status: **Accepted**
- Date: 2026-10-25 (decisions taken 2026-10-25)
- Deciders: project maintainer
- Related: [M3 spec](../../specifications/m3-compose.md) §10, [ADR 0001](0001-initial-architecture.md), [ADR 0002](0002-m1-navigation-logs-and-live-updates.md), [ADR 0003](0003-m2-monitoring.md)

## Context

M3 makes Compose projects visible: containers grouped by project in the list,
and one logs page per project. It needed choices about where projects live in
the UI, where project logs are merged, whether to manage a project's
lifecycle, and what identifies a project.

Facts established while deciding (Compose 5.5.1, Engine 29.9.2):

| Area                          | Finding                                                                                                                                                     |
| ----------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Container labels              | `docker compose` sets `com.docker.compose.project`, `.service`, `.container-number`, and `.oneoff` on every container. The list API already returns labels. |
| Project names                 | Unique per Engine, so the name alone identifies a project.                                                                                                  |
| `docker compose restart`      | Restarts the same container; a log stream ends and the container is running again shortly after.                                                            |
| `up -d` after a config change | Recreates the container: for a moment the old and the new `api` (both number 1) exist together.                                                             |

## Decisions

### D1: Groups inside the containers list

Projects are groups of rows in the existing list (header row with running
count, service count, summed CPU and memory; collapsible), not a separate
Projects page. Standalone containers follow the groups. One list keeps one
filter and one set of row actions.

### D2: Project logs are merged in the frontend

The project logs page opens one existing `open_logs` stream per container and
merges them in the frontend buffer, ordered by each line's timestamp. The
alternative was a Rust supervisor that merges and follows lifecycle events, as
the M2 stats supervisor does. It was rejected because it would duplicate M1's
resume and batching logic per container, and the frontend already knows
membership from the containers query. Revisit this if restart detection from
the list proves unreliable.

### D3: No project lifecycle in M3

Start, Stop, and Restart for a whole project are deferred. Doing it properly
needs `depends_on` ordering. Grouping and logs were what was wanted.

### D4: Project identity is the project label

The `com.docker.compose.project` label alone. The working directory and config
file labels are ignored, so a project recreated under another name shows as a
different project.

### Implementation calls (not separately approved)

- The tail applies per container, like `docker compose logs --tail`.
- "Running" means `state === "running"` only (header colour, running count,
  reopen rule). The list's Running filter also includes paused and restarting.
- The sidebar's disabled "Compose" entry is removed; projects live in the list.
- Hiding a service is a view filter, not a stream close.
- The log buffer orders lines by timestamp (key padded to nanoseconds, arrival
  order for ties). The container Logs tab uses the same buffer, so its stdout
  and stderr lines now also sort by timestamp.
- Replica tags are `service`, or `service-N` while more than one distinct
  replica number exists. Counting numbers, not containers, keeps the plain tag
  through a force-recreate window.
- Reopen rule: an ended container is reopened once per newer running query
  result, so `docker compose restart` resumes without duplicates using the
  per-container resume point.
- Collapse state is a session-only Zustand store.

## Consequences

- One Engine log connection per container in the open project, as
  `docker compose logs -f`. A project with many containers costs that many.
- The page depends on the containers query for membership and restarts; a stale
  list delays a new container's logs by up to the invalidation delay (about 1 s).
- A late or out-of-order batch re-sorts into place, copying the buffer; if that
  janks at the 10,000-entry cap, merge only the tail.
- Ordering relies on Docker's per-container timestamps from one host clock;
  lines in the same instant may swap.
- No component tests cover the page: the repo has no DOM test environment and
  adding one needs a dependency. Session, buffer, tag, and grouping logic are
  unit tested; the page is checked by hand.
- Project-wide actions would need a Rust-side or compose-aware layer (D3) and a
  decision on ordering, left for a later milestone.
