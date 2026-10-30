# Security policy

## Threat model

Docktide talks to the local Docker Engine over its Unix socket. Access to that
socket is root-equivalent on the host, so the main risk is anything that lets
untrusted input drive the Engine on the user's behalf.

**In scope:**

- **The WebView reaching the socket.** Only Rust talks to Docker. The WebView
  can call only the commands registered in `src-tauri/build.rs` and granted in
  `src-tauri/capabilities/default.json`. There is no shell, `fs`, or `http`
  plugin, a strict CSP is set, and no remote content is loaded.
- **Untrusted data from containers.** Log output, labels, environment values,
  and image metadata are rendered as text. Log escape sequences are filtered in
  Rust before they reach the UI.
- **Privilege escalation.** Docktide runs as the desktop user and never calls
  `sudo` or `pkexec` or changes socket permissions.

**Out of scope:**

- What a user with socket access can already do with the `docker` CLI.
- Remote Engines (`tcp://`, `ssh://`): not supported yet.

See [§8 of the architecture overview](docs/architecture/overview.md#8-security-model).

## Supported versions

Docktide is pre-1.0. Only the latest release and `main` are supported.

## Reporting a vulnerability

E-mail **alvy.tsk@gmail.com** with a description, its impact, and steps to
reproduce. Please do not open a public issue for security problems.

Expect an acknowledgement within 7 days and an agreed disclosure timeline
within 14 days. Reporters are credited in the fix unless they prefer not to be.
