fn main() {
    // App commands must be listed here and granted in `capabilities/`, or the
    // WebView cannot call them.
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "get_connection_status",
            "subscribe_live_updates",
            "list_containers",
            "get_container",
            "start_container",
            "stop_container",
            "restart_container",
            "open_logs",
            "close_logs",
        ]),
    ))
    .expect("failed to run tauri-build");
}
