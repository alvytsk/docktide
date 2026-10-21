//! Docker Engine integration and domain logic for Docktide.
//!
//! This crate must not depend on Tauri. The desktop adapter in `src-tauri`
//! translates between these services and the frontend.

pub mod connection;
pub mod endpoint;
pub mod error;
pub mod system;

pub use connection::{ConnectionStatus, DockerService};
pub use endpoint::{EndpointSource, ResolvedEndpoint};
pub use error::{DockerError, ErrorKind, IpcError};
pub use system::{EngineInfo, ImageStore};

#[cfg(test)]
mod bindings {
    use std::path::Path;

    use ts_rs::{Config, TS};

    use super::*;

    /// Run with `DOCKTIDE_UPDATE_BINDINGS=1` to regenerate `src/bindings/`.
    #[test]
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
