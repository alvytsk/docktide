# Docktide

A modern, lightweight, Linux-first Docker manager with integrated real-time monitoring.

> **Status: pre-alpha.** Milestone M0 (foundation) is in progress. Nothing is usable yet.

## Features

Planned for M0: connection status, a container list, and start, stop, and restart.
See the [M0 specification](docs/specifications/m0-foundation.md) and the
[roadmap](docs/architecture/overview.md#10-roadmap-tentative).

## Requirements

- Linux with Wayland or X11.
- A local Docker Engine, rootful or rootless.
- Your user must be able to open the Docker socket. Docktide never runs as root.
  Membership in the `docker` group is equivalent to root access on the host;
  [rootless Docker](https://docs.docker.com/engine/security/rootless/) is the safer option.

## Development

### Prerequisites

- Rust 1.90 or newer (stable)
- Node.js 22.18+ or 24.11+
- pnpm 11 (`corepack enable` picks up the version from `package.json`)
- System libraries for Tauri on Debian/Ubuntu:

  ```sh
  sudo apt install build-essential libwebkit2gtk-4.1-dev
  ```

  Tray, menu, and bundling features (later milestones) will also need
  `libayatana-appindicator3-dev`, `librsvg2-dev`, and `libxdo-dev`. See
  [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/#linux).

### Run

```sh
pnpm install
pnpm tauri dev
```

### Checks

```sh
pnpm check          # format, lint, and type check (Vite+: Oxfmt, Oxlint, TypeScript 7)
pnpm fix            # the same, with autofixes
pnpm test           # frontend unit tests
pnpm typecheck      # plain tsc, as a cross-check

cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Tests that need a running Docker Engine are marked `#[ignore]` and run with
`cargo test --workspace -- --ignored`. They only touch containers they create
themselves (label `io.docktide.test=true`).

## Architecture

- [Architecture overview](docs/architecture/overview.md)
- [Architecture decisions](docs/architecture/decisions/)
- [Specifications](docs/specifications/)

## Contributing

Docktide is developed specification-first. Substantial features start with a spec
in `docs/specifications/`, and significant decisions are recorded as ADRs.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT License ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for
inclusion in this work, as defined in the Apache-2.0 license, shall be dual licensed
as above, without any additional terms or conditions.
