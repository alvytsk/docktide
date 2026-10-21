use docktide_core::{ConnectionStatus, ContainerSummary, DockerService, IpcError};
use tauri::State;

/// Failures are part of the status, so this never returns `Err`. Async
/// commands that borrow state must return a `Result`.
#[tauri::command]
async fn get_connection_status(
    docker: State<'_, DockerService>,
) -> Result<ConnectionStatus, IpcError> {
    Ok(docker.connection_status().await)
}

#[tauri::command]
async fn list_containers(
    docker: State<'_, DockerService>,
) -> Result<Vec<ContainerSummary>, IpcError> {
    docker.list_containers().await.map_err(Into::into)
}

/// Builds and runs the Docktide desktop application.
///
/// # Panics
///
/// Panics if Tauri fails to initialize the application or its main window.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt::init();
    tauri::Builder::default()
        .manage(DockerService::new())
        .invoke_handler(tauri::generate_handler![
            get_connection_status,
            list_containers
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
