use crate::core::{TunnelResult, TunnelSpec};
use crate::error::{FunnelError, Result};

pub mod localapi;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendStatus {
    pub dns_name: Option<String>,
    pub version: Option<String>,
    pub https_enabled: Option<bool>,
    pub funnel_enabled: Option<bool>,
    pub permissions_ok: Option<bool>,
}

#[async_trait]
pub trait Backend: Send + Sync {
    async fn apply(&self, spec: &TunnelSpec) -> Result<TunnelResult>;
    async fn remove(&self, lease_id: &str) -> Result<()>;
    async fn status(&self) -> Result<BackendStatus>;
}

pub struct MockBackend;

#[async_trait]
impl Backend for MockBackend {
    async fn apply(&self, _spec: &TunnelSpec) -> Result<TunnelResult> {
        Err(FunnelError::Other(
            "MockBackend not implemented".to_string(),
        ))
    }

    async fn remove(&self, _lease_id: &str) -> Result<()> {
        Err(FunnelError::Other(
            "MockBackend not implemented".to_string(),
        ))
    }

    async fn status(&self) -> Result<BackendStatus> {
        Ok(BackendStatus {
            dns_name: Some("mock-node".to_string()),
            version: Some("1.50.0".to_string()),
            https_enabled: Some(true),
            funnel_enabled: Some(true),
            permissions_ok: Some(true),
        })
    }
}

pub struct UnreachableBackend {
    context: String,
}

impl UnreachableBackend {
    pub fn new(context: impl Into<String>) -> Self {
        Self {
            context: context.into(),
        }
    }
}

#[async_trait]
impl Backend for UnreachableBackend {
    async fn apply(&self, _spec: &TunnelSpec) -> Result<TunnelResult> {
        Err(FunnelError::Unreachable {
            source: None,
            context: self.context.clone(),
        })
    }

    async fn remove(&self, _lease_id: &str) -> Result<()> {
        Err(FunnelError::Unreachable {
            source: None,
            context: self.context.clone(),
        })
    }

    async fn status(&self) -> Result<BackendStatus> {
        Err(FunnelError::Unreachable {
            source: None,
            context: self.context.clone(),
        })
    }
}
