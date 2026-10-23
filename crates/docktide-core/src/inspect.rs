//! Container details from `inspect` (spec M1 FR-3).

use std::collections::HashMap;

use bollard::models::{
    ContainerInspectResponse, ContainerStateStatusEnum as DockerState, EndpointSettings,
    HealthStatusEnum, MountPoint, PortBinding, RestartPolicy, RestartPolicyNameEnum,
};
use bollard::query_parameters::InspectContainerOptions;
use serde::Serialize;

use crate::connection::DockerService;
use crate::containers::{ContainerState, PortMapping, Protocol, normalized};
use crate::error::DockerError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum HealthStatus {
    Starting,
    Healthy,
    Unhealthy,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ContainerHealth {
    pub status: HealthStatus,
    pub failing_streak: u32,
    /// Output of the most recent check, trimmed.
    pub last_output: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum MountType {
    Bind,
    Volume,
    Tmpfs,
    Npipe,
    Cluster,
    Image,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct MountInfo {
    #[serde(rename = "type")]
    #[cfg_attr(test, ts(rename = "type"))]
    pub typ: MountType,
    /// Volume name.
    pub name: Option<String>,
    pub source: String,
    pub destination: String,
    pub read_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct NetworkAttachment {
    pub name: String,
    pub ip_address: Option<String>,
    pub ipv6_address: Option<String>,
    pub gateway: Option<String>,
    pub mac_address: Option<String>,
    pub aliases: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct KeyValue {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ContainerDetails {
    pub id: String,
    pub name: String,
    /// `Config.Image`, as the user asked for it.
    pub image: String,
    /// `sha256:…`
    pub image_id: String,
    pub state: ContainerState,
    /// RFC 3339.
    pub created_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    /// Unset until the container has stopped at least once.
    #[cfg_attr(test, ts(type = "number | null"))]
    pub exit_code: Option<i64>,
    pub oom_killed: bool,
    /// `State.Error`, when non-empty.
    pub error: Option<String>,
    /// Unset when no healthcheck is configured.
    pub health: Option<ContainerHealth>,
    pub entrypoint: Vec<String>,
    pub command: Vec<String>,
    pub working_dir: Option<String>,
    pub user: Option<String>,
    pub tty: bool,
    /// `no`, `always`, `unless-stopped`, or `on-failure[:N]`.
    pub restart_policy: String,
    pub restart_count: u32,
    pub ports: Vec<PortMapping>,
    pub mounts: Vec<MountInfo>,
    pub networks: Vec<NetworkAttachment>,
    pub env: Vec<KeyValue>,
    pub labels: Vec<KeyValue>,
    /// Bytes; unset when unlimited.
    #[cfg_attr(test, ts(type = "number | null"))]
    pub memory_limit: Option<u64>,
    /// CPUs (`NanoCpus` / 1e9); unset when unlimited.
    pub cpu_limit: Option<f64>,
    pub log_driver: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ContainerInspect {
    pub details: ContainerDetails,
    /// Bollard's model re-serialized, with Docker's field names (spec M1 §9).
    #[cfg_attr(test, ts(type = "unknown"))]
    pub raw: serde_json::Value,
}

impl DockerService {
    /// One `GET /containers/{id}/json`, without sizes (computing them is slow).
    pub async fn inspect_container(&self, id: &str) -> Result<ContainerInspect, DockerError> {
        let (endpoint, docker) = self.docker().await?;
        let response = docker
            .inspect_container(id, Some(InspectContainerOptions { size: false }))
            .await
            .map_err(|err| self.failed(&err, &endpoint))?;
        let raw = serde_json::to_value(&response).map_err(|err| DockerError::Internal {
            message: format!("Could not encode the inspect result: {err}"),
        })?;
        Ok(ContainerInspect {
            details: ContainerDetails::from_docker(response),
            raw,
        })
    }
}

impl ContainerDetails {
    fn from_docker(inspect: ContainerInspectResponse) -> Self {
        let state = inspect.state.unwrap_or_default();
        let config = inspect.config.unwrap_or_default();
        let host_config = inspect.host_config.unwrap_or_default();
        let network = inspect.network_settings.unwrap_or_default();
        let finished_at = time(state.finished_at);
        Self {
            id: inspect.id.unwrap_or_default(),
            name: inspect
                .name
                .unwrap_or_default()
                .trim_start_matches('/')
                .to_owned(),
            image: config.image.unwrap_or_default(),
            image_id: inspect.image.unwrap_or_default(),
            state: match state.status {
                Some(DockerState::CREATED) => ContainerState::Created,
                Some(DockerState::RUNNING) => ContainerState::Running,
                Some(DockerState::PAUSED) => ContainerState::Paused,
                Some(DockerState::RESTARTING) => ContainerState::Restarting,
                Some(DockerState::REMOVING) => ContainerState::Removing,
                Some(DockerState::EXITED) => ContainerState::Exited,
                Some(DockerState::DEAD) => ContainerState::Dead,
                Some(DockerState::EMPTY | DockerState::STOPPING) | None => ContainerState::Unknown,
            },
            created_at: inspect.created.unwrap_or_default(),
            started_at: time(state.started_at),
            exit_code: finished_at.as_ref().and(state.exit_code),
            finished_at,
            oom_killed: state.oom_killed.unwrap_or_default(),
            error: non_empty(state.error),
            health: state.health.and_then(|health| {
                let status = match health.status? {
                    HealthStatusEnum::NONE => return None,
                    HealthStatusEnum::STARTING => HealthStatus::Starting,
                    HealthStatusEnum::HEALTHY => HealthStatus::Healthy,
                    HealthStatusEnum::UNHEALTHY => HealthStatus::Unhealthy,
                    HealthStatusEnum::EMPTY => HealthStatus::Unknown,
                };
                Some(ContainerHealth {
                    status,
                    failing_streak: count(health.failing_streak),
                    last_output: health
                        .log
                        .and_then(|log| log.into_iter().last())
                        .and_then(|check| non_empty(check.output.map(|o| o.trim().to_owned()))),
                })
            }),
            entrypoint: config.entrypoint.unwrap_or_default(),
            command: config.cmd.unwrap_or_default(),
            working_dir: non_empty(config.working_dir),
            user: non_empty(config.user),
            tty: config.tty.unwrap_or_default(),
            restart_policy: restart_policy(&host_config.restart_policy.unwrap_or_default()),
            restart_count: count(inspect.restart_count),
            ports: ports(network.ports.unwrap_or_default()),
            mounts: mounts(inspect.mounts.unwrap_or_default()),
            networks: networks(network.networks.unwrap_or_default()),
            env: config
                .env
                .unwrap_or_default()
                .into_iter()
                .map(|entry| match entry.split_once('=') {
                    Some((key, value)) => KeyValue {
                        key: key.to_owned(),
                        value: value.to_owned(),
                    },
                    None => KeyValue {
                        key: entry,
                        value: String::new(),
                    },
                })
                .collect(),
            labels: {
                let mut labels: Vec<_> = config
                    .labels
                    .unwrap_or_default()
                    .into_iter()
                    .map(|(key, value)| KeyValue { key, value })
                    .collect();
                labels.sort_by(|a, b| a.key.cmp(&b.key));
                labels
            },
            memory_limit: host_config
                .memory
                .and_then(|bytes| u64::try_from(bytes).ok())
                .filter(|&bytes| bytes > 0),
            #[expect(clippy::cast_precision_loss, reason = "CPU counts are far below 2^52")]
            cpu_limit: host_config
                .nano_cpus
                .filter(|&nanos| nanos > 0)
                .map(|nanos| nanos as f64 / 1e9),
            log_driver: host_config
                .log_config
                .and_then(|log| log.typ)
                .unwrap_or_default(),
        }
    }
}

/// Docker reports "never" as its zero time.
fn time(value: Option<String>) -> Option<String> {
    non_empty(value).filter(|t| t != "0001-01-01T00:00:00Z")
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|v| !v.is_empty())
}

fn count(value: Option<i64>) -> u32 {
    value.map_or(0, |n| u32::try_from(n).unwrap_or(u32::MAX))
}

fn restart_policy(policy: &RestartPolicy) -> String {
    match policy.name {
        Some(RestartPolicyNameEnum::ALWAYS) => "always".to_owned(),
        Some(RestartPolicyNameEnum::UNLESS_STOPPED) => "unless-stopped".to_owned(),
        Some(RestartPolicyNameEnum::ON_FAILURE) => match policy.maximum_retry_count {
            Some(retries) if retries > 0 => format!("on-failure:{retries}"),
            _ => "on-failure".to_owned(),
        },
        Some(RestartPolicyNameEnum::NO | RestartPolicyNameEnum::EMPTY) | None => "no".to_owned(),
    }
}

/// `NetworkSettings.Ports` keys are `<port>/<protocol>`; an unpublished port has no bindings.
fn ports(ports: HashMap<String, Option<Vec<PortBinding>>>) -> Vec<PortMapping> {
    let mut mappings = Vec::new();
    for (key, bindings) in ports {
        let (port, protocol) = key.split_once('/').unwrap_or((&key, "tcp"));
        let Ok(private_port) = port.parse() else {
            continue;
        };
        let protocol = match protocol {
            "udp" => Protocol::Udp,
            "sctp" => Protocol::Sctp,
            _ => Protocol::Tcp,
        };
        let bindings = bindings.unwrap_or_default();
        if bindings.is_empty() {
            mappings.push(PortMapping {
                private_port,
                public_port: None,
                host_ip: None,
                protocol,
            });
        }
        for binding in bindings {
            mappings.push(PortMapping {
                private_port,
                public_port: binding.host_port.and_then(|p| p.parse().ok()),
                host_ip: binding.host_ip,
                protocol,
            });
        }
    }
    normalized(mappings)
}

fn mounts(mounts: Vec<MountPoint>) -> Vec<MountInfo> {
    let mut mounts: Vec<_> = mounts
        .into_iter()
        .map(|mount| MountInfo {
            typ: match mount.typ.as_deref() {
                Some("bind") => MountType::Bind,
                Some("volume") => MountType::Volume,
                Some("tmpfs") => MountType::Tmpfs,
                Some("npipe") => MountType::Npipe,
                Some("cluster") => MountType::Cluster,
                Some("image") => MountType::Image,
                _ => MountType::Unknown,
            },
            name: non_empty(mount.name),
            source: mount.source.unwrap_or_default(),
            destination: mount.destination.unwrap_or_default(),
            read_only: !mount.rw.unwrap_or(true),
        })
        .collect();
    mounts.sort_by(|a, b| a.destination.cmp(&b.destination));
    mounts
}

fn networks(networks: HashMap<String, EndpointSettings>) -> Vec<NetworkAttachment> {
    let mut networks: Vec<_> = networks
        .into_iter()
        .map(|(name, endpoint)| NetworkAttachment {
            name,
            ip_address: non_empty(endpoint.ip_address),
            ipv6_address: non_empty(endpoint.global_ipv6_address),
            gateway: non_empty(endpoint.gateway),
            mac_address: non_empty(endpoint.mac_address),
            aliases: endpoint.aliases.unwrap_or_default(),
        })
        .collect();
    networks.sort_by(|a, b| a.name.cmp(&b.name));
    networks
}

#[cfg(test)]
mod tests {
    use bollard::models::{
        ContainerConfig, ContainerState as DockerContainerState, Health, HealthcheckResult,
        HostConfig, HostConfigLogConfig, NetworkSettings,
    };
    use serde_json::json;

    use super::*;

    const ZERO_TIME: &str = "0001-01-01T00:00:00Z";

    fn inspect() -> ContainerInspectResponse {
        ContainerInspectResponse {
            id: Some("0123456789abcdef".into()),
            name: Some("/web".into()),
            image: Some("sha256:abc".into()),
            created: Some("2026-10-21T10:00:00.123456789Z".into()),
            ..Default::default()
        }
    }

    fn with_state(state: DockerContainerState) -> ContainerDetails {
        ContainerDetails::from_docker(ContainerInspectResponse {
            state: Some(state),
            ..inspect()
        })
    }

    fn with_config(config: ContainerConfig) -> ContainerDetails {
        ContainerDetails::from_docker(ContainerInspectResponse {
            config: Some(config),
            ..inspect()
        })
    }

    fn with_host_config(host_config: HostConfig) -> ContainerDetails {
        ContainerDetails::from_docker(ContainerInspectResponse {
            host_config: Some(host_config),
            ..inspect()
        })
    }

    fn kv(key: &str, value: &str) -> KeyValue {
        KeyValue {
            key: key.into(),
            value: value.into(),
        }
    }

    #[test]
    fn identity_fields_map_and_the_name_loses_its_slash() {
        let d = ContainerDetails::from_docker(ContainerInspectResponse {
            config: Some(ContainerConfig {
                image: Some("nginx:1.27".into()),
                ..Default::default()
            }),
            ..inspect()
        });
        assert_eq!(d.id, "0123456789abcdef");
        assert_eq!(d.name, "web");
        assert_eq!(d.image, "nginx:1.27");
        assert_eq!(d.image_id, "sha256:abc");
        assert_eq!(d.created_at, "2026-10-21T10:00:00.123456789Z");
    }

    #[test]
    fn states_map_like_the_list() {
        let cases = [
            (DockerState::RUNNING, ContainerState::Running),
            (DockerState::EXITED, ContainerState::Exited),
            (DockerState::CREATED, ContainerState::Created),
            (DockerState::STOPPING, ContainerState::Unknown),
        ];
        for (status, expected) in cases {
            let d = with_state(DockerContainerState {
                status: Some(status),
                ..Default::default()
            });
            assert_eq!(d.state, expected);
        }
        assert_eq!(
            with_state(DockerContainerState::default()).state,
            ContainerState::Unknown
        );
    }

    #[test]
    fn zero_times_become_null_and_exit_code_needs_a_finish() {
        let never_ran = with_state(DockerContainerState {
            started_at: Some(ZERO_TIME.into()),
            finished_at: Some(ZERO_TIME.into()),
            exit_code: Some(0),
            ..Default::default()
        });
        assert_eq!(never_ran.started_at, None);
        assert_eq!(never_ran.finished_at, None);
        assert_eq!(never_ran.exit_code, None);

        let stopped = with_state(DockerContainerState {
            started_at: Some("2026-10-21T10:00:01Z".into()),
            finished_at: Some("2026-10-21T10:05:00Z".into()),
            exit_code: Some(137),
            oom_killed: Some(true),
            error: Some(String::new()),
            ..Default::default()
        });
        assert_eq!(stopped.started_at.as_deref(), Some("2026-10-21T10:00:01Z"));
        assert_eq!(stopped.finished_at.as_deref(), Some("2026-10-21T10:05:00Z"));
        assert_eq!(stopped.exit_code, Some(137));
        assert!(stopped.oom_killed);
        assert_eq!(stopped.error, None);

        let failed = with_state(DockerContainerState {
            error: Some("port is already allocated".into()),
            ..Default::default()
        });
        assert_eq!(failed.error.as_deref(), Some("port is already allocated"));
    }

    #[test]
    fn health_is_null_without_a_healthcheck() {
        assert_eq!(with_state(DockerContainerState::default()).health, None);
        let none = with_state(DockerContainerState {
            health: Some(Health {
                status: Some(HealthStatusEnum::NONE),
                ..Default::default()
            }),
            ..Default::default()
        });
        assert_eq!(none.health, None);
    }

    #[test]
    fn health_reports_the_last_check_output_trimmed() {
        let check = |output: &str| HealthcheckResult {
            output: Some(output.into()),
            ..Default::default()
        };
        let d = with_state(DockerContainerState {
            health: Some(Health {
                status: Some(HealthStatusEnum::UNHEALTHY),
                failing_streak: Some(3),
                log: Some(vec![check("ok\n"), check("  connection refused\n")]),
            }),
            ..Default::default()
        });
        assert_eq!(
            d.health,
            Some(ContainerHealth {
                status: HealthStatus::Unhealthy,
                failing_streak: 3,
                last_output: Some("connection refused".into()),
            })
        );

        let starting = with_state(DockerContainerState {
            health: Some(Health {
                status: Some(HealthStatusEnum::STARTING),
                log: Some(vec![check(" \n")]),
                ..Default::default()
            }),
            ..Default::default()
        });
        assert_eq!(
            starting.health,
            Some(ContainerHealth {
                status: HealthStatus::Starting,
                failing_streak: 0,
                last_output: None,
            })
        );
    }

    #[test]
    fn process_fields_map_and_empty_strings_become_null() {
        let d = with_config(ContainerConfig {
            entrypoint: Some(vec!["/docker-entrypoint.sh".into()]),
            cmd: Some(vec!["nginx".into(), "-g".into(), "daemon off;".into()]),
            working_dir: Some(String::new()),
            user: Some("1000:1000".into()),
            tty: Some(true),
            ..Default::default()
        });
        assert_eq!(d.entrypoint, ["/docker-entrypoint.sh"]);
        assert_eq!(d.command, ["nginx", "-g", "daemon off;"]);
        assert_eq!(d.working_dir, None);
        assert_eq!(d.user.as_deref(), Some("1000:1000"));
        assert!(d.tty);

        let bare = with_config(ContainerConfig::default());
        assert!(bare.entrypoint.is_empty() && bare.command.is_empty());
        assert_eq!(bare.user, None);
        assert!(!bare.tty);
    }

    #[test]
    fn env_splits_at_the_first_equals_and_keeps_docker_order() {
        let d = with_config(ContainerConfig {
            env: Some(vec![
                "PATH=/usr/bin:/bin".into(),
                "DSN=postgres://u:p@db/x?a=b".into(),
                "FLAG".into(),
                "EMPTY=".into(),
            ]),
            ..Default::default()
        });
        assert_eq!(
            d.env,
            [
                kv("PATH", "/usr/bin:/bin"),
                kv("DSN", "postgres://u:p@db/x?a=b"),
                kv("FLAG", ""),
                kv("EMPTY", ""),
            ]
        );
    }

    #[test]
    fn labels_are_sorted_by_key() {
        let d = with_config(ContainerConfig {
            labels: Some(HashMap::from([
                ("org.opencontainers.image.version".into(), "1.27".into()),
                ("com.docker.compose.service".into(), "web".into()),
                ("maintainer".into(), "nginx".into()),
            ])),
            ..Default::default()
        });
        assert_eq!(
            d.labels,
            [
                kv("com.docker.compose.service", "web"),
                kv("maintainer", "nginx"),
                kv("org.opencontainers.image.version", "1.27"),
            ]
        );
    }

    #[test]
    fn restart_policy_text() {
        let policy = |name, retries| {
            with_host_config(HostConfig {
                restart_policy: Some(RestartPolicy {
                    name,
                    maximum_retry_count: retries,
                }),
                ..Default::default()
            })
            .restart_policy
        };
        assert_eq!(policy(Some(RestartPolicyNameEnum::NO), None), "no");
        assert_eq!(policy(Some(RestartPolicyNameEnum::EMPTY), None), "no");
        assert_eq!(policy(None, None), "no");
        assert_eq!(
            policy(Some(RestartPolicyNameEnum::ALWAYS), Some(0)),
            "always"
        );
        assert_eq!(
            policy(Some(RestartPolicyNameEnum::UNLESS_STOPPED), None),
            "unless-stopped"
        );
        assert_eq!(
            policy(Some(RestartPolicyNameEnum::ON_FAILURE), Some(0)),
            "on-failure"
        );
        assert_eq!(
            policy(Some(RestartPolicyNameEnum::ON_FAILURE), Some(5)),
            "on-failure:5"
        );
        assert_eq!(with_host_config(HostConfig::default()).restart_policy, "no");
    }

    #[test]
    fn limits_are_null_when_unlimited() {
        let unlimited = with_host_config(HostConfig {
            memory: Some(0),
            nano_cpus: Some(0),
            ..Default::default()
        });
        assert_eq!(unlimited.memory_limit, None);
        assert_eq!(unlimited.cpu_limit, None);

        let limited = with_host_config(HostConfig {
            memory: Some(512 * 1024 * 1024),
            nano_cpus: Some(1_500_000_000),
            log_config: Some(HostConfigLogConfig {
                typ: Some("json-file".into()),
                config: None,
            }),
            ..Default::default()
        });
        assert_eq!(limited.memory_limit, Some(512 * 1024 * 1024));
        assert_eq!(limited.cpu_limit, Some(1.5));
        assert_eq!(limited.log_driver, "json-file");
    }

    #[test]
    fn restart_count_maps() {
        let d = ContainerDetails::from_docker(ContainerInspectResponse {
            restart_count: Some(4),
            ..inspect()
        });
        assert_eq!(d.restart_count, 4);
    }

    #[test]
    fn ports_are_normalized_like_the_list() {
        let binding = |ip: &str, port: &str| PortBinding {
            host_ip: Some(ip.into()),
            host_port: Some(port.into()),
        };
        let d = ContainerDetails::from_docker(ContainerInspectResponse {
            network_settings: Some(NetworkSettings {
                ports: Some(HashMap::from([
                    (
                        "80/tcp".into(),
                        Some(vec![binding("0.0.0.0", "8080"), binding("::", "8080")]),
                    ),
                    ("53/udp".into(), Some(vec![binding("127.0.0.1", "5353")])),
                    ("443/tcp".into(), None),
                    ("9000/sctp".into(), Some(vec![])),
                ])),
                ..Default::default()
            }),
            ..inspect()
        });
        let mapping = |private, public, ip: Option<&str>, protocol| PortMapping {
            private_port: private,
            public_port: public,
            host_ip: ip.map(Into::into),
            protocol,
        };
        assert_eq!(
            d.ports,
            [
                mapping(53, Some(5353), Some("127.0.0.1"), Protocol::Udp),
                mapping(80, Some(8080), None, Protocol::Tcp),
                mapping(443, None, None, Protocol::Tcp),
                mapping(9000, None, None, Protocol::Sctp),
            ]
        );
    }

    #[test]
    fn mounts_map_types_and_sort_by_destination() {
        let mount =
            |typ: &str, name: Option<&str>, source: &str, destination: &str, rw| MountPoint {
                typ: Some(typ.into()),
                name: name.map(Into::into),
                source: Some(source.into()),
                destination: Some(destination.into()),
                rw: Some(rw),
                ..Default::default()
            };
        let d = ContainerDetails::from_docker(ContainerInspectResponse {
            mounts: Some(vec![
                mount(
                    "volume",
                    Some("pgdata"),
                    "/var/lib/docker/volumes/pgdata/_data",
                    "/var/lib/postgresql/data",
                    true,
                ),
                mount("bind", None, "/home/u/conf", "/etc/app", false),
                mount("tmpfs", None, "", "/run/cache", true),
                mount("something-new", None, "x", "/x", true),
            ]),
            ..inspect()
        });
        let info =
            |typ, name: Option<&str>, source: &str, destination: &str, read_only| MountInfo {
                typ,
                name: name.map(Into::into),
                source: source.into(),
                destination: destination.into(),
                read_only,
            };
        assert_eq!(
            d.mounts,
            [
                info(MountType::Bind, None, "/home/u/conf", "/etc/app", true),
                info(MountType::Tmpfs, None, "", "/run/cache", false),
                info(
                    MountType::Volume,
                    Some("pgdata"),
                    "/var/lib/docker/volumes/pgdata/_data",
                    "/var/lib/postgresql/data",
                    false
                ),
                info(MountType::Unknown, None, "x", "/x", false),
            ]
        );
    }

    #[test]
    fn networks_sort_by_name_and_blank_addresses_become_null() {
        let d = ContainerDetails::from_docker(ContainerInspectResponse {
            network_settings: Some(NetworkSettings {
                networks: Some(HashMap::from([
                    (
                        "web_default".into(),
                        EndpointSettings {
                            ip_address: Some("172.18.0.2".into()),
                            global_ipv6_address: Some(String::new()),
                            gateway: Some("172.18.0.1".into()),
                            mac_address: Some("02:42:ac:12:00:02".into()),
                            aliases: Some(vec!["web".into()]),
                            ..Default::default()
                        },
                    ),
                    ("bridge".into(), EndpointSettings::default()),
                ])),
                ..Default::default()
            }),
            ..inspect()
        });
        assert_eq!(
            d.networks,
            [
                NetworkAttachment {
                    name: "bridge".into(),
                    ip_address: None,
                    ipv6_address: None,
                    gateway: None,
                    mac_address: None,
                    aliases: vec![],
                },
                NetworkAttachment {
                    name: "web_default".into(),
                    ip_address: Some("172.18.0.2".into()),
                    ipv6_address: None,
                    gateway: Some("172.18.0.1".into()),
                    mac_address: Some("02:42:ac:12:00:02".into()),
                    aliases: vec!["web".into()],
                },
            ]
        );
    }

    #[test]
    fn serializes_to_the_ipc_shape() {
        let d = ContainerDetails {
            mounts: vec![MountInfo {
                typ: MountType::Volume,
                name: Some("data".into()),
                source: "/v".into(),
                destination: "/data".into(),
                read_only: false,
            }],
            ..ContainerDetails::from_docker(inspect())
        };
        let value = serde_json::to_value(&d).unwrap();
        assert_eq!(value["imageId"], json!("sha256:abc"));
        assert_eq!(value["exitCode"], json!(null));
        assert_eq!(value["restartPolicy"], json!("no"));
        assert_eq!(
            value["mounts"],
            json!([{ "type": "volume", "name": "data", "source": "/v", "destination": "/data", "readOnly": false }])
        );
    }
}
