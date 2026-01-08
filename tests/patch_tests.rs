use funnelctl::core::{
    apply_patch, detect_conflicts, remove_patch, Conflict, HttpHandler, ServeConfig,
    WebServerConfig,
};
use serde_json::Value;
use std::collections::HashMap;

// Helper function to create a basic ServeConfig with handlers
fn create_config_with_handlers(
    host_port: &str,
    handlers: Vec<(&str, &str)>,
    funnel_enabled: bool,
) -> ServeConfig {
    let mut config = ServeConfig::new();
    let mut web = HashMap::new();
    let mut web_config = WebServerConfig::new();
    let mut handlers_map = HashMap::new();

    for (path, target) in handlers {
        handlers_map.insert(path.to_string(), HttpHandler::new_proxy(target.to_string()));
    }

    web_config.handlers = Some(handlers_map);
    web.insert(host_port.to_string(), web_config);
    config.web = Some(web);

    if funnel_enabled {
        let mut funnel = HashMap::new();
        funnel.insert(host_port.to_string(), true);
        config.allow_funnel = Some(funnel);
    }

    config
}

#[test]
fn test_detect_conflicts_no_existing_config() {
    let config = ServeConfig::new();
    let result = detect_conflicts(
        &config,
        "example.com:443",
        "/api",
        "http://127.0.0.1:8080",
        false,
    );
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), None);
}

#[test]
fn test_detect_conflicts_no_conflict_different_paths() {
    let config = create_config_with_handlers(
        "example.com:443",
        vec![("/api", "http://127.0.0.1:8080")],
        false,
    );

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
    let config = create_config_with_handlers(
        "example.com:443",
        vec![("/api", "http://127.0.0.1:8080")],
        false,
    );

    let result = detect_conflicts(
        &config,
        "example.com:443",
        "/api",
        "http://127.0.0.1:9000",
        false,
    );
    assert!(result.is_err());
    match result.unwrap_err() {
        Conflict::ExactPathDifferentTarget {
            path,
            existing_target,
            new_target,
        } => {
            assert_eq!(path, "/api");
            assert!(existing_target.contains("8080"));
            assert!(new_target.contains("9000"));
        }
        _ => panic!("Expected ExactPathDifferentTarget conflict"),
    }
}

#[test]
fn test_detect_conflicts_idempotent_operation() {
    let config = create_config_with_handlers(
        "example.com:443",
        vec![("/api", "http://127.0.0.1:8080")],
        true,
    );

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
fn test_detect_conflicts_same_target_different_funnel() {
    let config = create_config_with_handlers(
        "example.com:443",
        vec![("/api", "http://127.0.0.1:8080")],
        false,
    );

    let result = detect_conflicts(
        &config,
        "example.com:443",
        "/api",
        "http://127.0.0.1:8080",
        true,
    );
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), None);
}

#[test]
fn test_detect_conflicts_non_proxy_handler() {
    let mut config = ServeConfig::new();
    let mut web = HashMap::new();
    let mut web_config = WebServerConfig::new();
    let mut handlers = HashMap::new();
    handlers.insert(
        "/api".to_string(),
        HttpHandler {
            proxy: None,
            path: None,
            text: Some("ok".to_string()),
            unknown_fields: HashMap::new(),
        },
    );
    web_config.handlers = Some(handlers);
    web.insert("example.com:443".to_string(), web_config);
    config.web = Some(web);

    let result = detect_conflicts(
        &config,
        "example.com:443",
        "/api",
        "http://127.0.0.1:8080",
        true,
    );
    assert!(result.is_err());
}

#[test]
fn test_detect_conflicts_prefix_captures_new_path() {
    let config = create_config_with_handlers(
        "example.com:443",
        vec![("/api/", "http://127.0.0.1:8080")],
        false,
    );

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
            existing_target,
        } => {
            assert_eq!(new_path, "/api/v1");
            assert_eq!(existing_prefix, "/api/");
            assert!(existing_target.contains("8080"));
        }
        _ => panic!("Expected CapturedByExistingPrefix conflict"),
    }
}

#[test]
fn test_detect_conflicts_new_prefix_captures_existing() {
    let config = create_config_with_handlers(
        "example.com:443",
        vec![("/api/v1", "http://127.0.0.1:8080")],
        false,
    );

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
            captured_target,
        } => {
            assert_eq!(new_prefix, "/api/");
            assert_eq!(captured_path, "/api/v1");
            assert!(captured_target.contains("8080"));
        }
        _ => panic!("Expected NewPrefixCapturesExisting conflict"),
    }
}

#[test]
fn test_detect_conflicts_multiple_prefix_levels() {
    let config = create_config_with_handlers(
        "example.com:443",
        vec![("/api/v1/users/", "http://127.0.0.1:8080")],
        false,
    );

    // Should conflict with shorter prefix
    let result = detect_conflicts(
        &config,
        "example.com:443",
        "/api/",
        "http://127.0.0.1:9000",
        false,
    );
    assert!(result.is_err());

    // Should conflict with intermediate prefix
    let result2 = detect_conflicts(
        &config,
        "example.com:443",
        "/api/v1/",
        "http://127.0.0.1:9000",
        false,
    );
    assert!(result2.is_err());
}

#[test]
fn test_detect_conflicts_no_prefix_without_trailing_slash() {
    let config = create_config_with_handlers(
        "example.com:443",
        vec![("/api", "http://127.0.0.1:8080")],
        false,
    );

    // /api without trailing slash should NOT capture /api/v1
    let result = detect_conflicts(
        &config,
        "example.com:443",
        "/api/v1",
        "http://127.0.0.1:9000",
        false,
    );
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), None);
}

#[test]
fn test_detect_conflicts_similar_paths_no_conflict() {
    let config = create_config_with_handlers(
        "example.com:443",
        vec![("/api", "http://127.0.0.1:8080")],
        false,
    );

    let test_cases = vec!["/api2", "/apiv1", "/api-v1", "/api_v1", "/xapi", "/apis"];

    for path in test_cases {
        let result = detect_conflicts(
            &config,
            "example.com:443",
            path,
            "http://127.0.0.1:9000",
            false,
        );
        assert!(result.is_ok(), "Path {} should not conflict", path);
        assert_eq!(result.unwrap(), None);
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

    let session_value = foreground.get("session123").unwrap();
    let session_config: ServeConfig = serde_json::from_value(session_value.clone()).unwrap();

    let handlers = session_config.get_handlers("example.com:443").unwrap();
    assert_eq!(handlers.len(), 1);
    assert!(handlers.contains_key("/api"));
}

#[test]
fn test_apply_patch_existing_session() {
    let mut config = ServeConfig::new();

    // Add first handler
    apply_patch(
        &mut config,
        "session123",
        "example.com:443",
        "/api",
        "http://127.0.0.1:8080",
        false,
    )
    .unwrap();

    // Add second handler to same session
    apply_patch(
        &mut config,
        "session123",
        "example.com:443",
        "/other",
        "http://127.0.0.1:9000",
        false,
    )
    .unwrap();

    let foreground = config.foreground.as_ref().unwrap();
    let session_value = foreground.get("session123").unwrap();
    let session_config: ServeConfig = serde_json::from_value(session_value.clone()).unwrap();

    let handlers = session_config.get_handlers("example.com:443").unwrap();
    assert_eq!(handlers.len(), 2);
    assert!(handlers.contains_key("/api"));
    assert!(handlers.contains_key("/other"));
}

#[test]
fn test_apply_patch_multiple_sessions() {
    let mut config = ServeConfig::new();

    apply_patch(
        &mut config,
        "session1",
        "example.com:443",
        "/api",
        "http://127.0.0.1:8080",
        false,
    )
    .unwrap();

    apply_patch(
        &mut config,
        "session2",
        "example.com:443",
        "/other",
        "http://127.0.0.1:9000",
        false,
    )
    .unwrap();

    let foreground = config.foreground.as_ref().unwrap();
    assert_eq!(foreground.len(), 2);
    assert!(foreground.contains_key("session1"));
    assert!(foreground.contains_key("session2"));
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
    let funnel = session_config.allow_funnel.as_ref().unwrap();
    assert_eq!(funnel.get("example.com:443"), Some(&true));
}

#[test]
fn test_apply_patch_without_funnel() {
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

    let foreground = config.foreground.as_ref().unwrap();
    let session_value = foreground.get("session123").unwrap();
    let session_config: ServeConfig = serde_json::from_value(session_value.clone()).unwrap();

    // AllowFunnel should not be set if funnel is not enabled
    assert!(
        session_config.allow_funnel.is_none()
            || !session_config
                .allow_funnel
                .as_ref()
                .unwrap()
                .contains_key("example.com:443")
    );
}

#[test]
fn test_apply_patch_preserves_unknown_fields() {
    let mut config = ServeConfig::new();
    config.unknown_fields.insert(
        "CustomField".to_string(),
        Value::String("preserved".to_string()),
    );
    config.unknown_fields.insert(
        "AnotherField".to_string(),
        Value::Number(serde_json::Number::from(42)),
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

    // Top-level unknown fields should be preserved
    assert_eq!(
        config.unknown_fields.get("CustomField"),
        Some(&Value::String("preserved".to_string()))
    );
    assert_eq!(
        config.unknown_fields.get("AnotherField"),
        Some(&Value::Number(serde_json::Number::from(42)))
    );
}

#[test]
fn test_apply_patch_update_existing_handler() {
    let mut config = ServeConfig::new();

    // Add initial handler
    apply_patch(
        &mut config,
        "session123",
        "example.com:443",
        "/api",
        "http://127.0.0.1:8080",
        false,
    )
    .unwrap();

    // Update same path with different target
    apply_patch(
        &mut config,
        "session123",
        "example.com:443",
        "/api",
        "http://127.0.0.1:9000",
        false,
    )
    .unwrap();

    let foreground = config.foreground.as_ref().unwrap();
    let session_value = foreground.get("session123").unwrap();
    let session_config: ServeConfig = serde_json::from_value(session_value.clone()).unwrap();

    let handlers = session_config.get_handlers("example.com:443").unwrap();
    let handler = handlers.get("/api").unwrap();
    assert_eq!(handler.get_proxy_target(), Some("http://127.0.0.1:9000"));
}

#[test]
fn test_remove_patch_existing_handler() {
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

    // Handlers should be cleaned up
    assert!(
        session_config.web.is_none()
            || session_config
                .get_handlers("example.com:443")
                .map_or(true, |h| h.is_empty())
    );
}

#[test]
fn test_remove_patch_nonexistent_handler() {
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

    let removed = remove_patch(&mut config, "session123", "example.com:443", "/other").unwrap();
    assert!(!removed);

    // Original handler should still exist
    let foreground = config.foreground.as_ref().unwrap();
    let session_value = foreground.get("session123").unwrap();
    let session_config: ServeConfig = serde_json::from_value(session_value.clone()).unwrap();
    let handlers = session_config.get_handlers("example.com:443").unwrap();
    assert!(handlers.contains_key("/api"));
}

#[test]
fn test_remove_patch_nonexistent_session() {
    let mut config = ServeConfig::new();
    let removed = remove_patch(&mut config, "session123", "example.com:443", "/api").unwrap();
    assert!(!removed);
}

#[test]
fn test_remove_patch_one_of_multiple_handlers() {
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
    apply_patch(
        &mut config,
        "session123",
        "example.com:443",
        "/other",
        "http://127.0.0.1:9000",
        false,
    )
    .unwrap();

    let removed = remove_patch(&mut config, "session123", "example.com:443", "/api").unwrap();
    assert!(removed);

    let foreground = config.foreground.as_ref().unwrap();
    let session_value = foreground.get("session123").unwrap();
    let session_config: ServeConfig = serde_json::from_value(session_value.clone()).unwrap();
    let handlers = session_config.get_handlers("example.com:443").unwrap();

    assert!(!handlers.contains_key("/api"));
    assert!(handlers.contains_key("/other"));
    assert_eq!(handlers.len(), 1);
}

#[test]
fn test_conflict_describe_messages() {
    let conflict1 = Conflict::ExactPathDifferentTarget {
        path: "/api".to_string(),
        existing_target: "http://127.0.0.1:8080".to_string(),
        new_target: "http://127.0.0.1:9000".to_string(),
    };
    let desc1 = conflict1.describe();
    assert!(desc1.contains("/api"));
    assert!(desc1.contains("8080"));
    assert!(desc1.contains("9000"));

    let conflict2 = Conflict::CapturedByExistingPrefix {
        new_path: "/api/v1".to_string(),
        existing_prefix: "/api/".to_string(),
        existing_target: "http://127.0.0.1:8080".to_string(),
    };
    let desc2 = conflict2.describe();
    assert!(desc2.contains("/api/v1"));
    assert!(desc2.contains("/api/"));

    let conflict3 = Conflict::NewPrefixCapturesExisting {
        new_prefix: "/api/".to_string(),
        captured_path: "/api/v1".to_string(),
        captured_target: "http://127.0.0.1:8080".to_string(),
    };
    let desc3 = conflict3.describe();
    assert!(desc3.contains("/api/"));
    assert!(desc3.contains("/api/v1"));
}

#[test]
fn test_roundtrip_serialization_with_patch() {
    let mut config = ServeConfig::new();
    config
        .unknown_fields
        .insert("TestField".to_string(), Value::String("test".to_string()));

    apply_patch(
        &mut config,
        "session123",
        "example.com:443",
        "/api",
        "http://127.0.0.1:8080",
        true,
    )
    .unwrap();

    // Serialize and deserialize
    let json = serde_json::to_string(&config).unwrap();
    let deserialized: ServeConfig = serde_json::from_str(&json).unwrap();

    // Unknown fields should be preserved
    assert_eq!(
        deserialized.unknown_fields.get("TestField"),
        Some(&Value::String("test".to_string()))
    );

    // Foreground config should be intact
    assert!(deserialized.foreground.is_some());
}

#[test]
fn test_multiple_host_ports() {
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

    apply_patch(
        &mut config,
        "session123",
        "other.com:8443",
        "/api",
        "http://127.0.0.1:9000",
        false,
    )
    .unwrap();

    let foreground = config.foreground.as_ref().unwrap();
    let session_value = foreground.get("session123").unwrap();
    let session_config: ServeConfig = serde_json::from_value(session_value.clone()).unwrap();

    assert!(session_config.get_handlers("example.com:443").is_some());
    assert!(session_config.get_handlers("other.com:8443").is_some());
}

#[test]
fn test_path_prefix_edge_cases() {
    // Root path with trailing slash should capture everything
    let config = create_config_with_handlers(
        "example.com:443",
        vec![("/", "http://127.0.0.1:8080")],
        false,
    );

    let result = detect_conflicts(
        &config,
        "example.com:443",
        "/anything",
        "http://127.0.0.1:9000",
        false,
    );
    // "/" without trailing slash added in the handler should not be treated as a prefix
    // because we store it as "/" not "//"
    // Actually, "/" does end with "/" so it SHOULD capture everything
    assert!(result.is_err());

    // Exact root match with same target but no funnel
    let config2 = create_config_with_handlers(
        "example.com:443",
        vec![("/", "http://127.0.0.1:8080")],
        false,
    );

    let result2 = detect_conflicts(
        &config2,
        "example.com:443",
        "/",
        "http://127.0.0.1:8080",
        false,
    );
    // Same path, same target, both without funnel - should be OK (not idempotent without funnel)
    assert!(result2.is_ok());
    assert_eq!(result2.unwrap(), None);
}
