//! Typed Docker errors with stable, UI-relevant kinds (spec M0 FR-5).

use std::error::Error as StdError;
use std::io;
use std::path::PathBuf;

use bollard::errors::Error as BollardError;
use serde::Serialize;

/// A failure the UI can explain. `Display` is the user-facing message.
#[derive(Debug, thiserror::Error)]
pub enum DockerError {
    #[error("Docker is not running at {endpoint}.")]
    DaemonUnavailable { endpoint: String },
    #[error("Permission denied for {endpoint}.")]
    PermissionDenied { endpoint: String },
    #[error("Remote endpoints are not supported yet: {uri}")]
    UnsupportedEndpoint { uri: String },
    #[error("{}", match context {
        Some(name) => format!("Docker context {name} is not configured correctly."),
        None => "Docker configuration is not valid.".to_owned(),
    })]
    InvalidConfiguration {
        context: Option<String>,
        path: PathBuf,
        detail: String,
    },
    #[error("Docker Engine API {version} is older than the supported {min}.")]
    ApiVersionUnsupported { version: String, min: String },
    #[error("Container no longer exists.")]
    NotFound,
    #[error("{message}")]
    Conflict { message: String },
    #[error("Docker did not respond in time.")]
    Timeout,
    #[error("{message}")]
    Internal { message: String },
}

/// The stable kinds the frontend switches on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum ErrorKind {
    DaemonUnavailable,
    PermissionDenied,
    UnsupportedEndpoint,
    InvalidConfiguration,
    ApiVersionUnsupported,
    NotFound,
    Conflict,
    Timeout,
    Internal,
}

/// The error shape sent over IPC. `message` is safe to show as is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct IpcError {
    pub kind: ErrorKind,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub hint: Option<String>,
}

impl From<DockerError> for IpcError {
    fn from(err: DockerError) -> Self {
        Self {
            kind: err.kind(),
            message: err.to_string(),
            hint: err.hint(),
        }
    }
}

impl DockerError {
    #[must_use]
    pub fn kind(&self) -> ErrorKind {
        match self {
            Self::DaemonUnavailable { .. } => ErrorKind::DaemonUnavailable,
            Self::PermissionDenied { .. } => ErrorKind::PermissionDenied,
            Self::UnsupportedEndpoint { .. } => ErrorKind::UnsupportedEndpoint,
            Self::InvalidConfiguration { .. } => ErrorKind::InvalidConfiguration,
            Self::ApiVersionUnsupported { .. } => ErrorKind::ApiVersionUnsupported,
            Self::NotFound => ErrorKind::NotFound,
            Self::Conflict { .. } => ErrorKind::Conflict,
            Self::Timeout => ErrorKind::Timeout,
            Self::Internal { .. } => ErrorKind::Internal,
        }
    }

    /// An actionable next step, if there is one.
    #[must_use]
    pub fn hint(&self) -> Option<String> {
        let hint = match self {
            Self::DaemonUnavailable { .. } => {
                "Start Docker with systemctl start docker, or with systemctl --user start docker \
                 for rootless Docker."
                    .to_owned()
            }
            Self::PermissionDenied { .. } => {
                "Your user cannot access this socket. Adding yourself to the docker group grants \
                 access, but that group is root-equivalent. Rootless Docker is the safer option."
                    .to_owned()
            }
            Self::UnsupportedEndpoint { .. } => {
                "Set DOCKER_HOST or the Docker context to a local unix:// socket.".to_owned()
            }
            Self::InvalidConfiguration { path, detail, .. } => {
                format!("Check {}: {detail}", path.display())
            }
            Self::ApiVersionUnsupported { .. } => "Upgrade Docker Engine.".to_owned(),
            Self::NotFound | Self::Conflict { .. } | Self::Timeout | Self::Internal { .. } => {
                return None;
            }
        };
        Some(hint)
    }

    /// Whether the cached client should be dropped and rebuilt on the next request.
    #[must_use]
    pub fn is_connection_level(&self) -> bool {
        matches!(
            self,
            Self::DaemonUnavailable { .. } | Self::PermissionDenied { .. } | Self::Timeout
        )
    }

    /// Classifies a Bollard failure for a request to `endpoint`.
    pub(crate) fn from_bollard(err: &BollardError, endpoint: &str) -> Self {
        let io_kind = match err {
            BollardError::RequestTimeoutError => return Self::Timeout,
            // Bollard checks `Path::exists`, which is also false when a parent
            // directory cannot be searched.
            BollardError::SocketNotFoundError(path) => Some(
                std::fs::metadata(path).map_or_else(|err| err.kind(), |_| io::ErrorKind::NotFound),
            ),
            BollardError::DockerResponseServerError {
                status_code,
                message,
            } => {
                return match status_code {
                    404 => Self::NotFound,
                    409 => Self::Conflict {
                        message: message.clone(),
                    },
                    _ => Self::Internal {
                        message: message.clone(),
                    },
                };
            }
            // `IOError` is `#[error(transparent)]`, so its `io::Error` is not in the source chain.
            BollardError::IOError { err } => Some(err.kind()),
            _ => io_kind_in_chain(err),
        };
        let endpoint = endpoint.to_owned();
        match io_kind {
            Some(io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused) => {
                Self::DaemonUnavailable { endpoint }
            }
            Some(io::ErrorKind::PermissionDenied) => Self::PermissionDenied { endpoint },
            _ => {
                tracing::error!(error = ?err, %endpoint, "unclassified Docker error");
                Self::Internal {
                    message: err.to_string(),
                }
            }
        }
    }
}

/// Bollard wraps transport errors, so the `io::Error` can sit anywhere in the chain.
fn io_kind_in_chain(err: &(dyn StdError + 'static)) -> Option<io::ErrorKind> {
    let mut current = Some(err);
    while let Some(e) = current {
        if let Some(io) = e.downcast_ref::<io::Error>() {
            return Some(io.kind());
        }
        current = e.source();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const EP: &str = "unix:///var/run/docker.sock";

    fn io_err(kind: io::ErrorKind) -> BollardError {
        BollardError::IOError {
            err: io::Error::from(kind),
        }
    }

    #[test]
    fn socket_not_found_is_daemon_unavailable() {
        let err = BollardError::SocketNotFoundError("/var/run/docker.sock".into());
        assert!(matches!(
            DockerError::from_bollard(&err, EP),
            DockerError::DaemonUnavailable { endpoint } if endpoint == EP
        ));
    }

    #[test]
    fn io_not_found_and_refused_are_daemon_unavailable() {
        for kind in [io::ErrorKind::NotFound, io::ErrorKind::ConnectionRefused] {
            assert!(matches!(
                DockerError::from_bollard(&io_err(kind), EP),
                DockerError::DaemonUnavailable { .. }
            ));
        }
    }

    #[test]
    fn io_permission_denied_is_permission_denied() {
        assert!(matches!(
            DockerError::from_bollard(&io_err(io::ErrorKind::PermissionDenied), EP),
            DockerError::PermissionDenied { endpoint } if endpoint == EP
        ));
    }

    #[derive(Debug, thiserror::Error)]
    #[error("wrapper")]
    struct Wrapper(#[source] io::Error);

    #[test]
    fn io_kind_is_found_deeper_in_the_source_chain() {
        let err = Wrapper(io::Error::from(io::ErrorKind::PermissionDenied));
        assert_eq!(
            io_kind_in_chain(&err),
            Some(io::ErrorKind::PermissionDenied)
        );
    }

    #[test]
    fn timeout_is_timeout() {
        assert!(matches!(
            DockerError::from_bollard(&BollardError::RequestTimeoutError, EP),
            DockerError::Timeout
        ));
    }

    fn server(status_code: u16, message: &str) -> BollardError {
        BollardError::DockerResponseServerError {
            status_code,
            message: message.into(),
        }
    }

    #[test]
    fn http_404_is_not_found() {
        assert!(matches!(
            DockerError::from_bollard(&server(404, "No such container: x"), EP),
            DockerError::NotFound
        ));
    }

    #[test]
    fn http_409_is_conflict_with_docker_message() {
        assert!(matches!(
            DockerError::from_bollard(&server(409, "container is paused"), EP),
            DockerError::Conflict { message } if message == "container is paused"
        ));
    }

    #[test]
    fn other_server_errors_are_internal_with_docker_message() {
        assert!(matches!(
            DockerError::from_bollard(&server(500, "boom"), EP),
            DockerError::Internal { message } if message == "boom"
        ));
    }

    #[test]
    fn other_io_errors_are_internal() {
        assert!(matches!(
            DockerError::from_bollard(&io_err(io::ErrorKind::BrokenPipe), EP),
            DockerError::Internal { .. }
        ));
    }

    #[test]
    fn only_transport_failures_are_connection_level() {
        let endpoint = || EP.to_owned();
        assert!(
            DockerError::DaemonUnavailable {
                endpoint: endpoint()
            }
            .is_connection_level()
        );
        assert!(
            DockerError::PermissionDenied {
                endpoint: endpoint()
            }
            .is_connection_level()
        );
        assert!(DockerError::Timeout.is_connection_level());
        assert!(!DockerError::NotFound.is_connection_level());
        assert!(
            !DockerError::Internal {
                message: String::new()
            }
            .is_connection_level()
        );
    }

    #[test]
    fn permission_hint_never_suggests_root() {
        let hint = DockerError::PermissionDenied {
            endpoint: EP.into(),
        }
        .hint()
        .unwrap();
        assert!(hint.contains("docker"));
        assert!(hint.to_lowercase().contains("rootless"));
        assert!(!hint.contains("sudo"));
    }

    #[test]
    fn invalid_configuration_hint_names_the_file() {
        let err = DockerError::InvalidConfiguration {
            context: Some("work".into()),
            path: "/home/u/.docker/config.json".into(),
            detail: "expected value".into(),
        };
        assert_eq!(
            err.to_string(),
            "Docker context work is not configured correctly."
        );
        assert!(err.hint().unwrap().contains("/home/u/.docker/config.json"));
    }

    #[test]
    fn not_found_has_no_hint() {
        assert_eq!(DockerError::NotFound.hint(), None);
    }
}

#[cfg(test)]
mod ipc_tests {
    use super::*;

    #[test]
    fn converts_to_ipc_error_with_kind_message_and_hint() {
        let ipc = IpcError::from(DockerError::DaemonUnavailable {
            endpoint: "unix:///var/run/docker.sock".into(),
        });
        assert_eq!(ipc.kind, ErrorKind::DaemonUnavailable);
        assert_eq!(
            ipc.message,
            "Docker is not running at unix:///var/run/docker.sock."
        );
        assert!(ipc.hint.unwrap().contains("systemctl"));
    }

    #[test]
    fn serializes_camel_case_kind_and_omits_missing_hint() {
        let json = serde_json::to_value(IpcError::from(DockerError::NotFound)).unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "kind": "notFound", "message": "Container no longer exists." })
        );
    }
}
