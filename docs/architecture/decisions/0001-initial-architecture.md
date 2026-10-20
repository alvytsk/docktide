# ADR 0001: Initial architecture and technology baseline

- Status: **Accepted**
- Date: 2026-10-20 (accepted 2026-10-20)
- Deciders: project maintainer

## Context

Docktide is a new, empty repository. Before writing code we need a baseline
for crate boundaries, the frontend stack, and the Docker integration approach,
so that M0 does not bake in choices that are expensive to undo.

Facts established on 2026-10-20 (registry queries, source inspection, and the
development host). Versions are the newest stable releases on that date.

| Area                                      | Finding                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| ----------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Tauri                                     | Stable `tauri` 2.12.4, `@tauri-apps/cli` 2.12.3, `@tauri-apps/api` 2.12.4. **Tauri 3.0.0-alpha.5** is published (alpha since 2026-09-13).                                                                                                                                                                                                                                                                                                                |
| Bollard                                   | 0.21.1. Client API version 1.53. `negotiate_version()` only downgrades. Honours `DOCKER_HOST=unix://…`. **No Docker context support.** Treats HTTP 304 as success.                                                                                                                                                                                                                                                                                       |
| Engine (dev host)                         | Docker 29.9.1, API 1.56 (minimum 1.40), containerd image store, cgroup v2, rootful, user is in the `docker` group. Compose v5.6.0.                                                                                                                                                                                                                                                                                                                       |
| TypeScript                                | `typescript@latest` is **7.0.2** (native Go compiler; the package exposes only an `unstable/*` JS API). `typescript-eslint` 8.72 requires `typescript <6.1.0`, so TS 7 rules out ESLint-based type-aware linting.                                                                                                                                                                                                                                        |
| Vite+                                     | `vite-plus` **1.3.0** (MIT; 1.0 released 2026-09-28). Bundles Vite 8.3.4 (as `@voidzero-dev/vite-plus-core`), Vitest 5.0.5, Oxlint 1.88, Oxfmt 0.73, and `oxlint-tsgolint` 7.0.2004 (type-aware lint and type check on the TypeScript 7 toolchain). Single CLI `vp`; config in `vite.config.ts`.                                                                                                                                                         |
| Vite+ spike (scratch project, 2026-10-20) | With TS 7.0.2, `@vitejs/plugin-react` 6.1.2, and `@tailwindcss/vite` 4.3.3: install works, but pnpm reports an unmet `vite` peer because the alias reports version 1.3.0 (cosmetic; silenced with `peerDependencyRules.allowedVersions`); `vp check` caught a TS type error (`TS2322`), a `react-hooks/rules-of-hooks` violation, and a `no-floating-promises` violation; after the fixes `vp check`, `vp test run`, `vp build`, and `tsc` 7 all passed. |
| Frontend                                  | React 19.3, `@vitejs/plugin-react` 6.1 (requires Vite 8), Tailwind 4.3 with `@tailwindcss/vite`, shadcn CLI 4.22 (uses `ts-morph`, which bundles its own TypeScript), TanStack Query 5.104, Zustand 5.0.                                                                                                                                                                                                                                                 |
| Type generation                           | `ts-rs` 12.0.1 (stable). `specta` / `tauri-specta` 2.0.0-rc.25 (release candidates; last published 2026-05).                                                                                                                                                                                                                                                                                                                                             |

## Decisions

### D1: Tauri 2.12.x, not Tauri 3

Use the Tauri 2 stable line. Tauri 3 is in alpha with a fast release cadence;
building on it would put framework churn on our critical path. Revisit when
Tauri 3 is stable and its migration guide is published.

### D2: Two Rust packages now; split further only on evidence

```text
Cargo.toml              # virtual workspace, resolver 3, edition 2024, shared [workspace.lints]
crates/docktide-core/   # Docker integration and domain logic, no Tauri
src-tauri/              # Tauri binary: adapter, lifecycle, desktop integration
```

The boundary that matters is "Docker logic does not depend on Tauri". A separate
crate enforces it at compile time and lets core tests run without WebKitGTK.

`docktide-monitor` is **deferred**. In M0 it would be an empty crate. Monitoring
code starts as `docktide-core::stats` and is extracted when there is a concrete
reason: compile time, a second consumer (CLI/TUI), or a dependency we want
isolated.

Alternatives considered:

- _Single crate (`src-tauri` only):_ simplest, but the Tauri-independence rule
  would only be a convention, and tests would need the Tauri build chain.
- _Three crates as proposed:_ adds an empty crate and cross-crate type plumbing
  with no benefit yet.

### D3: Bollard for all ordinary Docker operations

All container, image, volume, network, event, stats, log, and exec operations go
through Bollard over the Unix socket. The `docker` CLI is never used as an API
substitute. The `docker compose` CLI is used only for orchestration that has no
Engine API equivalent (`up`, `down`, and similar), spawned without a shell.

Bollard types stay inside `docktide-core`. Core exposes its own domain types, so
a Bollard upgrade or replacement does not ripple into IPC or the UI.

### D4: Docktide resolves the Docker endpoint itself

Because Bollard lacks context support, `docktide-core::endpoint` implements the
CLI's precedence (`DOCKER_HOST` → `DOCKER_CONTEXT` → `currentContext` → default
socket), plus a rootless-socket fallback (see overview §6.1). M0 supports
`unix://` only and reports other schemes as `unsupportedEndpoint`.

### D5: No duplicated authoritative state

Docker Engine is the authority for resources. Rust keeps no resource cache in M0.
TanStack Query is the only frontend cache. Zustand holds UI state only. Later,
the events stream drives _invalidation_, not a mirrored store.

### D6: Typed errors with stable kinds across IPC

`docktide-core` defines `DockerError` (`thiserror`) with variants that map 1:1 to
UI-relevant kinds: `daemonUnavailable`, `permissionDenied`, `unsupportedEndpoint`,
`invalidConfiguration`, `apiVersionUnsupported`, `notFound`, `conflict`, `timeout`, `internal`. The
adapter serializes them as `{ kind, message, hint? }`. `anyhow` is used only in
`src-tauri` setup code (startup, logging init), never in core APIs.

### D7: Frontend stack

| Choice             | Decision                                                                                                                                           | Note                                                                                                                                                                       |
| ------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Package manager    | pnpm (11.x present on host)                                                                                                                        | `packageManager` field pins it                                                                                                                                             |
| Frontend toolchain | **Vite+ (`vite-plus`), pinned exactly**, as a project-local dev dependency                                                                         | One tool for dev, build, test, lint, format, and type check. The global `vp` installer is optional and not required by the project (it also manages global Node and pnpm). |
| TypeScript         | **7.0.x** (`~7.0`)                                                                                                                                 | Possible because linting no longer goes through typescript-eslint. `vp check` type-checks via tsgolint; plain `tsc` 7 is kept as a cross-check.                            |
| Build / dev        | `vp dev`, `vp build` (bundled Vite 8 + Rolldown) with `@vitejs/plugin-react` 6                                                                     | pnpm `overrides` alias `vite` to `@voidzero-dev/vite-plus-core` and pin `vitest`, so plugins resolve the bundled copies.                                                   |
| Lint               | `vp lint` (Oxlint) with `typeAware` + `typeCheck`, `react`, `react-hooks`, `typescript`, `oxc` rules                                               | No ESLint. JS plugins are available if a needed rule is missing.                                                                                                           |
| Format             | `vp fmt` (Oxfmt)                                                                                                                                   | Replaces Prettier. Rust keeps `cargo fmt`.                                                                                                                                 |
| Tests              | `vp test` (bundled Vitest 5), imports from `vite-plus/test`                                                                                        | Testing Library added when component tests are written.                                                                                                                    |
| Styling            | Tailwind CSS 4 via `@tailwindcss/vite`; shadcn/ui components copied in, `base-nova` style on Base UI (`@base-ui/react`), shadcn's default since v4 |                                                                                                                                                                            |
| Server state       | TanStack Query 5                                                                                                                                   |                                                                                                                                                                            |
| UI state           | Zustand 5                                                                                                                                          |                                                                                                                                                                            |
| Routing            | **None in M0**                                                                                                                                     | Only one functional page. Adopt TanStack Router when a second page lands (M1) if deep links and typed params pay off.                                                      |
| Charts             | Deferred to M2                                                                                                                                     |                                                                                                                                                                            |
| Terminal           | `@xterm/xterm` 6, deferred to M6                                                                                                                   |                                                                                                                                                                            |

Vite+ configuration rules learned from the spike:

- Client types come from `vite-plus/client`, not `vite/client`, because `vite`
  is not a direct dependency.
- tsgolint does not support `baseUrl`. Path aliases use `paths` only.
- Lint and format follow `.gitignore`. In addition, `src-tauri/target/` and
  generated `src/bindings/` are excluded explicitly, so `vp fmt` does not rewrite
  generated files (which would break the bindings drift test).
- `vp check` runs formatting first and stops on format errors. `vp check --fix` is
  the local workflow, and plain `vp check` is the gate.
- `vp <name>` runs built-ins and `vp run <name>` runs scripts. Scripts named
  `dev`, `build`, and so on map to the built-ins, and Tauri calls them through
  `pnpm dev` / `pnpm build`.

Alternatives considered:

- _ESLint 10 + typescript-eslint + Prettier + TypeScript 6.0:_ the most mature
  rule ecosystem, but it pins TS below 6.1 and means four tools with four configs.
- _Standalone Oxlint + Oxfmt + Vite:_ the same engines, without the single
  config and CLI. Vite+ adds little weight on top.

### D8: Generate TS types with `ts-rs`

DTOs derive `ts_rs::TS` behind a `ts` cargo feature in `docktide-core` and are
exported to `src/bindings/`. A test checks that the generated files are up to date.
Command wrappers in `src/lib/ipc.ts` stay hand-written, because there are few of
them and they are trivial.

Alternatives:

- _Hand-written TS types:_ zero tooling, but drift is silent.
- _`tauri-specta`:_ also generates command bindings, which is attractive, but it
  has been a release candidate for a long time and couples our build to it.

### D9: Security posture

The app never runs as root and never escalates. No shell, fs, or http Tauri
plugins. Minimal capabilities, strict CSP. See overview §8.

### D10: License

Dual-licensed `MIT OR Apache-2.0`, the Rust ecosystem norm. Contributions are
accepted under the same terms (Apache-2.0 §5). License texts are in
`LICENSE-MIT` and `LICENSE-APACHE`.

### D11: Application identifier

`io.github.alvytsk.docktide`. It determines config, data, and cache paths
(`~/.config/io.github.alvytsk.docktide/`, and so on) and the desktop entry, so
it must not change after the first release.

### D12: M0 defaults for open Docker questions

- Minimum supported Engine API version: **1.44** (Engine 25).
- The rootless-socket fallback (overview §6.1, step 5) is **kept**.

Both are cheap to revisit before the first release.

## Consequences

- M0 scaffolding is small: a virtual workspace with two members and a Vite app.
- Endpoint resolution is our code to maintain, including Docker context file
  format changes. It is small and well tested.
- The frontend toolchain depends on a young project (Vite+ 1.x, about a month
  stable). The exact pin and overrides contain the risk, and the fallback is
  standalone Vite + Oxlint + Oxfmt, which uses the same engines and config
  semantics.
- Editor support for TypeScript 7 (language server) must be checked per editor.
  The CLI gate does not depend on it.
- When monitoring lands, revisit D2 explicitly in a new ADR.
