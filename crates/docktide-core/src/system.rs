//! Engine info and version (spec M0 §4 `EngineInfo`).

use bollard::models::{SystemInfo, SystemVersion};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum ImageStore {
    Containerd,
    Classic,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct EngineInfo {
    pub server_version: String,
    /// Negotiated with the server.
    pub api_version: String,
    /// The server's own maximum.
    pub server_api_version: String,
    pub os: String,
    pub arch: String,
    pub kernel_version: String,
    pub operating_system: String,
    pub rootless: bool,
    pub image_store: ImageStore,
    pub containers_running: u32,
    pub containers_stopped: u32,
    pub containers_paused: u32,
}

impl EngineInfo {
    /// Maps `/info` and `/version`. Missing fields become empty strings or zero.
    pub(crate) fn from_docker(
        info: SystemInfo,
        version: SystemVersion,
        negotiated_api_version: String,
    ) -> Self {
        let count = |n: Option<i64>| n.and_then(|n| u32::try_from(n).ok()).unwrap_or(0);
        let rootless = info
            .security_options
            .iter()
            .flatten()
            .any(|option| option.split(',').any(|part| part == "name=rootless"));
        Self {
            server_version: version.version.or(info.server_version).unwrap_or_default(),
            api_version: negotiated_api_version,
            server_api_version: version.api_version.unwrap_or_default(),
            os: version.os.unwrap_or_default(),
            arch: version.arch.unwrap_or_default(),
            kernel_version: version.kernel_version.unwrap_or_default(),
            operating_system: info.operating_system.unwrap_or_default(),
            rootless,
            image_store: image_store(info.driver_status.as_deref()),
            containers_running: count(info.containers_running),
            containers_stopped: count(info.containers_stopped),
            containers_paused: count(info.containers_paused),
        }
    }
}

/// The containerd store reports `driver-type`; graph drivers report their own status pairs.
fn image_store(driver_status: Option<&[Vec<String>]>) -> ImageStore {
    match driver_status {
        None | Some([]) => ImageStore::Unknown,
        Some(pairs) => {
            let containerd = pairs.iter().any(|pair| {
                matches!(pair.as_slice(), [key, value]
                    if key == "driver-type" && value.starts_with("io.containerd.snapshotter"))
            });
            if containerd {
                ImageStore::Containerd
            } else {
                ImageStore::Classic
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn driver_status(pairs: &[(&str, &str)]) -> Vec<Vec<String>> {
        pairs
            .iter()
            .map(|(k, v)| vec![(*k).to_owned(), (*v).to_owned()])
            .collect()
    }

    fn map(info: SystemInfo) -> EngineInfo {
        EngineInfo::from_docker(info, SystemVersion::default(), "1.53".into())
    }

    #[test]
    fn maps_version_and_counts() {
        let info = SystemInfo {
            operating_system: Some("Ubuntu 26.04 LTS".into()),
            containers_running: Some(3),
            containers_stopped: Some(2),
            containers_paused: Some(1),
            ..Default::default()
        };
        let version = SystemVersion {
            version: Some("29.9.1".into()),
            api_version: Some("1.56".into()),
            os: Some("linux".into()),
            arch: Some("amd64".into()),
            kernel_version: Some("7.0.0-38-generic".into()),
            ..Default::default()
        };
        let engine = EngineInfo::from_docker(info, version, "1.53".into());
        assert_eq!(engine.server_version, "29.9.1");
        assert_eq!(engine.api_version, "1.53");
        assert_eq!(engine.server_api_version, "1.56");
        assert_eq!(engine.os, "linux");
        assert_eq!(engine.arch, "amd64");
        assert_eq!(engine.kernel_version, "7.0.0-38-generic");
        assert_eq!(engine.operating_system, "Ubuntu 26.04 LTS");
        assert_eq!(
            (
                engine.containers_running,
                engine.containers_stopped,
                engine.containers_paused
            ),
            (3, 2, 1)
        );
    }

    #[test]
    fn missing_fields_become_empty_or_zero() {
        let engine = map(SystemInfo::default());
        assert_eq!(engine.server_version, "");
        assert_eq!(engine.os, "");
        assert_eq!(engine.containers_running, 0);
        assert!(!engine.rootless);
        assert_eq!(engine.image_store, ImageStore::Unknown);
    }

    #[test]
    fn rootless_is_detected_from_security_options() {
        let info = SystemInfo {
            security_options: Some(vec![
                "name=seccomp,profile=builtin".into(),
                "name=rootless".into(),
            ]),
            ..Default::default()
        };
        assert!(map(info).rootless);

        let info = SystemInfo {
            security_options: Some(vec!["name=apparmor,profile=default".into()]),
            ..Default::default()
        };
        assert!(!map(info).rootless);
    }

    #[test]
    fn containerd_store_is_detected_from_driver_type() {
        let info = SystemInfo {
            driver_status: Some(driver_status(&[(
                "driver-type",
                "io.containerd.snapshotter.v1",
            )])),
            ..Default::default()
        };
        assert_eq!(map(info).image_store, ImageStore::Containerd);
    }

    #[test]
    fn graph_driver_status_without_driver_type_is_classic() {
        let info = SystemInfo {
            driver_status: Some(driver_status(&[
                ("Backing Filesystem", "extfs"),
                ("Supports d_type", "true"),
            ])),
            ..Default::default()
        };
        assert_eq!(map(info).image_store, ImageStore::Classic);
    }
}

#[cfg(test)]
mod ipc_tests {
    use super::*;

    #[test]
    fn engine_info_serializes_camel_case() {
        let mut engine = EngineInfo::from_docker(
            SystemInfo::default(),
            SystemVersion::default(),
            "1.53".into(),
        );
        engine.image_store = ImageStore::Containerd;
        let json = serde_json::to_value(engine).unwrap();
        assert_eq!(json["apiVersion"], "1.53");
        assert_eq!(json["serverApiVersion"], "");
        assert_eq!(json["imageStore"], "containerd");
        assert_eq!(json["containersRunning"], 0);
    }
}
