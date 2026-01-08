pub mod lease;
pub mod patch;
pub mod spec;
pub mod types;
pub mod validation;

pub use lease::{BackendKind, Lease};
pub use patch::{apply_patch, detect_conflicts, remove_patch, Conflict};
pub use spec::{LocalTarget, TunnelResult, TunnelSpec};
pub use types::{HttpHandler, PathMapping, ServeConfig, WebServerConfig};
pub use validation::{
    validate_https_port, validate_path, validate_port, validate_ttl, PathValidationResult,
    TtlValidationResult, ValidationWarning,
};
