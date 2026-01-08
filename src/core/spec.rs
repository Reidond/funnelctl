use serde::{Deserialize, Serialize};
use std::fmt;
use url::Url;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalTarget {
    pub bind: String,
    pub port: u16,
}

impl LocalTarget {
    pub fn new(bind: String, port: u16) -> Self {
        Self { bind, port }
    }

    pub fn to_url(&self) -> Result<Url, url::ParseError> {
        let host = self.host_for_url();
        Url::parse(&format!("http://{}:{}", host, self.port))
    }

    fn host_for_url(&self) -> String {
        if self.bind.contains(':') && !self.bind.starts_with('[') {
            format!("[{}]", self.bind)
        } else {
            self.bind.clone()
        }
    }
}

impl fmt::Display for LocalTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let host = self.host_for_url();
        write!(f, "http://{}:{}", host, self.port)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TunnelSpec {
    pub local_target: LocalTarget,
    pub https_port: u16,
    pub path: String,
    pub funnel: bool,
}

impl TunnelSpec {
    pub fn new(local_target: LocalTarget, https_port: u16, path: String, funnel: bool) -> Self {
        Self {
            local_target,
            https_port,
            path,
            funnel,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TunnelResult {
    pub url: Url,
    pub lease_id: String,
    pub applied_at: chrono::DateTime<chrono::Utc>,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_local_target_display() {
        let target = LocalTarget::new("127.0.0.1".to_string(), 8081);
        assert_eq!(target.to_string(), "http://127.0.0.1:8081");
    }

    #[test]
    fn test_local_target_to_url() {
        let target = LocalTarget::new("127.0.0.1".to_string(), 8081);
        let url = target.to_url().expect("Failed to create URL");
        assert_eq!(url.scheme(), "http");
        assert_eq!(url.host_str(), Some("127.0.0.1"));
        assert_eq!(url.port(), Some(8081));
    }

    #[test]
    fn test_tunnel_spec_creation() {
        let target = LocalTarget::new("127.0.0.1".to_string(), 8081);
        let spec = TunnelSpec::new(target, 443, "/funnelctl/test".to_string(), true);

        assert_eq!(spec.https_port, 443);
        assert_eq!(spec.path, "/funnelctl/test");
        assert!(spec.funnel);
    }
}
