//! Client lifecycle and connection status (spec M0 FR-2).

use std::sync::{Mutex, MutexGuard, PoisonError};

use bollard::errors::Error as BollardError;
use bollard::{API_DEFAULT_VERSION, ClientVersion, Docker};
use serde::Serialize;

use crate::endpoint::{self, ResolvedEndpoint};
use crate::error::{DockerError, IpcError};
use crate::system::EngineInfo;

/// Engine 25. Older engines report `apiVersionUnsupported` (ADR 0001 D12).
pub const MIN_API_VERSION: ClientVersion = ClientVersion {
    major_version: 1,
    minor_version: 44,
};

/// Bollard's default request timeout, in seconds.
const REQUEST_TIMEOUT_SECS: u64 = 120;

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

/// Owns the Bollard client. It connects lazily and drops the client after a
/// connection-level failure, so the next request reconnects.
#[derive(Debug, Default)]
pub struct DockerService {
    client: Mutex<Option<(ResolvedEndpoint, Docker)>>,
}

impl DockerService {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Resolves the endpoint and reports its status. Failures are part of the value.
    pub async fn connection_status(&self) -> ConnectionStatus {
        match endpoint::resolve_from_system() {
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

    async fn engine_info(&self, endpoint: &ResolvedEndpoint) -> Result<EngineInfo, DockerError> {
        let docker = self.client(endpoint).await?;
        let (info, version) = tokio::try_join!(docker.info(), docker.version())
            .map_err(|err| self.failed(&err, endpoint))?;
        Ok(EngineInfo::from_docker(
            info,
            version,
            docker.client_version().to_string(),
        ))
    }

    /// The cached client for `endpoint`, or a newly connected and negotiated one.
    pub(crate) async fn client(&self, endpoint: &ResolvedEndpoint) -> Result<Docker, DockerError> {
        if let Some((cached, docker)) = self.cached().as_ref()
            && cached == endpoint
        {
            return Ok(docker.clone());
        }
        let docker = connect(endpoint).await?;
        *self.cached() = Some((endpoint.clone(), docker.clone()));
        Ok(docker)
    }

    /// Classifies a failed request and drops the client if the connection is at fault.
    pub(crate) fn failed(&self, err: &BollardError, endpoint: &ResolvedEndpoint) -> DockerError {
        let error = DockerError::from_bollard(err, &endpoint.uri);
        if error.is_connection_level() {
            *self.cached() = None;
        }
        error
    }

    fn cached(&self) -> MutexGuard<'_, Option<(ResolvedEndpoint, Docker)>> {
        self.client.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

async fn connect(endpoint: &ResolvedEndpoint) -> Result<Docker, DockerError> {
    let classify = |err: BollardError| DockerError::from_bollard(&err, &endpoint.uri);
    let docker =
        Docker::connect_with_socket(&endpoint.uri, REQUEST_TIMEOUT_SECS, API_DEFAULT_VERSION)
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
