//! Docker Engine integration and domain logic for Docktide.
//!
//! This crate must not depend on Tauri. The desktop adapter in `src-tauri`
//! translates between these services and the frontend.

pub mod connection;
pub mod containers;
pub mod endpoint;
pub mod error;
pub mod events;
pub mod images;
pub mod inspect;
pub mod logs;
pub mod stats;
pub mod streams;
pub mod system;

pub use connection::{ConnectionStatus, DockerService};
pub use containers::{ComposeInfo, ContainerState, ContainerSummary, PortMapping, Protocol};
pub use endpoint::{EndpointSource, ResolvedEndpoint};
pub use error::{DockerError, ErrorKind, IpcError};
pub use events::LiveUpdate;
pub use images::{
    BuildCachePruneResult, BuildCacheUsage, ImageDetails, ImageInspect, ImageSummary,
};
pub use inspect::{
    ContainerDetails, ContainerHealth, ContainerInspect, HealthStatus, KeyValue, MountInfo,
    MountType, NetworkAttachment,
};
pub use logs::{LogEvent, LogLine, LogStream};
pub use stats::StatsSample;
pub use streams::SubscriptionId;
pub use system::{EngineInfo, ImageStore};

#[cfg(test)]
mod bindings {
    use std::path::Path;

    use ts_rs::{Config, TS};

    use super::*;

    /// Run with `DOCKTIDE_UPDATE_BINDINGS=1` to regenerate `src/bindings/`.
    #[test]
    #[allow(clippy::too_many_lines)] // one entry per exported type
    fn typescript_bindings_are_up_to_date() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../src/bindings");
        let cfg = Config::new();
        let update = std::env::var_os("DOCKTIDE_UPDATE_BINDINGS").is_some();
        let generated = [
            (
                ConnectionStatus::ident(&cfg),
                ConnectionStatus::export_to_string(&cfg),
            ),
            (
                ResolvedEndpoint::ident(&cfg),
                ResolvedEndpoint::export_to_string(&cfg),
            ),
            (
                EndpointSource::ident(&cfg),
                EndpointSource::export_to_string(&cfg),
            ),
            (EngineInfo::ident(&cfg), EngineInfo::export_to_string(&cfg)),
            (ImageStore::ident(&cfg), ImageStore::export_to_string(&cfg)),
            (IpcError::ident(&cfg), IpcError::export_to_string(&cfg)),
            (ErrorKind::ident(&cfg), ErrorKind::export_to_string(&cfg)),
            (
                ComposeInfo::ident(&cfg),
                ComposeInfo::export_to_string(&cfg),
            ),
            (
                ContainerSummary::ident(&cfg),
                ContainerSummary::export_to_string(&cfg),
            ),
            (
                ContainerState::ident(&cfg),
                ContainerState::export_to_string(&cfg),
            ),
            (
                PortMapping::ident(&cfg),
                PortMapping::export_to_string(&cfg),
            ),
            (Protocol::ident(&cfg), Protocol::export_to_string(&cfg)),
            (LiveUpdate::ident(&cfg), LiveUpdate::export_to_string(&cfg)),
            (
                ContainerInspect::ident(&cfg),
                ContainerInspect::export_to_string(&cfg),
            ),
            (
                ContainerDetails::ident(&cfg),
                ContainerDetails::export_to_string(&cfg),
            ),
            (
                ContainerHealth::ident(&cfg),
                ContainerHealth::export_to_string(&cfg),
            ),
            (
                HealthStatus::ident(&cfg),
                HealthStatus::export_to_string(&cfg),
            ),
            (MountInfo::ident(&cfg), MountInfo::export_to_string(&cfg)),
            (MountType::ident(&cfg), MountType::export_to_string(&cfg)),
            (
                NetworkAttachment::ident(&cfg),
                NetworkAttachment::export_to_string(&cfg),
            ),
            (KeyValue::ident(&cfg), KeyValue::export_to_string(&cfg)),
            (
                ImageSummary::ident(&cfg),
                ImageSummary::export_to_string(&cfg),
            ),
            (
                ImageInspect::ident(&cfg),
                ImageInspect::export_to_string(&cfg),
            ),
            (
                ImageDetails::ident(&cfg),
                ImageDetails::export_to_string(&cfg),
            ),
            (
                BuildCacheUsage::ident(&cfg),
                BuildCacheUsage::export_to_string(&cfg),
            ),
            (
                BuildCachePruneResult::ident(&cfg),
                BuildCachePruneResult::export_to_string(&cfg),
            ),
            (LogEvent::ident(&cfg), LogEvent::export_to_string(&cfg)),
            (LogLine::ident(&cfg), LogLine::export_to_string(&cfg)),
            (LogStream::ident(&cfg), LogStream::export_to_string(&cfg)),
            (
                StatsSample::ident(&cfg),
                StatsSample::export_to_string(&cfg),
            ),
        ];
        let mut stale = Vec::new();
        for (name, contents) in generated {
            let path = dir.join(format!("{name}.ts"));
            let contents = contents.unwrap();
            if update {
                std::fs::create_dir_all(&dir).unwrap();
                std::fs::write(&path, contents).unwrap();
            } else if std::fs::read_to_string(&path).ok().as_deref() != Some(&contents) {
                stale.push(name);
            }
        }
        assert!(
            stale.is_empty(),
            "stale bindings {stale:?}; run `DOCKTIDE_UPDATE_BINDINGS=1 cargo test -p docktide-core bindings`"
        );
    }
}
