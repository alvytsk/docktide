# M4: Images

- Status: **Implemented** 2026-10-29. Decisions in §10, verification in §11.
- Date: 2026-10-29
- Related: [Architecture overview](../architecture/overview.md) (§6.3 image store neutrality), [M1 spec](m1-live-details-logs.md), [M3 spec](m3-compose.md)

## 1. Goal

Free disk space. The user opens Images mostly to clean up unused images
(their words) and wants to see **which images are unused and how big they
are**. The page answers "what can I delete, and how much will I get back?"
and then deletes it.

On the dev host on 2026-10-29, images took about 700 MB while the **build
cache took 6.4 GB**, so build cache size and cleanup are part of M4 (§10 D2).

## 2. Scope

In scope:

1. An Images page: one row per image with name, size, age, and usage
   (FR-1, FR-2, §5.1).
2. Removing one image, several selected images, or all unused images in one
   step (FR-3).
3. Build cache: its size on the Images page and a button to clear it (FR-4).
4. An image details page: overview and raw inspect JSON (FR-5, §5.2).
5. Live updates for images: list refreshes after `docker pull`, `docker rmi`,
   `docker build` (FR-6).

Non-goals (deferred to a later milestone):

- Pulling images, creating a container from an image, tagging, pushing,
  building, import/export. None of them help with cleanup.
- Layer history (`docker history`), vulnerability scanning, registry search.
- Force-removing images (`docker rmi -f`) and removing containers from the
  Images page (§10 D3).
- Volume and container pruning (`docker system prune`). Volumes are M5.

Anti-goals: removing anything the user did not clearly choose. The page must
never delete an image a container still uses, and every multi-image or cache
removal shows what it will delete and asks first.

## 3. Functional requirements

### FR-1 Image list

`list_images` returns the Engine's top-level images (`GET /images/json`,
`all=false`, `shared-size=true`). Seen on Engine 29.9.2 with the containerd
image store (2026-10-29): `Size` is the total, `SharedSize` is the part shared
with other images, and `Containers` is always `-1` on this endpoint.

- **Unique size** = `size - sharedSize`: what removing this image alone gives
  back. When `sharedSize` is `-1` or missing (overview §6.3: stores differ),
  `sharedSize` is `null` and the unique size shows as `—`.
- **Name**: each entry of `RepoTags`. An image with no tags (or only
  `<none>:<none>`) is **dangling** and shows as `<none>` with its short id.
  An image with several tags is **one row** listing all tags.
- The list is not cached in Rust (overview §4).

### FR-2 Usage

Usage is computed in the frontend from the containers query (no extra Docker
request), matching each container's `imageId` to the image's `id`. Both are the full
`sha256:` id; checked equal on the containerd store on 2026-10-29.
`ContainerSummary` gains `imageId: string` (Bollard `image_id`).

| Usage           | Rule                                                          |
| --------------- | ------------------------------------------------------------- |
| **In use**      | at least one container that is running, paused, or restarting |
| **Stopped (N)** | only containers in other states; N = their count              |
| **Unused**      | no container at all                                           |

"Unused" means no container's `ImageID` is this image's id, whichever tag
the container was created from.

### FR-3 Removing images

- **One image** (`remove_image`): removes without force. For an image with
  several tags, Docker refuses removal by id ("referenced in multiple
  repositories"), so core removes each tag in turn; removing the last tag
  removes the image. A dangling image is removed by id. The builder verifies
  this sequence on the containerd store in an integration test; if removal by
  id works there for multi-tagged images, the simpler call wins.
- **No partial untagging:** before removing tags, core lists all containers
  (`all=true`, no filter) and checks for one whose `ImageID` equals the image
  id exactly, the same rule as FR-2. If one exists, it returns `conflict`
  without touching a tag. Not the `ancestor=` filter: it also matches
  containers of descendant images, so a base image shown as Unused would
  always fail the guard. Other conflicts are left to Docker. A failure after the first tag (a race) leaves the remaining
  tags; the error toast says "Removed tags a, b; <tag>: <message>".
- **Images with containers** (In use or Stopped): the Remove action is
  disabled and its tooltip names up to three containers ("Used by api, worker
  and 1 more"). If the Engine still answers `409` (a container appeared in the
  meantime), the error toast shows the Engine's message.
- **Several selected images**: rows have checkboxes; only unused images can be
  selected. "Remove N images" confirms with the list of names and an
  approximate total of unique sizes, then removes them one by one. Failures
  do not stop the rest; one toast summarises "Removed 3 of 4. <name>: <message>".
- **Remove all unused**: the confirm dialog lists the images marked Unused
  with their count and approximate unique size. On confirm, exactly those
  image ids go through the guarded single-image path above, one by one, as
  for a selection. The Engine's prune is **not** used: on the containerd store
  it works per reference, so it can untag `app:b` of an image whose `app:a` a
  container uses (moby `daemon/containerd/image_prune.go`), and it would also
  remove images that became unused after the dialog opened. The success toast
  reports "Removed N images, about Y freed" (sum of unique sizes).
- Approximate totals are labelled "about": unique sizes undercount layers
  shared only among the removed images.
- An image already gone (`404`) counts as removed.

### FR-4 Build cache

- `get_build_cache_usage` returns `BuildCacheUsage.TotalSize` and
  `Reclaimable` from `GET /system/df?type=build-cache` (Bollard
  `build_cache_usage`, without `verbose`). Seen on Engine 29.9.2 / API 1.56
  (2026-10-29): 6,402,404,974 total, 5,713,938,493 reclaimable. No summing of
  records: API 1.53 removed the legacy `BuildCache` array.
- **Older Engines** (API 1.44 to 1.52, which send only the legacy array that
  Bollard 0.21 does not model): `size` and `reclaimable` are `null`. The strip
  shows "Build cache: size not reported by this Engine" and keeps the Clear
  button, so an older Engine never hides existing cache. Summing the legacy
  array is deferred until someone runs an older Engine.
- `prune_build_cache` clears all unused build cache (`POST /build/prune`,
  `all=true`, as `docker builder prune -a`) and returns the space reclaimed.
- There are no build cache events. The figure refetches when the Images page
  mounts, on window focus (TanStack default), and after a prune or `connected`.
- Only when `size` is `0` does the strip say "Build cache: empty" and hide the
  button. A prune error (for example no BuildKit) shows as a toast.

### FR-5 Image details

`get_image` returns the inspect result (`GET /images/{id}/json`) mapped to a
domain type plus the raw JSON, like M1's container inspect.

Overview shows: tags, digests, id, created, total size, platform
(`os/architecture[/variant]`), entrypoint and command, working dir, user,
exposed ports, environment, labels, and **Used by**: the containers (from the
containers query) with links to their details pages. Inspect shows the raw
JSON with the existing Inspect tab viewer.

### FR-6 Live updates

The events supervisor also forwards image events (`type=image`, actions
`create`, `pull`, `tag`, `untag`, `delete`, `import`, `load`; not `save` or
`push`; the builder checks which of these `docker build` emits on the
containerd store) as a new
hint `imagesChanged`, coalesced in the same 250 ms window as container hints.
The frontend invalidates the images list and image details queries on it.
Container hints already refresh usage, because usage comes from the
containers query. `connected` refetches everything, as today.

## 4. IPC types

New commands: `list_images`, `get_image`, `remove_image { id }`,
`get_build_cache_usage`, `prune_build_cache`.
Regenerated into `src/bindings/`:

```ts
type ImageSummary = {
  id: string; // full "sha256:…"
  repoTags: string[]; // "<none>:<none>" filtered out; empty = dangling
  createdAt: number; // unix seconds
  size: number; // bytes
  sharedSize: number | null; // null when the Engine does not report it
};

type ImageInspect = {
  summary: ImageSummary; // sharedSize null here
  repoDigests: string[];
  platform: string; // "linux/amd64", "linux/arm64/v8"
  entrypoint: string[];
  cmd: string[];
  workingDir: string | null;
  user: string | null;
  exposedPorts: string[]; // "80/tcp"
  env: string[];
  labels: Record<string, string>;
  raw: unknown; // the inspect JSON, as in ContainerInspect
};

type BuildCacheUsage = { size: number | null; reclaimable: number | null }; // null: not reported
type BuildCachePruneResult = { spaceReclaimed: number };

type ContainerSummary = { /* ...unchanged */ imageId: string };
type LiveUpdate =
  | /* ...unchanged */
  | { type: "imagesChanged" };
```

`remove_image` returns `null`. Errors use the existing `IpcError` kinds;
`409` maps to `conflict`.

## 5. UI specification

### 5.1 Images page

Route `/images`. The sidebar's Images entry is enabled (drop it from
`UPCOMING`).

- **Header**: title "Images", then a summary line: "N images · about Y
  unused" (Y = sum of unique sizes of unused images). No total across images:
  summing `size` double-counts shared layers.
- **Build cache strip** below the header: "Build cache: 6.4 GB (6.1 GB
  reclaimable)" and a "Clear build cache" button → confirm dialog naming the
  reclaimable size → toast with the space reclaimed.
- **Toolbar**: text filter (matches tag or id), usage filter
  (All / Unused / In use / Stopped), "Remove all unused" (disabled when there
  are none), and "Remove N selected" when a selection exists.
- **Table columns**: checkbox, Name (tags, one per line; dangling as muted
  `<none>`), ID (12 chars), Unique size, Size, Created (relative, absolute in
  the tooltip), Usage badge, Remove button. Clicking a row opens details.
- **Sort**: clickable headers for Name, Unique size, Size, Created. Default
  is Unique size descending, so the biggest wins are on top. Sort and filter
  are kept for the session in Zustand.
- Skeleton, empty ("No images"), filtered-empty, error, and disconnected
  states follow the Containers page.

### 5.2 Image details page

Route `/images/$id` with tabs Overview and Inspect, using the container
details layout. The header shows the first tag (or `<none>` + short id), the
usage badge, a Remove button (same rules as FR-3), and a link back to Images.
After removal it navigates back to Images. A removed image shows the existing
not-found page with "Back to images".

## 6. Acceptance criteria

| ID    | Criterion                                                                                                                                                | Verification               |
| ----- | -------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------- |
| AC-1  | The Images page lists the same images as `docker image ls`, with sizes matching `docker system df -v` (Size, Unique size)                                | manual                     |
| AC-2  | Usage is right for an image with a running container, one used only by stopped containers, and an unused one                                             | unit tests + manual        |
| AC-3  | Default sort puts the largest unique size first; sort headers and filters work                                                                           | unit tests + manual        |
| AC-4  | Removing an unused image (single and multi-tagged) removes it; the row disappears within about 1 s                                                       | integration test + manual  |
| AC-5  | Remove is disabled for images with containers and names them; a `409` race shows the Engine's message                                                    | unit test + manual         |
| AC-6  | Removing several selected images reports partial failures without stopping                                                                               | unit test + manual         |
| AC-7  | "Remove all unused" removes exactly the confirmed Unused image ids, keeps every tag of in-use images, and reports about how much it freed                | manual                     |
| AC-8  | The build cache size matches `docker system df` and clearing it frees the reclaimable space                                                              | manual                     |
| AC-9  | `docker pull busybox:musl` and `docker rmi` in a terminal update the list within about 1 s                                                               | manual                     |
| AC-10 | Image details show the FR-5 fields and the raw JSON; Used by links work                                                                                  | manual                     |
| AC-11 | Every M3 acceptance criterion still holds                                                                                                                | re-run of the M3 checklist |
| AC-12 | `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `pnpm check`, `pnpm test`, `pnpm typecheck` pass | command output             |
| AC-13 | README and the architecture overview describe Images as built                                                                                            | review                     |

Any criterion not verified in the implementation session is reported as
**unverified** with the reason.

## 7. Test plan

### 7.1 Unit tests (`cargo test`, no Docker)

- `ImageSummary` mapping: `<none>:<none>` filtered; `SharedSize -1` → `null`.
- `ImageInspect` mapping: platform with and without variant; missing config.
- Build cache usage: both response shapes, `BuildCacheUsage` present (totals
  used) and absent (legacy-only response → `null`, not `0`).
- Image events: relevant actions produce `imagesChanged`, coalesced with
  container hints in one window.

### 7.2 Docker integration tests (`cargo test -- --ignored`)

Same rules as M1 §7.2. Tests use **their own image id** only. Tagging
`busybox` is not enough: a tag points at the same image, and FR-3 removes
every tag on an id, which would include `busybox:latest` and any user tags.
Each test creates a test container from `busybox` and commits it with the
label `io.docktide.test=<random>` (a distinct image id), tags that image
`docktide-test/<random>:a` (and `:b`), and removes only that id. Teardown
removes the test container and the test image. Never prune build cache in
tests: `prune_build_cache` acts on the whole Engine and is verified manually.

- A test tag lists with its size; inspect returns it.
- Removing a multi-tagged test image removes all its tags.
- Removing an image used by a test container answers `conflict`.

### 7.3 Frontend (`vp test`)

- Usage from containers: running/paused/restarting → In use; only stopped →
  Stopped (N); none → Unused.
- Unique size with `null` shared size; summary totals.
- Sort and filter (text on tags and id; usage filter).
- Selection allows only unused images; bulk removal collects per-image
  failures into one summary.
- `imagesChanged` invalidates the images list and image details.

## 8. Implementation plan

Each slice ends with a working, checked state. Stop for the user's review
after S2.

| Slice                        | Work                                                                                                                       | Done when       |
| ---------------------------- | -------------------------------------------------------------------------------------------------------------------------- | --------------- |
| **S1 Core and IPC**          | `images` module: list, inspect, guarded remove; build cache usage and prune; `imageId`; image events; commands; bindings   | §7.1, §7.2 pass |
| **S2 Images page**           | Route, sidebar, table with usage, sort, filters, selection, remove/bulk/remove-all-unused, build cache strip, live updates | AC-1 to AC-9    |
| **S3 Image details**         | Route, Overview and Inspect tabs, Remove from details                                                                      | AC-10           |
| **S4 Docs and verification** | README, overview (§3, §4, §5, §10), ADR 0005 for §10; run the AC checklist and record results in §11                       | AC-11 to AC-13  |

## 9. Known limitations accepted for M4

- Freed totals are approximate: unique sizes undercount layers shared only
  among removed images. Only the build cache prune reports a real figure.
- Build cache size is not shown on Engines older than API 1.53.
- Usage counts only containers on this Engine; images used by something else
  (a CI job about to run, Kubernetes on another runtime) look unused.
- The build cache figure has no events and can be stale until the page is
  revisited or the window regains focus.
- No force removal: an image used only by stopped containers needs those
  containers removed first, outside the Images page in M4.

## 10. Decisions

1. **D1 Cleanup first:** the page is built around usage and unique size.
   Chosen by the user ("mostly cleaning up unused images to free disk";
   "seeing which images are unused and how big they are").
2. **D2 Build cache in M4:** size and a clear button on the Images page.
   Proposed after finding 6.4 GB of build cache vs 700 MB of images; the user
   answered "sounds good" to the playback that asked about it.
3. **D3 No force removal, no container removal from Images.** The page refuses
   and names the containers instead. Claude's call, agreed in the playback.
4. **D4 Pull and create container deferred** to a later milestone. Claude's
   call, agreed in the playback.

Implementation calls (not separately approved):

- Unique size is the primary size column and the default sort.
- Usage is computed in the frontend from the containers query; `imageId` is
  added to `ContainerSummary` rather than a Rust join.
- "Remove all unused" removes the confirmed image ids through the guarded
  single-image path, not the Engine's prune (per-reference on containerd;
  would widen the confirmed set). Changed after review on 2026-10-29.
- Build cache prune uses `all=true` (as `docker builder prune -a`), since the
  goal is freeing disk.
- One row per image id, with all its tags; no per-tag rows.
- No "total size" in the header (it would double-count shared layers and need
  another Engine call to get right).
- A multi-tag removal checks for containers first, so a refused removal never
  strips tags.

## 11. Verification

Run on 2026-10-29 on the dev host: Ubuntu 26.04.1, kernel 7.0.0-42-generic,
Docker Engine 29.9.2 (API 1.56, rootful, default socket), Rust 1.99.1, Node
24.19.0, pnpm 11.28.6. The user ran the S2 checklist in the running app; the
S3 details page was not checked in the app. Checks that need the running app
are marked **Unverified, pending manual check** unless the user did them or a
test covers them. The Docker tests touched only their own test containers and
images (label `io.docktide.test`); nothing was pruned.

Two defects surfaced during the user's S2 check and were fixed before it passed:
the image commands were missing from Tauri's ACL (`src-tauri/build.rs` manifest
and `capabilities/default.json`; the plan omitted that step), and the selection
was not cleared after removal, so a re-pulled image came back selected.

| ID    | Result                                                  | Evidence                                                                                                                                                                                                                                                                                                                                                      |
| ----- | ------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| AC-1  | Verified by the user (manual, 2026-10-29)               | S2 check after the two fixes: "ok, everything working fine". `list_and_inspect_include_the_test_image` (Docker) covers listing and sizes of a test image.                                                                                                                                                                                                     |
| AC-2  | Verified by the user (manual, 2026-10-29); unit tests   | Same S2 check. Usage tests in `src/features/images/list.test.ts`.                                                                                                                                                                                                                                                                                             |
| AC-3  | Verified by the user (manual, 2026-10-29); unit tests   | Same S2 check. Sort and filter tests in `list.test.ts`; `src/stores/image-view.test.ts`.                                                                                                                                                                                                                                                                      |
| AC-4  | Verified by the user (manual, 2026-10-29); Docker tests | Same S2 check. Removal of single and multi-tagged test images in `tests/docker.rs` (`cargo test -- --ignored`).                                                                                                                                                                                                                                               |
| AC-5  | Verified by the user (manual, 2026-10-29); tests        | Same S2 check. `remove_refuses_an_in_use_image_given_its_short_id` and the in-use conflict test (Docker); disabled-Remove rule in `list.test.ts`. The `409` race was not provoked by hand.                                                                                                                                                                    |
| AC-6  | Verified by the user (manual, 2026-10-29); unit tests   | Same S2 check. `removalSummary` tests in `list.test.ts`.                                                                                                                                                                                                                                                                                                      |
| AC-7  | Verified by the user (manual, 2026-10-29)               | Same S2 check. Removal goes through the guarded single-image path; no Engine prune is called.                                                                                                                                                                                                                                                                 |
| AC-8  | Verified by the user (manual, 2026-10-29)               | Same S2 check; the user did not say whether Clear build cache was clicked. `build_cache_usage_reads_without_error` (Docker, read-only) and four unit tests cover the size reading; `prune_build_cache` was never run by a test, by design (spec §7.2).                                                                                                        |
| AC-9  | Verified by the user (manual, 2026-10-29); unit tests   | Same S2 check. Image event relevance and coalescing: `image_actions_are_relevant`, `image_events_coalesce_into_one_hint_and_are_not_published`, `image_only_window_sends_only_images_changed`; `imagesChanged` case in `apply.test.ts`.                                                                                                                       |
| AC-10 | **Unverified, pending manual check**                    | Not checked in the app. Router tests cover the routes, including `/images/sha256%3Aabc` landing on `/images/sha256%3Aabc/overview`. No component test for the page (no DOM test environment). Used by links and the Remove-then-navigate path are unseen.                                                                                                     |
| AC-11 | Partly verified                                         | All M3 tests pass in the gate below. The M3 checklist was not re-run by hand, so it stays outstanding, as M3 did for M2.                                                                                                                                                                                                                                      |
| AC-12 | Verified                                                | `cargo fmt --all --check` and `cargo clippy --workspace --all-targets -- -D warnings` clean. `cargo test --workspace`: 151 unit + 31 fake-Engine tests passed, 21 ignored. `cargo test --workspace -- --ignored`: 21 passed. `pnpm check`: 119 files formatted, 0 warnings or errors in 94. `pnpm test`: 18 files, 145 tests passed. `pnpm typecheck` passes. |
| AC-13 | Verified, pending review                                | README (status, Features, spec link), overview (§3.1, §3.3, §4, §5, §10), and ADR 0005 describe Images as built.                                                                                                                                                                                                                                              |

Deviations from the spec and plan made during implementation:

- `ImageInspect` is `{ details, raw }`, with `details: ImageDetails` holding the
  mapped fields, not the flat shape in §4.
- Extra files beyond the plan: `confirm-dialog.tsx` (shared by the three
  confirm dialogs), `image-inspect-tab.tsx` (thin wrapper), and
  `src/components/raw-json.tsx` (extracted so both Inspect tabs share it).
- A single-row Remove has no confirm dialog. The spec requires confirms only
  for multi-image and cache removals.
- The success summary reads "Removed 1 image" for one removal, not "1 images".
- Image details: the Remove button and usage badge read the image's row from the
  images and containers queries. If the image is not in the list (just removed,
  not loaded), Remove is disabled. After Remove, the page navigates to Images
  only if the refreshed list no longer contains the id.
- The build cache is read with `df(None)`: Bollard 0.21.1 cannot URL-encode the
  `type` filter. The Engine computes all usage types (0.04 s here against 0.00 s
  typed); upgrade path is a raw request with `?type=build-cache`.
- `ImageSummary.sharedSize` comes from Bollard's required `shared_size`; an
  Engine that omits it would fail the whole list instead of giving `null`.
  Engines 1.42 and later send `-1`, and the minimum supported is 1.44.
- `tsconfig.app.json` lib is ES2023 (was ES2022) for `toSorted`. WebKitGTK 2.52.6
  here supports it (since 2.40); older WebKitGTK would crash.
- A removal blocked by the in-flight guard still clears its ids from the
  selection. It is reachable only through a dialog opened before the run; the
  selection is lost, nothing is removed.
- Confirm dialogs freeze their list when they open, and a second removal is
  rejected while one runs (review findings, fixed in S2).
- The removal guard compares the canonical id from inspect, not the raw input
  (review finding, fixed in S1), so short ids and tags cannot slip past it.
- The five image commands were added to the Tauri ACL after the user's check
  found them missing (see above).
- `#[allow(clippy::too_many_lines)]` on the bindings test, a flat list of types.
- Event relevance filter now requests `type=[container,image]`;
  `container_events()` was renamed `watched_events()`.
- Several tasks had no separate RED run (tests and code written together); see
  the task reports.

Observations, not fixed in M4:

- No serde-shape test for `ImageSummary` or `ImageDetails`; the bindings drift
  test covers type names only.
- The messages "and N more" and "Removed tags a, b; c: …" are untested.
- `removing_a_missing_image_is_ok` does not match the `remove` test filter.
- `images.rs` has a second and third `impl DockerService` block, and two
  `use bollard::models` lines; the `df(None)` comment carries no `ponytail:`
  marker.
- `apply.ts` uses the literal `["image"]` key, duplicating the prefix of
  `imageQueryKey`.
- `image-view.test.ts` depends on test order; the store is not reset in a
  `beforeEach`.
- A confirm clicked during an in-flight removal is dropped silently, with no
  toast.
- `image-page.tsx` reads the images cache with an ad-hoc `{ id: string }[]` type,
  and "Used by" filters containers by `imageId` itself instead of sharing the
  usage helper.
- Image environment variables are shown unmasked on the overview, as in the
  raw inspect (container details hides values until asked).
- Older Engines (API 1.44 to 1.52) show no build cache size; summing the legacy
  array is deferred (spec §9).
