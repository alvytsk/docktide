//! Container list (spec M0 FR-3).

use bollard::models::{
    ContainerSummary as DockerContainer, ContainerSummaryStateEnum as DockerState, PortSummary,
    PortSummaryTypeEnum,
};
use bollard::query_parameters::ListContainersOptions;
use serde::Serialize;

use crate::connection::{DockerService, LIFECYCLE_TIMEOUT};
use crate::error::DockerError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum ContainerState {
    Created,
    Running,
    Paused,
    Restarting,
    Removing,
    Exited,
    Dead,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum Protocol {
    Tcp,
    Udp,
    Sctp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct PortMapping {
    pub private_port: u16,
    pub public_port: Option<u16>,
    /// No IP means all interfaces.
    pub host_ip: Option<String>,
    pub protocol: Protocol,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ContainerSummary {
    pub id: String,
    pub name: String,
    pub image: String,
    pub state: ContainerState,
    /// Docker's text, e.g. "Up 3 hours (healthy)".
    pub status: String,
    pub ports: Vec<PortMapping>,
    /// Unix seconds.
    #[cfg_attr(test, ts(type = "number"))]
    pub created_at: i64,
}

impl DockerService {
    /// All containers, running first, then by name.
    pub async fn list_containers(&self) -> Result<Vec<ContainerSummary>, DockerError> {
        let (endpoint, docker) = self.docker().await?;
        let options = ListContainersOptions {
            all: true,
            ..Default::default()
        };
        let containers = docker
            .list_containers(Some(options))
            .await
            .map_err(|err| self.failed(&err, &endpoint))?;
        Ok(sorted(
            containers
                .into_iter()
                .map(ContainerSummary::from_docker)
                .collect(),
        ))
    }

    /// Docker answers 304 for an already started container, which Bollard treats as success.
    pub async fn start_container(&self, id: &str) -> Result<(), DockerError> {
        let (endpoint, docker) = self.docker().await?;
        docker
            .start_container(id, None)
            .await
            .map_err(|err| self.failed(&err, &endpoint))
    }

    /// Uses the Engine's default timeout: the container's `StopTimeout`, or 10 s.
    pub async fn stop_container(&self, id: &str) -> Result<(), DockerError> {
        let (endpoint, docker) = self.docker().await?;
        docker
            .with_timeout(LIFECYCLE_TIMEOUT)
            .stop_container(id, None)
            .await
            .map_err(|err| self.failed(&err, &endpoint))
    }

    /// Uses the Engine's default timeout, like `stop_container`.
    pub async fn restart_container(&self, id: &str) -> Result<(), DockerError> {
        let (endpoint, docker) = self.docker().await?;
        docker
            .with_timeout(LIFECYCLE_TIMEOUT)
            .restart_container(id, None)
            .await
            .map_err(|err| self.failed(&err, &endpoint))
    }
}

impl ContainerSummary {
    fn from_docker(container: DockerContainer) -> Self {
        let id = container.id.unwrap_or_default();
        let name = container
            .names
            .and_then(|names| names.into_iter().next())
            .map_or_else(
                || id.chars().take(12).collect(),
                |name| name.trim_start_matches('/').to_owned(),
            );
        let state = match container.state {
            Some(DockerState::CREATED) => ContainerState::Created,
            Some(DockerState::RUNNING) => ContainerState::Running,
            Some(DockerState::PAUSED) => ContainerState::Paused,
            Some(DockerState::RESTARTING) => ContainerState::Restarting,
            Some(DockerState::REMOVING) => ContainerState::Removing,
            Some(DockerState::EXITED) => ContainerState::Exited,
            Some(DockerState::DEAD) => ContainerState::Dead,
            Some(DockerState::EMPTY | DockerState::STOPPING) | None => ContainerState::Unknown,
        };
        Self {
            id,
            name,
            image: container.image.unwrap_or_default(),
            state,
            status: container.status.unwrap_or_default(),
            ports: ports(container.ports.unwrap_or_default()),
            created_at: container.created.unwrap_or_default(),
        }
    }
}

fn ports(ports: Vec<PortSummary>) -> Vec<PortMapping> {
    normalized(
        ports
            .into_iter()
            .map(|port| PortMapping {
                private_port: port.private_port,
                public_port: port.public_port,
                host_ip: port.ip,
                protocol: match port.typ {
                    Some(PortSummaryTypeEnum::UDP) => Protocol::Udp,
                    Some(PortSummaryTypeEnum::SCTP) => Protocol::Sctp,
                    _ => Protocol::Tcp,
                },
            })
            .collect(),
    )
}

/// Docker reports each published port once per IP family, so wildcard IPs are
/// normalized to `None` and the duplicates dropped.
pub(crate) fn normalized(mut ports: Vec<PortMapping>) -> Vec<PortMapping> {
    for port in &mut ports {
        port.host_ip = port
            .host_ip
            .take()
            .filter(|ip| !matches!(ip.as_str(), "" | "0.0.0.0" | "::"));
    }
    ports.sort_by(|a, b| {
        (a.private_port, a.protocol, a.public_port, &a.host_ip).cmp(&(
            b.private_port,
            b.protocol,
            b.public_port,
            &b.host_ip,
        ))
    });
    ports.dedup();
    ports
}

fn sorted(mut containers: Vec<ContainerSummary>) -> Vec<ContainerSummary> {
    containers.sort_by_cached_key(|c| (c.state != ContainerState::Running, c.name.to_lowercase()));
    containers
}

#[cfg(test)]
mod tests {
    use super::*;

    fn container(names: Option<&[&str]>, state: Option<DockerState>) -> DockerContainer {
        DockerContainer {
            id: Some("0123456789abcdef0123".into()),
            names: names.map(|names| names.iter().map(|&n| n.into()).collect()),
            state,
            ..Default::default()
        }
    }

    fn port(ip: Option<&str>, private: u16, public: Option<u16>, typ: &str) -> PortSummary {
        PortSummary {
            ip: ip.map(Into::into),
            private_port: private,
            public_port: public,
            typ: serde_json::from_value(serde_json::json!(typ)).unwrap(),
        }
    }

    fn mapping(private: u16, public: Option<u16>, ip: Option<&str>, p: Protocol) -> PortMapping {
        PortMapping {
            private_port: private,
            public_port: public,
            host_ip: ip.map(Into::into),
            protocol: p,
        }
    }

    #[test]
    fn name_drops_the_leading_slash() {
        let c = ContainerSummary::from_docker(container(Some(&["/web"]), None));
        assert_eq!(c.name, "web");
    }

    #[test]
    fn missing_names_fall_back_to_the_short_id() {
        for names in [None, Some(&[][..])] {
            let c = ContainerSummary::from_docker(container(names, None));
            assert_eq!(c.name, "0123456789ab");
            assert_eq!(c.id, "0123456789abcdef0123");
        }
    }

    #[test]
    fn states_map_and_unknown_values_become_unknown() {
        let cases = [
            (DockerState::RUNNING, ContainerState::Running),
            (DockerState::EXITED, ContainerState::Exited),
            (DockerState::PAUSED, ContainerState::Paused),
            (DockerState::EMPTY, ContainerState::Unknown),
            (DockerState::STOPPING, ContainerState::Unknown),
        ];
        for (docker, expected) in cases {
            let c = ContainerSummary::from_docker(container(None, Some(docker)));
            assert_eq!(c.state, expected);
        }
        let c = ContainerSummary::from_docker(container(None, None));
        assert_eq!(c.state, ContainerState::Unknown);
    }

    #[test]
    fn wildcard_ips_are_normalized_and_deduplicated() {
        let mapped = ports(vec![
            port(Some("0.0.0.0"), 80, Some(8080), "tcp"),
            port(Some("::"), 80, Some(8080), "tcp"),
            port(Some("127.0.0.1"), 5432, Some(5432), "tcp"),
        ]);
        assert_eq!(
            mapped,
            [
                mapping(80, Some(8080), None, Protocol::Tcp),
                mapping(5432, Some(5432), Some("127.0.0.1"), Protocol::Tcp),
            ]
        );
    }

    #[test]
    fn ports_sort_by_private_port_protocol_then_public_port() {
        let mapped = ports(vec![
            port(None, 443, None, "tcp"),
            port(Some("0.0.0.0"), 53, Some(5353), "udp"),
            port(Some("0.0.0.0"), 53, Some(5300), "tcp"),
            port(Some("0.0.0.0"), 53, Some(53), "tcp"),
        ]);
        assert_eq!(
            mapped,
            [
                mapping(53, Some(53), None, Protocol::Tcp),
                mapping(53, Some(5300), None, Protocol::Tcp),
                mapping(53, Some(5353), None, Protocol::Udp),
                mapping(443, None, None, Protocol::Tcp),
            ]
        );
    }

    #[test]
    fn running_first_then_name_case_insensitive() {
        let summary = |name: &str, state| ContainerSummary {
            name: name.into(),
            state,
            ..ContainerSummary::from_docker(DockerContainer::default())
        };
        let list = sorted(vec![
            summary("beta", ContainerState::Exited),
            summary("Zed", ContainerState::Running),
            summary("Alpha", ContainerState::Exited),
            summary("api", ContainerState::Running),
        ]);
        let names: Vec<_> = list.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["api", "Zed", "Alpha", "beta"]);
    }

    #[test]
    fn serializes_to_the_ipc_shape() {
        let c = ContainerSummary {
            ports: vec![mapping(80, None, None, Protocol::Udp)],
            created_at: 1_700_000_000,
            ..ContainerSummary::from_docker(container(Some(&["/web"]), Some(DockerState::RUNNING)))
        };
        assert_eq!(
            serde_json::to_value(c).unwrap(),
            serde_json::json!({
                "id": "0123456789abcdef0123",
                "name": "web",
                "image": "",
                "state": "running",
                "status": "",
                "ports": [{ "privatePort": 80, "publicPort": null, "hostIp": null, "protocol": "udp" }],
                "createdAt": 1_700_000_000,
            })
        );
    }
}
