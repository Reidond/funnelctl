use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// ServeConfig represents the top-level Tailscale serve configuration
/// This structure preserves unknown fields to maintain round-trip compatibility
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ServeConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tcp: Option<HashMap<u16, Value>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub web: Option<HashMap<String, WebServerConfig>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_funnel: Option<HashMap<String, bool>>,

    /// Foreground maps session_id -> ephemeral ServeConfig
    #[serde(skip_serializing_if = "Option::is_none")]
    pub foreground: Option<HashMap<String, Value>>,

    /// Preserve any unknown fields for round-trip compatibility
    #[serde(flatten)]
    pub unknown_fields: HashMap<String, Value>,
}

impl Default for ServeConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl ServeConfig {
    /// Creates a new empty ServeConfig
    pub fn new() -> Self {
        Self {
            tcp: None,
            web: None,
            allow_funnel: None,
            foreground: None,
            unknown_fields: HashMap::new(),
        }
    }

    /// Gets all HTTP handlers for a given host:port
    pub fn get_handlers(&self, host_port: &str) -> Option<&HashMap<String, HttpHandler>> {
        self.web
            .as_ref()
            .and_then(|web| web.get(host_port))
            .and_then(|config| config.handlers.as_ref())
    }

    /// Checks if funnel is enabled for a given host:port
    pub fn is_funnel_enabled(&self, host_port: &str) -> bool {
        self.allow_funnel
            .as_ref()
            .and_then(|funnel| funnel.get(host_port))
            .copied()
            .unwrap_or(false)
    }
}

/// WebServerConfig represents configuration for a specific host:port
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct WebServerConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub handlers: Option<HashMap<String, HttpHandler>>,

    /// Preserve any unknown fields for round-trip compatibility
    #[serde(flatten)]
    pub unknown_fields: HashMap<String, Value>,
}

impl WebServerConfig {
    /// Creates a new empty WebServerConfig
    pub fn new() -> Self {
        Self {
            handlers: None,
            unknown_fields: HashMap::new(),
        }
    }
}

impl Default for WebServerConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// HttpHandler represents a handler for a specific path
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct HttpHandler {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proxy: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,

    /// Preserve any unknown fields for round-trip compatibility
    #[serde(flatten)]
    pub unknown_fields: HashMap<String, Value>,
}

impl HttpHandler {
    /// Creates a new proxy handler
    pub fn new_proxy(target: String) -> Self {
        Self {
            proxy: Some(target),
            path: None,
            text: None,
            unknown_fields: HashMap::new(),
        }
    }

    /// Gets the target URL for a proxy handler
    pub fn get_proxy_target(&self) -> Option<&str> {
        self.proxy.as_deref()
    }
}

/// Represents a mapping of path to target for conflict detection
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathMapping {
    pub path: String,
    pub target: String,
    pub funnel_enabled: bool,
}

impl PathMapping {
    pub fn new(path: String, target: String, funnel_enabled: bool) -> Self {
        Self {
            path,
            target,
            funnel_enabled,
        }
    }

    /// Checks if this path is a prefix of another path
    /// Trailing slash indicates a prefix mount
    pub fn is_prefix_of(&self, other: &str) -> bool {
        if !self.path.ends_with('/') {
            return false;
        }
        other.starts_with(&self.path)
    }

    /// Checks if another path is a prefix of this path
    pub fn has_prefix(&self, other: &str) -> bool {
        if !other.ends_with('/') {
            return false;
        }
        self.path.starts_with(other)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serve_config_serialization() {
        let config = ServeConfig::new();
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: ServeConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config, deserialized);
    }

    #[test]
    fn test_serve_config_preserves_unknown_fields() {
        let json = r#"{
            "Web": {
                "example.com:443": {
                    "Handlers": {
                        "/": {"Proxy": "http://127.0.0.1:8080"}
                    }
                }
            },
            "UnknownField": "should be preserved",
            "AnotherUnknown": 42
        }"#;

        let config: ServeConfig = serde_json::from_str(json).unwrap();
        assert!(config.unknown_fields.contains_key("UnknownField"));
        assert!(config.unknown_fields.contains_key("AnotherUnknown"));

        let serialized = serde_json::to_value(&config).unwrap();
        assert!(serialized.get("UnknownField").is_some());
        assert!(serialized.get("AnotherUnknown").is_some());
    }

    #[test]
    fn test_web_server_config_preserves_unknown_fields() {
        let json = r#"{
            "Handlers": {
                "/": {"Proxy": "http://127.0.0.1:8080"}
            },
            "CustomField": "preserved"
        }"#;

        let config: WebServerConfig = serde_json::from_str(json).unwrap();
        assert!(config.unknown_fields.contains_key("CustomField"));

        let serialized = serde_json::to_value(&config).unwrap();
        assert!(serialized.get("CustomField").is_some());
    }

    #[test]
    fn test_http_handler_preserves_unknown_fields() {
        let json = r#"{
            "Proxy": "http://127.0.0.1:8080",
            "ExtraField": "data"
        }"#;

        let handler: HttpHandler = serde_json::from_str(json).unwrap();
        assert_eq!(handler.proxy, Some("http://127.0.0.1:8080".to_string()));
        assert!(handler.unknown_fields.contains_key("ExtraField"));

        let serialized = serde_json::to_value(&handler).unwrap();
        assert!(serialized.get("ExtraField").is_some());
    }

    #[test]
    fn test_path_mapping_prefix_detection() {
        let prefix = PathMapping::new("/api/".to_string(), "target".to_string(), false);
        assert!(prefix.is_prefix_of("/api/v1"));
        assert!(prefix.is_prefix_of("/api/v2/users"));
        assert!(!prefix.is_prefix_of("/other"));

        let non_prefix = PathMapping::new("/api".to_string(), "target".to_string(), false);
        assert!(!non_prefix.is_prefix_of("/api/v1"));
    }

    #[test]
    fn test_path_mapping_has_prefix() {
        let path = PathMapping::new("/api/v1".to_string(), "target".to_string(), false);
        assert!(path.has_prefix("/api/"));
        assert!(path.has_prefix("/"));
        assert!(!path.has_prefix("/other/"));

        let path2 = PathMapping::new("/api/v1/users".to_string(), "target".to_string(), false);
        assert!(path2.has_prefix("/api/"));
        assert!(path2.has_prefix("/api/v1/"));
    }

    #[test]
    fn test_get_handlers() {
        let mut config = ServeConfig::new();
        let mut web = HashMap::new();
        let mut web_config = WebServerConfig::new();
        let mut handlers = HashMap::new();
        handlers.insert(
            "/".to_string(),
            HttpHandler::new_proxy("http://127.0.0.1:8080".to_string()),
        );
        web_config.handlers = Some(handlers);
        web.insert("example.com:443".to_string(), web_config);
        config.web = Some(web);

        let result = config.get_handlers("example.com:443");
        assert!(result.is_some());
        assert_eq!(result.unwrap().len(), 1);

        let no_result = config.get_handlers("other.com:443");
        assert!(no_result.is_none());
    }

    #[test]
    fn test_is_funnel_enabled() {
        let mut config = ServeConfig::new();
        let mut funnel = HashMap::new();
        funnel.insert("example.com:443".to_string(), true);
        funnel.insert("other.com:443".to_string(), false);
        config.allow_funnel = Some(funnel);

        assert!(config.is_funnel_enabled("example.com:443"));
        assert!(!config.is_funnel_enabled("other.com:443"));
        assert!(!config.is_funnel_enabled("unknown.com:443"));
    }
}
