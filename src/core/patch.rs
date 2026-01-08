use crate::core::types::{HttpHandler, PathMapping, ServeConfig};
use crate::error::{FunnelError, Result};
use std::collections::HashMap;

/// Represents a conflict between existing and new configuration
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Conflict {
    /// Exact path match with different target
    ExactPathDifferentTarget {
        path: String,
        existing_target: String,
        new_target: String,
    },
    /// New path would be captured by existing prefix
    CapturedByExistingPrefix {
        new_path: String,
        existing_prefix: String,
        existing_target: String,
    },
    /// New prefix would capture existing paths
    NewPrefixCapturesExisting {
        new_prefix: String,
        captured_path: String,
        captured_target: String,
    },
}

impl Conflict {
    /// Returns a human-readable description of the conflict
    pub fn describe(&self) -> String {
        match self {
            Conflict::ExactPathDifferentTarget {
                path,
                existing_target,
                new_target,
            } => {
                format!(
                    "path '{}' already maps to '{}', but new mapping targets '{}'",
                    path, existing_target, new_target
                )
            }
            Conflict::CapturedByExistingPrefix {
                new_path,
                existing_prefix,
                existing_target,
            } => {
                format!(
                    "new path '{}' would be captured by existing prefix '{}' (targets '{}')",
                    new_path, existing_prefix, existing_target
                )
            }
            Conflict::NewPrefixCapturesExisting {
                new_prefix,
                captured_path,
                captured_target,
            } => {
                format!(
                    "new prefix '{}' would capture existing path '{}' (targets '{}')",
                    new_prefix, captured_path, captured_target
                )
            }
        }
    }
}

/// Detects conflicts between a new path mapping and existing configuration
///
/// Returns:
/// - Ok(None) if there are no conflicts
/// - Ok(Some(true)) if the mapping is idempotent (exact match with funnel enabled)
/// - Err(Conflict) if there is a conflict
pub fn detect_conflicts(
    config: &ServeConfig,
    host_port: &str,
    new_path: &str,
    new_target: &str,
    funnel_enabled: bool,
) -> std::result::Result<Option<bool>, Conflict> {
    let handlers = match config.get_handlers(host_port) {
        Some(h) => h,
        None => return Ok(None), // No existing handlers, no conflict
    };

    let existing_funnel_enabled = config.is_funnel_enabled(host_port);

    // Extract existing path mappings
    let existing_mappings: Vec<PathMapping> = handlers
        .iter()
        .map(|(path, handler)| {
            let target = describe_handler_target(handler);
            PathMapping::new(path.clone(), target, existing_funnel_enabled)
        })
        .collect();

    let new_mapping =
        PathMapping::new(new_path.to_string(), new_target.to_string(), funnel_enabled);

    // Check for conflicts
    for existing in &existing_mappings {
        // Exact path match
        if existing.path == new_path {
            if existing.target == new_target {
                if existing_funnel_enabled && funnel_enabled {
                    return Ok(Some(true));
                }
                return Ok(None);
            }
            return Err(Conflict::ExactPathDifferentTarget {
                path: new_path.to_string(),
                existing_target: existing.target.clone(),
                new_target: new_target.to_string(),
            });
        }

        // Check if new path would be captured by existing prefix
        if existing.is_prefix_of(new_path) {
            return Err(Conflict::CapturedByExistingPrefix {
                new_path: new_path.to_string(),
                existing_prefix: existing.path.clone(),
                existing_target: existing.target.clone(),
            });
        }

        // Check if new prefix would capture existing paths
        if new_mapping.is_prefix_of(&existing.path) {
            return Err(Conflict::NewPrefixCapturesExisting {
                new_prefix: new_path.to_string(),
                captured_path: existing.path.clone(),
                captured_target: existing.target.clone(),
            });
        }
    }

    Ok(None)
}

fn describe_handler_target(handler: &HttpHandler) -> String {
    if let Some(proxy) = handler.get_proxy_target() {
        return proxy.to_string();
    }
    if let Some(path) = handler.path.as_deref() {
        return format!("path handler {}", path);
    }
    if handler.text.is_some() {
        return "text handler".to_string();
    }
    "non-proxy handler".to_string()
}

/// Applies a patch to the ServeConfig, updating Foreground[session_id] and AllowFunnel
/// while preserving unknown JSON fields
///
/// This function:
/// 1. Updates or creates the foreground config for the given session_id
/// 2. Sets the handler for the specified path
/// 3. Updates the AllowFunnel setting if funnel is enabled
/// 4. Preserves all unknown fields in the JSON structure
pub fn apply_patch(
    config: &mut ServeConfig,
    session_id: &str,
    host_port: &str,
    path: &str,
    target: &str,
    funnel_enabled: bool,
) -> Result<()> {
    // Ensure foreground map exists
    let foreground = config.foreground.get_or_insert_with(HashMap::new);

    // Get or create foreground config for this session
    let default_value = serde_json::to_value(ServeConfig::new())
        .map_err(|e| FunnelError::Other(format!("Failed to serialize empty ServeConfig: {}", e)))?;
    let session_config_value = foreground
        .entry(session_id.to_string())
        .or_insert(default_value);

    // Deserialize session config
    let mut session_config: ServeConfig = serde_json::from_value(session_config_value.clone())
        .map_err(|e| FunnelError::Other(format!("Failed to parse session config: {}", e)))?;

    // Ensure web map exists
    let web = session_config.web.get_or_insert_with(HashMap::new);

    // Get or create web server config for this host:port
    let web_config = web.entry(host_port.to_string()).or_default();

    // Ensure handlers map exists
    let handlers = web_config.handlers.get_or_insert_with(HashMap::new);

    // Add/update the handler
    handlers.insert(path.to_string(), HttpHandler::new_proxy(target.to_string()));

    // Update funnel setting if enabled
    if funnel_enabled {
        let allow_funnel = session_config.allow_funnel.get_or_insert_with(HashMap::new);
        allow_funnel.insert(host_port.to_string(), true);
    }

    // Serialize session config back
    *session_config_value = serde_json::to_value(&session_config)
        .map_err(|e| FunnelError::Other(format!("Failed to serialize session config: {}", e)))?;

    Ok(())
}

/// Removes a path mapping from the foreground configuration
pub fn remove_patch(
    config: &mut ServeConfig,
    session_id: &str,
    host_port: &str,
    path: &str,
) -> Result<bool> {
    let foreground = match config.foreground.as_mut() {
        Some(f) => f,
        None => return Ok(false), // No foreground config
    };

    let session_config_value = match foreground.get_mut(session_id) {
        Some(v) => v,
        None => return Ok(false), // No session config
    };

    // Deserialize session config
    let mut session_config: ServeConfig = serde_json::from_value(session_config_value.clone())
        .map_err(|e| FunnelError::Other(format!("Failed to parse session config: {}", e)))?;

    let web = match session_config.web.as_mut() {
        Some(w) => w,
        None => return Ok(false), // No web config
    };

    let web_config = match web.get_mut(host_port) {
        Some(wc) => wc,
        None => return Ok(false), // No config for this host:port
    };

    let handlers = match web_config.handlers.as_mut() {
        Some(h) => h,
        None => return Ok(false), // No handlers
    };

    // Remove the handler
    let removed = handlers.remove(path).is_some();

    // Clean up empty structures
    if handlers.is_empty() {
        web_config.handlers = None;
    }
    if web_config.handlers.is_none() && web_config.unknown_fields.is_empty() {
        web.remove(host_port);
    }
    if web.is_empty() {
        session_config.web = None;
    }

    // Serialize session config back
    *session_config_value = serde_json::to_value(&session_config)
        .map_err(|e| FunnelError::Other(format!("Failed to serialize session config: {}", e)))?;

    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::WebServerConfig;
    use serde_json::Value;

    fn create_test_config() -> ServeConfig {
        let mut config = ServeConfig::new();
        let mut web = HashMap::new();
        let mut web_config = WebServerConfig::new();
        let mut handlers = HashMap::new();
        handlers.insert(
            "/api".to_string(),
            HttpHandler::new_proxy("http://127.0.0.1:8080".to_string()),
        );
        web_config.handlers = Some(handlers);
        web.insert("example.com:443".to_string(), web_config);
        config.web = Some(web);
        config
    }

    #[test]
    fn test_detect_conflicts_no_conflict() {
        let config = create_test_config();
        let result = detect_conflicts(
            &config,
            "example.com:443",
            "/other",
            "http://127.0.0.1:9000",
            false,
        );
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), None);
    }

    #[test]
    fn test_detect_conflicts_exact_path_different_target() {
        let config = create_test_config();
        let result = detect_conflicts(
            &config,
            "example.com:443",
            "/api",
            "http://127.0.0.1:9000",
            false,
        );
        assert!(result.is_err());
        match result.unwrap_err() {
            Conflict::ExactPathDifferentTarget { path, .. } => {
                assert_eq!(path, "/api");
            }
            _ => panic!("Expected ExactPathDifferentTarget conflict"),
        }
    }

    #[test]
    fn test_detect_conflicts_idempotent() {
        let mut config = create_test_config();
        // Enable funnel
        let mut funnel = HashMap::new();
        funnel.insert("example.com:443".to_string(), true);
        config.allow_funnel = Some(funnel);

        let result = detect_conflicts(
            &config,
            "example.com:443",
            "/api",
            "http://127.0.0.1:8080",
            true,
        );
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Some(true)); // Idempotent
    }

    #[test]
    fn test_detect_conflicts_prefix_overlap_existing() {
        let mut config = ServeConfig::new();
        let mut web = HashMap::new();
        let mut web_config = WebServerConfig::new();
        let mut handlers = HashMap::new();
        handlers.insert(
            "/api/".to_string(),
            HttpHandler::new_proxy("http://127.0.0.1:8080".to_string()),
        );
        web_config.handlers = Some(handlers);
        web.insert("example.com:443".to_string(), web_config);
        config.web = Some(web);

        let result = detect_conflicts(
            &config,
            "example.com:443",
            "/api/v1",
            "http://127.0.0.1:9000",
            false,
        );
        assert!(result.is_err());
        match result.unwrap_err() {
            Conflict::CapturedByExistingPrefix {
                new_path,
                existing_prefix,
                ..
            } => {
                assert_eq!(new_path, "/api/v1");
                assert_eq!(existing_prefix, "/api/");
            }
            _ => panic!("Expected CapturedByExistingPrefix conflict"),
        }
    }

    #[test]
    fn test_detect_conflicts_prefix_overlap_new() {
        let mut config = ServeConfig::new();
        let mut web = HashMap::new();
        let mut web_config = WebServerConfig::new();
        let mut handlers = HashMap::new();
        handlers.insert(
            "/api/v1".to_string(),
            HttpHandler::new_proxy("http://127.0.0.1:8080".to_string()),
        );
        web_config.handlers = Some(handlers);
        web.insert("example.com:443".to_string(), web_config);
        config.web = Some(web);

        let result = detect_conflicts(
            &config,
            "example.com:443",
            "/api/",
            "http://127.0.0.1:9000",
            false,
        );
        assert!(result.is_err());
        match result.unwrap_err() {
            Conflict::NewPrefixCapturesExisting {
                new_prefix,
                captured_path,
                ..
            } => {
                assert_eq!(new_prefix, "/api/");
                assert_eq!(captured_path, "/api/v1");
            }
            _ => panic!("Expected NewPrefixCapturesExisting conflict"),
        }
    }

    #[test]
    fn test_apply_patch_new_session() {
        let mut config = ServeConfig::new();
        apply_patch(
            &mut config,
            "session123",
            "example.com:443",
            "/api",
            "http://127.0.0.1:8080",
            false,
        )
        .unwrap();

        assert!(config.foreground.is_some());
        let foreground = config.foreground.as_ref().unwrap();
        assert!(foreground.contains_key("session123"));
    }

    #[test]
    fn test_apply_patch_with_funnel() {
        let mut config = ServeConfig::new();
        apply_patch(
            &mut config,
            "session123",
            "example.com:443",
            "/api",
            "http://127.0.0.1:8080",
            true,
        )
        .unwrap();

        let foreground = config.foreground.as_ref().unwrap();
        let session_value = foreground.get("session123").unwrap();
        let session_config: ServeConfig = serde_json::from_value(session_value.clone()).unwrap();

        assert!(session_config.allow_funnel.is_some());
        assert_eq!(
            session_config
                .allow_funnel
                .as_ref()
                .unwrap()
                .get("example.com:443"),
            Some(&true)
        );
    }

    #[test]
    fn test_apply_patch_preserves_unknown_fields() {
        let mut config = ServeConfig::new();
        config.unknown_fields.insert(
            "CustomField".to_string(),
            Value::String("preserved".to_string()),
        );

        apply_patch(
            &mut config,
            "session123",
            "example.com:443",
            "/api",
            "http://127.0.0.1:8080",
            false,
        )
        .unwrap();

        assert!(config.unknown_fields.contains_key("CustomField"));
        assert_eq!(
            config.unknown_fields.get("CustomField"),
            Some(&Value::String("preserved".to_string()))
        );
    }

    #[test]
    fn test_remove_patch_existing() {
        let mut config = ServeConfig::new();
        apply_patch(
            &mut config,
            "session123",
            "example.com:443",
            "/api",
            "http://127.0.0.1:8080",
            false,
        )
        .unwrap();

        let removed = remove_patch(&mut config, "session123", "example.com:443", "/api").unwrap();
        assert!(removed);

        let foreground = config.foreground.as_ref().unwrap();
        let session_value = foreground.get("session123").unwrap();
        let session_config: ServeConfig = serde_json::from_value(session_value.clone()).unwrap();
        assert!(session_config.web.is_none() || session_config.web.as_ref().unwrap().is_empty());
    }

    #[test]
    fn test_remove_patch_nonexistent() {
        let mut config = ServeConfig::new();
        let removed = remove_patch(&mut config, "session123", "example.com:443", "/api").unwrap();
        assert!(!removed);
    }

    #[test]
    fn test_conflict_describe() {
        let conflict = Conflict::ExactPathDifferentTarget {
            path: "/api".to_string(),
            existing_target: "http://127.0.0.1:8080".to_string(),
            new_target: "http://127.0.0.1:9000".to_string(),
        };
        let desc = conflict.describe();
        assert!(desc.contains("/api"));
        assert!(desc.contains("8080"));
        assert!(desc.contains("9000"));
    }
}
