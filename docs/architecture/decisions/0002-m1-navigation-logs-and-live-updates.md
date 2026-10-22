# ADR 0002: Navigation, logs, and live updates (M1)

- Status: **Accepted**
- Date: 2026-10-22 (decisions taken 2026-10-21)
- Deciders: project maintainer
- Related: [M1 spec](../../specifications/m1-live-details-logs.md) §10, [ADR 0001](0001-initial-architecture.md)

## Context

M1 adds the first long-running streams (Docker events and container logs) and
the first second-level page (container details). Each needed a choice that later
milestones (stats, exec, Compose, images) will inherit: how pages are addressed,
how a fast log is rendered, what happens to terminal escape sequences, how much
of a container's environment is shown, and whether the M0 status poll stays.

Facts established while deciding (2026-10-21) and implementing (2026-10-22):

| Area                  | Finding                                                                                                                                                                                                               |
| --------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Bollard 0.21.1        | `events()` and `logs()` return streams; the request timeout applies only to receiving headers, so streams outlive it. `/events` yields nothing until the first event, so a ping, not the stream, proves connectivity. |
| Engine 29 `json-file` | With `timestamps=1`, every TTY line carries its own timestamp. A non-TTY message over 16 KiB arrives as two frames with the same timestamp. Within a burst, stdout and stderr are not in timestamp order.             |
| Tauri 2.12            | A `Channel` send never blocks and does not report a reloaded or closed page to Rust. Hash navigation does not fire `PageLoadEvent::Started`.                                                                          |
| WebKitGTK             | No built-in Back/Forward shortcuts (those belong to browser chrome).                                                                                                                                                  |
| TanStack Router 1.170 | Code-based routes work without the Vite plugin; with hash history the bundled app needs no server routing.                                                                                                            |

## Decisions

### D1: TanStack Router with code-based routes and hash history

Routes are declared in `src/app/router.tsx` and addressed as
`#/containers/<id>/<tab>`. Chosen over a small in-app hash router for
type-checked paths and params, which images, Compose, volumes, and networks
will need. No route loaders: pages load data through TanStack Query so the
connection gating stays in one place. The shell handles Alt+←/→ and the mouse
side buttons, since WebKitGTK does not.

### D2: `@tanstack/react-virtual` for the log view

Only visible rows are rendered, with measured heights when lines wrap. The view
re-renders at most once per animation frame however many batches arrive.

### D3: Render ANSI colours and text attributes in logs

Rust keeps SGR sequences (colours, bold, italic, underline, inverse, strike)
and strips every other escape sequence and control character. The frontend
parses SGR into styled React elements, never HTML. The 16 standard colours are
theme tokens (`--ansi-0` … `--ansi-15`) chosen to be readable in light and dark
themes; 256-colour and 24-bit colours are used as given. Filter and copy work on
the plain text.

### D4: Environment values hidden on Overview, shown in Inspect

Overview lists variable names with values masked until "Show values" is
pressed, and forgets that choice. Inspect shows the raw JSON unmasked, as
`docker inspect` does; it is the deliberate escape hatch.

### D5: No connection poll

The 10 s ping from M0 is removed. Connection status follows the events stream
(`connected` / `disconnected` hints) plus a re-check on window focus. An idle,
connected app makes no Docker requests.

### D6: Logs open with the last 1,000 lines; the view holds 10,000

The tail is selectable (100, 1,000, 10,000, All). Rust delivers at most 500
lines per batch and one batch per 50 ms (10,000 lines/s); beyond that a
10,000-line queue drops the oldest and reports a gap. The view drops its oldest
lines past 10,000 and says so.

### D7: One events stream, invalidation hints only (implementation call)

`docktide-core` runs one supervised events stream per app, filters it to an
allowlist of container actions, and coalesces ids for 250 ms. The frontend
refetches through TanStack Query; Rust keeps no mirror of Docker state
(ADR 0001 D5 holds).

### D8: Log resume by timestamp (implementation call)

A reopened log stream (restart, reconnect, remount, Retry) asks Docker for
`since` = the last shown line's second and drops lines at or before that
timestamp in Rust. The view's lines live in a session outside React that
survives the tab unmounting, so returning to a container's logs continues
rather than refetching the tail.

## Consequences

- Every new page gets a typed route; links are checked at compile time.
- Streams follow one pattern: a `CancellationToken` in the `DockerService`
  registry, a `Channel` per subscription, cleanup on page load and window
  destruction, and pacing in Rust before data crosses the bridge. Stats (M2)
  and exec (M6) reuse it.
- Full-screen TTY programs that redraw in place render imperfectly (no cursor
  movement); colours do not carry across lines.
- A resume can lose lines only when a stream breaks in the middle of a mixed
  stdout/stderr burst (spec M1 §9).
- Hiding env values on Overview is a convenience against shoulder-surfing, not a
  security boundary: Inspect and the Docker CLI show them.
