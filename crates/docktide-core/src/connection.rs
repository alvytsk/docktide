//! Client lifecycle and connection status (spec M0 FR-2).

use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use bollard::errors::Error as BollardError;
use bollard::{API_DEFAULT_VERSION, ClientVersion, Docker};
use serde::Serialize;
use tokio::sync::{broadcast, watch};

use crate::endpoint::{self, ResolvedEndpoint};
use crate::error::{DockerError, IpcError};
use crate::events::{CONTAINER_EVENT_CAPACITY, ContainerEvent};
use crate::streams::StreamRegistry;
use crate::system::EngineInfo;

/// Engine 25. Older engines report `apiVersionUnsupported` (ADR 0001 D12).
pub const MIN_API_VERSION: ClientVersion = ClientVersion {
    major_version: 1,
    minor_version: 44,
};

/// Bollard's default request timeout, in seconds.
const REQUEST_TIMEOUT_SECS: u64 = 120;

/// Stop and restart wait for the container's `StopTimeout`, so they get longer.
// ponytail: fixed; a StopTimeout above ~59 min still times out. Read it from
// inspect if such containers show up.
pub(crate) const LIFECYCLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3600);

#[derive(Debug, Serialize)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum ConnectionStatus {
    Connected {
        endpoint: ResolvedEndpoint,
        engine: EngineInfo,
    },
    Disconnected {
        endpoint: Option<ResolvedEndpoint>,
        error: IpcError,
    },
}

/// The endpoint, its client, and the engine info fetched once per connection.
type Cached = (ResolvedEndpoint, Docker, Option<EngineInfo>);

type Resolver = dyn Fn() -> Result<ResolvedEndpoint, DockerError> + Send + Sync;

/// Owns the Bollard client. It connects lazily and drops the client after a
/// connection-level failure, so the next request reconnects. Clones share the
/// client and the stream registry, so background tasks can hold one.
#[derive(Clone)]
pub struct DockerService {
    shared: Arc<Shared>,
}

struct Shared {
    resolve: Arc<Resolver>,
    request_timeout: u64,
    client: Mutex<Option<Cached>>,
    /// Bumped whenever the cached client is replaced or dropped.
    generation: watch::Sender<u64>,
    streams: StreamRegistry,
    /// Relevant container events, published by the events supervisor.
    container_events: broadcast::Sender<ContainerEvent>,
}

impl fmt::Debug for DockerService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DockerService")
            .field("client", &self.shared.client)
            .field("streams", &self.shared.streams)
            .finish_non_exhaustive()
    }
}

impl Default for DockerService {
    fn default() -> Self {
        Self::new()
    }
}

impl DockerService {
    /// Resolves the endpoint like the Docker CLI on every connection attempt.
    #[must_use]
    pub fn new() -> Self {
        Self::build(
            Arc::new(endpoint::resolve_from_system),
            REQUEST_TIMEOUT_SECS,
        )
    }

    /// Always uses `endpoint`.
    #[must_use]
    pub fn for_endpoint(endpoint: ResolvedEndpoint) -> Self {
        Self::build(Arc::new(move || Ok(endpoint.clone())), REQUEST_TIMEOUT_SECS)
    }

    /// A new service with the same endpoint resolution and another request
    /// timeout. Streams are only subject to it until the Engine answers.
    #[must_use]
    pub fn with_request_timeout(&self, secs: u64) -> Self {
        Self::build(Arc::clone(&self.shared.resolve), secs)
    }

    fn build(resolve: Arc<Resolver>, request_timeout: u64) -> Self {
        Self {
            shared: Arc::new(Shared {
                resolve,
                request_timeout,
                client: Mutex::default(),
                generation: watch::Sender::new(0),
                streams: StreamRegistry::default(),
                container_events: broadcast::Sender::new(CONTAINER_EVENT_CAPACITY),
            }),
        }
    }

    /// Container events seen by the events supervisor from now on.
    #[expect(dead_code, reason = "used by the stats supervisor (M2 Task 3)")]
    pub(crate) fn container_events(&self) -> broadcast::Receiver<ContainerEvent> {
        self.shared.container_events.subscribe()
    }

    /// Having no subscribers is not an error.
    pub(crate) fn publish_container_event(&self, event: ContainerEvent) {
        let _ = self.shared.container_events.send(event);
    }

    /// Resolves the endpoint and reports its status. Failures are part of the value.
    pub async fn connection_status(&self) -> ConnectionStatus {
        match (self.shared.resolve)() {
            Ok(endpoint) => self.connection_status_for(endpoint).await,
            Err(error) => ConnectionStatus::Disconnected {
                endpoint: None,
                error: error.into(),
            },
        }
    }

    /// Reports the status of an already resolved endpoint.
    pub async fn connection_status_for(&self, endpoint: ResolvedEndpoint) -> ConnectionStatus {
        match self.engine_info(&endpoint).await {
            Ok(engine) => ConnectionStatus::Connected { endpoint, engine },
            Err(error) => ConnectionStatus::Disconnected {
                endpoint: Some(endpoint),
                error: error.into(),
            },
        }
    }

    /// Polls with `/_ping` once `/info` and `/version` were fetched for this connection.
    // ponytail: the Engine version and counts stay as fetched until a reconnect, so a
    // daemon upgraded between two polls shows its old version; M1's events fix that.
    async fn engine_info(&self, endpoint: &ResolvedEndpoint) -> Result<EngineInfo, DockerError> {
        let docker = self.client(endpoint).await?;
        if let Some(engine) = self.cached_engine(endpoint) {
            docker
                .ping()
                .await
                .map_err(|err| self.failed(&err, endpoint))?;
            return Ok(engine);
        }
        let (info, version) = tokio::try_join!(docker.info(), docker.version())
            .map_err(|err| self.failed(&err, endpoint))?;
        let engine = EngineInfo::from_docker(info, version, docker.client_version().to_string());
        if let Some((cached, _, slot)) = self.cached().as_mut()
            && cached == endpoint
        {
            *slot = Some(engine.clone());
        }
        Ok(engine)
    }

    fn cached_engine(&self, endpoint: &ResolvedEndpoint) -> Option<EngineInfo> {
        self.cached()
            .as_ref()
            .filter(|(cached, ..)| cached == endpoint)
            .and_then(|(.., engine)| engine.clone())
    }

    /// The current endpoint and its client, for a request whose failure is classified by `failed`.
    pub(crate) async fn docker(&self) -> Result<(ResolvedEndpoint, Docker), DockerError> {
        let endpoint = (self.shared.resolve)()?;
        let docker = self.client(&endpoint).await?;
        Ok((endpoint, docker))
    }

    /// The cached client for `endpoint`, or a newly connected and negotiated one.
    async fn client(&self, endpoint: &ResolvedEndpoint) -> Result<Docker, DockerError> {
        if let Some((cached, docker, _)) = self.cached().as_ref()
            && cached == endpoint
        {
            return Ok(docker.clone());
        }
        let docker = connect(endpoint, self.shared.request_timeout).await?;
        *self.cached() = Some((endpoint.clone(), docker.clone(), None));
        self.shared.generation.send_modify(|g| *g += 1);
        Ok(docker)
    }

    /// Classifies a failed request and drops the client if the connection is at fault.
    pub(crate) fn failed(&self, err: &BollardError, endpoint: &ResolvedEndpoint) -> DockerError {
        let error = DockerError::from_bollard(err, &endpoint.uri);
        if error.is_connection_level() {
            self.drop_client();
        }
        error
    }

    /// Makes the next request reconnect.
    pub(crate) fn drop_client(&self) {
        if self.cached().take().is_some() {
            self.shared.generation.send_modify(|g| *g += 1);
        }
    }

    /// Changes whenever the cached client is replaced or dropped.
    pub(crate) fn generation(&self) -> watch::Receiver<u64> {
        self.shared.generation.subscribe()
    }

    pub(crate) fn streams(&self) -> &StreamRegistry {
        &self.shared.streams
    }

    fn cached(&self) -> MutexGuard<'_, Option<Cached>> {
        self.shared
            .client
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

async fn connect(endpoint: &ResolvedEndpoint, timeout: u64) -> Result<Docker, DockerError> {
    let classify = |err: BollardError| DockerError::from_bollard(&err, &endpoint.uri);
    let docker = Docker::connect_with_socket(&endpoint.uri, timeout, API_DEFAULT_VERSION)
        .map_err(classify)?;
    docker.ping().await.map_err(classify)?;
    // Lowers the client version to the server's if the server is older; never raises it.
    let docker = docker.negotiate_version().await.map_err(classify)?;
    check_api_version(docker.client_version())?;
    Ok(docker)
}

fn check_api_version(negotiated: ClientVersion) -> Result<(), DockerError> {
    if negotiated < MIN_API_VERSION {
        return Err(DockerError::ApiVersionUnsupported {
            version: negotiated.to_string(),
            min: MIN_API_VERSION.to_string(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(major_version: usize, minor_version: usize) -> ClientVersion {
        ClientVersion {
            major_version,
            minor_version,
        }
    }

    #[test]
    fn api_below_minimum_is_unsupported() {
        assert!(matches!(
            check_api_version(version(1, 43)),
            Err(DockerError::ApiVersionUnsupported { version, min })
                if version == "1.43" && min == "1.44"
        ));
    }

    #[test]
    fn api_at_or_above_minimum_is_accepted() {
        for v in [version(1, 44), version(1, 53), version(2, 0)] {
            assert!(check_api_version(v).is_ok());
        }
    }
}

#[cfg(test)]
mod ipc_tests {
    use serde_json::json;

    use super::*;
    use crate::endpoint::EndpointSource;

    #[test]
    fn disconnected_status_serializes_to_the_ipc_shape() {
        let status = ConnectionStatus::Disconnected {
            endpoint: Some(ResolvedEndpoint {
                uri: "unix:///run/work.sock".into(),
                source: EndpointSource::DockerContextEnv,
                context_name: Some("work".into()),
            }),
            error: DockerError::Timeout.into(),
        };
        assert_eq!(
            serde_json::to_value(status).unwrap(),
            json!({
                "state": "disconnected",
                "endpoint": {
                    "uri": "unix:///run/work.sock",
                    "source": "dockerContextEnv",
                    "contextName": "work",
                },
                "error": { "kind": "timeout", "message": "Docker did not respond in time." },
            })
        );
    }

    #[test]
    fn disconnected_status_without_endpoint_serializes_null() {
        let status = ConnectionStatus::Disconnected {
            endpoint: None,
            error: DockerError::UnsupportedEndpoint {
                uri: "tcp://127.0.0.1:2375".into(),
            }
            .into(),
        };
        let json = serde_json::to_value(status).unwrap();
        assert_eq!(json["endpoint"], serde_json::Value::Null);
        assert_eq!(json["error"]["kind"], "unsupportedEndpoint");
    }
}
