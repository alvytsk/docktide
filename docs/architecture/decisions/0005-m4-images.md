# ADR 0005: Images (M4)

- Status: **Accepted**
- Date: 2026-10-29 (decisions taken 2026-10-29)
- Deciders: project maintainer
- Related: [M4 spec](../../specifications/m4-images.md) §10, [ADR 0001](0001-initial-architecture.md), [ADR 0004](0004-m3-compose.md)

## Context

M4 gives Images a page. The user opens it mostly to free disk space, so the
questions were what the page is built around, whether the build cache belongs
in it, and how far removal may go.

Facts established while deciding (Engine 29.9.2, containerd image store):

| Area                | Finding                                                                                                          |
| ------------------- | ---------------------------------------------------------------------------------------------------------------- |
| Disk use            | About 700 MB of images against 6.4 GB of build cache.                                                            |
| Image sizes         | `Size` is the total, `SharedSize` the part shared with other images; `Containers` is always `-1` in the list.    |
| Image ids           | A container's `ImageID` equals the image's full `sha256:` id.                                                    |
| Image prune         | On the containerd store it works per reference, so it can untag one tag of an image whose other tag is in use.   |
| Build cache in `df` | API 1.53 and later report `BuildCacheUsage`; older Engines send only a legacy array that Bollard does not model. |

## Decisions

### D1: Cleanup first

The page is built around usage and unique size (what removing an image alone
frees), sorted largest first. Chosen by the user.

### D2: Build cache in M4

Its size shows on the Images page with a button to clear it, because it is
where most of the disk went. Proposed after the measurement above; the user
agreed.

### D3: No force removal, no container removal from Images

An image with containers cannot be removed from the page; its Remove button is
disabled and names the containers. Force removal and removing containers are
left out.

### D4: Pull and create container deferred

Neither helps cleanup. They wait for a later milestone.

### Implementation calls (not separately approved)

- Usage is computed in the frontend from the containers query; `imageId` is
  added to `ContainerSummary` instead of a Rust join.
- One row per image id with all its tags.
- Removal checks all containers for the image's canonical id (from inspect)
  before touching a tag, then removes tags one by one, so a refused removal never
  strips tags. The Engine's `ancestor=` filter is not used: it also matches
  descendants.
- Post-review change: "Remove all unused" removes the confirmed image ids
  through that guarded path. It does not call the Engine's prune, which works per
  reference on containerd and would also remove images that became unused after
  the dialog opened.
- Build cache prune uses `all=true`, as `docker builder prune -a`.
- The build cache is read with `df(None)` because Bollard 0.21.1 cannot encode
  the `type` filter; on this host a full `df` costs 0.04 s.
- Image events (`create`, `pull`, `tag`, `untag`, `delete`, `import`, `load`)
  become one `imagesChanged` hint in the same 250 ms window as container hints.
  The frontend invalidates images and image details on it.
- Confirm dialogs freeze the list they show when they open, and only one removal
  runs at a time.
- Removal results are summarised in one toast: "Removed 3 of 4. name: message".
- `tsconfig.app.json` lib is ES2023 for `toSorted`.
- Tauri commands must be listed in `src-tauri/build.rs` and
  `capabilities/default.json`; the M4 plan missed this and the first run of the
  page failed.

## Consequences

- Freed totals are approximate; only the build cache prune reports a real figure.
- Images used by something outside this Engine look unused.
- The build cache size has no events and can be stale until the page is
  revisited or the window regains focus.
- Older Engines (API 1.44 to 1.52) show no build cache size, and the strip keeps
  the Clear button.
- Removing an image used only by stopped containers needs those containers
  removed first, outside the Images page.
- No component tests cover the pages: the repo has no DOM test environment.
  Logic is unit tested; the pages are checked by hand, and the details page was
  not yet.
