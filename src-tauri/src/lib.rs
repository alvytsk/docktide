use docktide_core::{
    BuildCachePruneResult, BuildCacheUsage, ConnectionStatus, ContainerInspect, ContainerSummary,
    DockerService, ImageInspect, ImageSummary, IpcError, LiveUpdate, LogEvent, StatsSample,
    SubscriptionId,
};
use tauri::ipc::Channel;
use tauri::webview::PageLoadEvent;
use tauri::{Manager, State, WindowEvent};

/// Failures are part of the status, so this never returns `Err`. Async
/// commands that borrow state must return a `Result`.
#[tauri::command]
async fn get_connection_status(
    docker: State<'_, DockerService>,
) -> Result<ConnectionStatus, IpcError> {
    Ok(docker.connection_status().await)
}

/// Replaces any previous subscriber. Streams end when the page reloads (see `run`).
#[tauri::command]
async fn subscribe_live_updates(
    docker: State<'_, DockerService>,
    on_update: Channel<LiveUpdate>,
) -> Result<(), IpcError> {
    docker.subscribe_live_updates(move |update| on_update.send(update).is_ok());
    Ok(())
}

/// Replaces any previous subscriber. Streams end when the page reloads (see `run`).
#[tauri::command]
async fn subscribe_stats(
    docker: State<'_, DockerService>,
    on_stats: Channel<Vec<StatsSample>>,
) -> Result<(), IpcError> {
    docker.subscribe_stats(move |samples| on_stats.send(samples).is_ok());
    Ok(())
}

#[tauri::command]
async fn list_containers(
    docker: State<'_, DockerService>,
) -> Result<Vec<ContainerSummary>, IpcError> {
    docker.list_containers().await.map_err(Into::into)
}

#[tauri::command]
async fn get_container(
    docker: State<'_, DockerService>,
    id: String,
) -> Result<ContainerInspect, IpcError> {
    docker.inspect_container(&id).await.map_err(Into::into)
}

#[tauri::command]
async fn list_images(docker: State<'_, DockerService>) -> Result<Vec<ImageSummary>, IpcError> {
    docker.list_images().await.map_err(Into::into)
}

#[tauri::command]
async fn get_image(docker: State<'_, DockerService>, id: String) -> Result<ImageInspect, IpcError> {
    docker.inspect_image(&id).await.map_err(Into::into)
}

#[tauri::command]
async fn remove_image(docker: State<'_, DockerService>, id: String) -> Result<(), IpcError> {
    docker.remove_image(&id).await.map_err(Into::into)
}

#[tauri::command]
async fn get_build_cache_usage(
    docker: State<'_, DockerService>,
) -> Result<BuildCacheUsage, IpcError> {
    docker.build_cache_usage().await.map_err(Into::into)
}

#[tauri::command]
async fn prune_build_cache(
    docker: State<'_, DockerService>,
) -> Result<BuildCachePruneResult, IpcError> {
    docker.prune_build_cache().await.map_err(Into::into)
}

#[tauri::command]
async fn start_container(docker: State<'_, DockerService>, id: String) -> Result<(), IpcError> {
    docker.start_container(&id).await.map_err(Into::into)
}

#[tauri::command]
async fn stop_container(docker: State<'_, DockerService>, id: String) -> Result<(), IpcError> {
    docker.stop_container(&id).await.map_err(Into::into)
}

#[tauri::command]
async fn restart_container(docker: State<'_, DockerService>, id: String) -> Result<(), IpcError> {
    docker.restart_container(&id).await.map_err(Into::into)
}

/// Follows a container's logs; see `DockerService::open_logs`. Returns the id
/// for `close_logs`. Streams also end when the page reloads (see `run`).
#[tauri::command]
async fn open_logs(
    docker: State<'_, DockerService>,
    id: String,
    tail: Option<u32>,
    after: Option<String>,
    on_event: Channel<LogEvent>,
) -> Result<SubscriptionId, IpcError> {
    docker
        .open_logs(&id, tail, after, move |event| on_event.send(event).is_ok())
        .await
        .map_err(Into::into)
}

/// Unknown ids are ignored, so closing twice is harmless.
#[tauri::command]
async fn close_logs(
    docker: State<'_, DockerService>,
    subscription: SubscriptionId,
) -> Result<(), IpcError> {
    docker.cancel_stream(subscription);
    Ok(())
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
        // A Channel does not tell Rust that its page went away, so streams are
        // stopped when a page starts loading and when the window closes.
        .on_page_load(|webview, payload| {
            if payload.event() == PageLoadEvent::Started {
                webview.state::<DockerService>().cancel_all_streams();
            }
        })
        .on_window_event(|window, event| {
            if matches!(event, WindowEvent::Destroyed) {
                window.state::<DockerService>().cancel_all_streams();
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_connection_status,
            subscribe_live_updates,
            subscribe_stats,
            list_containers,
            get_container,
            start_container,
            stop_container,
            restart_container,
            list_images,
            get_image,
            remove_image,
            get_build_cache_usage,
            prune_build_cache,
            open_logs,
            close_logs,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
