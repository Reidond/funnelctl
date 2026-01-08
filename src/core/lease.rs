use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::spec::TunnelSpec;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lease {
    pub lease_id: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub tunnel_spec: TunnelSpec,
    pub backend_kind: BackendKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendKind {
    LocalApi,
}

impl Lease {
    pub fn new(
        lease_id: String,
        tunnel_spec: TunnelSpec,
        expires_at: Option<DateTime<Utc>>,
    ) -> Self {
        Self {
            lease_id,
            created_at: Utc::now(),
            expires_at,
            tunnel_spec,
            backend_kind: BackendKind::LocalApi,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::spec::LocalTarget;

    #[test]
    fn test_lease_creation() {
        let target = LocalTarget::new("127.0.0.1".to_string(), 8081);
        let spec = TunnelSpec::new(target, 443, "/funnelctl/test".to_string(), true);
        let lease = Lease::new("test-lease-id".to_string(), spec, None);

        assert_eq!(lease.lease_id, "test-lease-id");
        assert!(lease.expires_at.is_none());
        assert!(matches!(lease.backend_kind, BackendKind::LocalApi));
    }
}
