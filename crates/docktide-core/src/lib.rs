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
pub use error::DockerError;
pub use system::{EngineInfo, ImageStore};
