# Changelog

All notable changes to this project are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Docktide is pre-1.0: any release may contain breaking changes.

A release is cut by renaming `[Unreleased]` to `## [x.y.z] - YYYY-MM-DD`,
bumping the version in `Cargo.toml`, `package.json`, and
`src-tauri/tauri.conf.json`, merging, and pushing a `vx.y.z` tag on `main`.
The release workflow refuses a tag without a matching section.

## [Unreleased]

### Added

- M0 foundation: connection status for the local Docker Engine, endpoint
  resolution that follows the Docker CLI, container list with filters, and
  start, stop, and restart.
- M1 live updates, details, and logs: Engine events refresh the UI within
  about a second, container details with raw `inspect` JSON, and streaming
  logs with ANSI colours, filter, and resume after reconnect.
- M2 monitoring: live CPU and memory columns and a Stats tab with four charts
  and 10 minutes of history, using the `docker stats` formulas.
- M3 Compose: containers grouped by Compose project, and merged project logs
  with service tags and service filters.
- M4 images: image list with unique size and usage, guarded removal, build
  cache size and prune, and image details.
- CI on every pull request (Rust format, clippy, tests, docs; frontend checks,
  tests, and build; `.deb`, `.rpm`, and AppImage bundles), a weekly
  dependency audit, and a tag-driven release workflow.
