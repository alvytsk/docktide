fn main() {
    // App commands must be listed here and granted in `capabilities/`, or the
    // WebView cannot call them.
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&["get_connection_status", "list_containers"]),
    ))
    .expect("failed to run tauri-build");
}
