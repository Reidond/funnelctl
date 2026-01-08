use funnelctl::core::{
    validate_https_port, validate_path, validate_port, validate_ttl, ValidationWarning,
};
use std::time::Duration;

#[test]
fn test_path_validation_basic_valid() {
    let result = validate_path("/api/v1/users").unwrap();
    assert_eq!(result.normalized_path, "/api/v1/users");
    assert!(result.warnings.is_empty());
}

#[test]
fn test_path_validation_root() {
    let result = validate_path("/").unwrap();
    assert_eq!(result.normalized_path, "/");
    assert_eq!(result.warnings.len(), 1);
    match &result.warnings[0] {
        ValidationWarning::PathTooShort { path, length } => {
            assert_eq!(path, "/");
            assert_eq!(*length, 1);
        }
        _ => panic!("Expected PathTooShort warning"),
    }
}

#[test]
fn test_path_validation_normalize_double_slashes() {
    let result = validate_path("//api///v1//users").unwrap();
    assert_eq!(result.normalized_path, "/api/v1/users");
}

#[test]
fn test_path_validation_normalize_triple_slashes() {
    let result = validate_path("///api").unwrap();
    assert_eq!(result.normalized_path, "/api");
}

#[test]
fn test_path_validation_preserve_trailing_slash() {
    let result = validate_path("/api/v1/").unwrap();
    assert_eq!(result.normalized_path, "/api/v1/");

    let result2 = validate_path("//api//v1//").unwrap();
    assert_eq!(result2.normalized_path, "/api/v1/");

    let result3 = validate_path("/api/v1///").unwrap();
    assert_eq!(result3.normalized_path, "/api/v1/");
}

#[test]
fn test_path_validation_no_trailing_slash() {
    let result = validate_path("/api/v1").unwrap();
    assert_eq!(result.normalized_path, "/api/v1");
    assert!(!result.normalized_path.ends_with('/'));
}

#[test]
fn test_path_validation_short_path_warning() {
    let result = validate_path("/foo").unwrap();
    assert_eq!(result.normalized_path, "/foo");
    assert_eq!(result.warnings.len(), 1);
    match &result.warnings[0] {
        ValidationWarning::PathTooShort { path, length } => {
            assert_eq!(path, "/foo");
            assert_eq!(*length, 4);
        }
        _ => panic!("Expected PathTooShort warning"),
    }
}

#[test]
fn test_path_validation_exactly_8_chars_no_warning() {
    let result = validate_path("/1234567").unwrap();
    assert_eq!(result.normalized_path, "/1234567");
    assert!(result.warnings.is_empty());
}

#[test]
fn test_path_validation_7_chars_warning() {
    let result = validate_path("/123456").unwrap();
    assert_eq!(result.normalized_path, "/123456");
    assert_eq!(result.warnings.len(), 1);
}

#[test]
fn test_path_validation_must_start_with_slash() {
    let err = validate_path("api/v1").unwrap_err();
    let msg = format!("{}", err);
    assert!(msg.contains("must start with"));
}

#[test]
fn test_path_validation_relative_path_fails() {
    let err = validate_path("./api/v1").unwrap_err();
    let msg = format!("{}", err);
    assert!(msg.contains("must start with"));
}

#[test]
fn test_path_validation_no_dotdot_segments() {
    let test_cases = vec![
        "/api/../etc/passwd",
        "/../etc/passwd",
        "/api/v1/../v2",
        "/api/v1/..",
        "/../../etc",
    ];

    for path in test_cases {
        let err = validate_path(path).unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains(".."), "Failed for path: {}", path);
    }
}

#[test]
fn test_path_validation_dotdot_in_name_ok() {
    // ".." as part of a name should be OK as long as it's not a segment
    let result = validate_path("/api..v1").unwrap();
    assert_eq!(result.normalized_path, "/api..v1");
}

#[test]
fn test_path_validation_no_control_chars() {
    let control_chars = vec![
        '\x00', '\x01', '\x02', '\x03', '\x04', '\x05', '\x06', '\x07', '\x08', '\x09', '\x0A',
        '\x0B', '\x0C', '\x0D', '\x0E', '\x0F', '\x10', '\x11', '\x12', '\x13', '\x14', '\x15',
        '\x16', '\x17', '\x18', '\x19', '\x1A', '\x1B', '\x1C', '\x1D', '\x1E', '\x1F',
    ];

    for ch in control_chars {
        let path = format!("/api{}v1", ch);
        let err = validate_path(&path).unwrap_err();
        let msg = format!("{}", err);
        assert!(
            msg.contains("control characters"),
            "Failed for char: {:?}",
            ch
        );
    }
}

#[test]
fn test_path_validation_unicode_ok() {
    let result = validate_path("/api/用户/données").unwrap();
    assert_eq!(result.normalized_path, "/api/用户/données");
}

#[test]
fn test_path_validation_special_chars_ok() {
    let result = validate_path("/api/v1/user-profile_123.json").unwrap();
    assert_eq!(result.normalized_path, "/api/v1/user-profile_123.json");
}

#[test]
fn test_ttl_validation_valid() {
    let result = validate_ttl(Duration::from_secs(600)).unwrap();
    assert_eq!(result.ttl, Duration::from_secs(600));
    assert!(result.warnings.is_empty());
}

#[test]
fn test_ttl_validation_exactly_5min_no_warning() {
    let result = validate_ttl(Duration::from_secs(5 * 60)).unwrap();
    assert_eq!(result.ttl, Duration::from_secs(5 * 60));
    assert!(result.warnings.is_empty());
}

#[test]
fn test_ttl_validation_one_hour() {
    let result = validate_ttl(Duration::from_secs(3600)).unwrap();
    assert_eq!(result.ttl, Duration::from_secs(3600));
    assert!(result.warnings.is_empty());
}

#[test]
fn test_ttl_validation_minimum_30s() {
    let result = validate_ttl(Duration::from_secs(30)).unwrap();
    assert_eq!(result.ttl, Duration::from_secs(30));
    assert_eq!(result.warnings.len(), 1);
    match &result.warnings[0] {
        ValidationWarning::TtlTooShort { ttl } => {
            assert_eq!(*ttl, Duration::from_secs(30));
        }
        _ => panic!("Expected TtlTooShort warning"),
    }
}

#[test]
fn test_ttl_validation_below_minimum() {
    let err = validate_ttl(Duration::from_secs(29)).unwrap_err();
    let msg = format!("{}", err);
    assert!(msg.contains("at least 30"));
    assert!(msg.contains("29"));
}

#[test]
fn test_ttl_validation_zero() {
    let err = validate_ttl(Duration::from_secs(0)).unwrap_err();
    let msg = format!("{}", err);
    assert!(msg.contains("at least 30"));
}

#[test]
fn test_ttl_validation_warn_below_5min() {
    let test_cases = vec![
        30,  // minimum
        60,  // 1 minute
        120, // 2 minutes
        180, // 3 minutes
        240, // 4 minutes
        299, // just under 5 minutes
    ];

    for seconds in test_cases {
        let result = validate_ttl(Duration::from_secs(seconds)).unwrap();
        assert_eq!(result.warnings.len(), 1, "Failed for {} seconds", seconds);
        match &result.warnings[0] {
            ValidationWarning::TtlTooShort { ttl } => {
                assert_eq!(*ttl, Duration::from_secs(seconds));
            }
            _ => panic!("Expected TtlTooShort warning for {} seconds", seconds),
        }
    }
}

#[test]
fn test_ttl_validation_no_warn_5min_or_more() {
    let test_cases = vec![
        300,   // exactly 5 minutes
        301,   // just over 5 minutes
        600,   // 10 minutes
        3600,  // 1 hour
        86400, // 1 day
    ];

    for seconds in test_cases {
        let result = validate_ttl(Duration::from_secs(seconds)).unwrap();
        assert!(result.warnings.is_empty(), "Failed for {} seconds", seconds);
    }
}

#[test]
fn test_port_validation_valid_ports() {
    let valid_ports = vec![1, 80, 443, 8080, 8443, 10000, 65535];

    for port in valid_ports {
        assert!(validate_port(port).is_ok(), "Port {} should be valid", port);
    }
}

#[test]
fn test_port_validation_zero_invalid() {
    let err = validate_port(0).unwrap_err();
    let msg = format!("{}", err);
    assert!(msg.contains("between 1 and 65535"));
}

#[test]
fn test_https_port_validation_valid() {
    assert!(validate_https_port(443).is_ok());
    assert!(validate_https_port(8443).is_ok());
    assert!(validate_https_port(10000).is_ok());
}

#[test]
fn test_https_port_validation_invalid() {
    let invalid_ports = vec![80, 8080, 9000, 3000, 5000, 65535, 1];

    for port in invalid_ports {
        let err = validate_https_port(port).unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("443"), "Port {} should be invalid", port);
        assert!(msg.contains("8443"), "Port {} should be invalid", port);
        assert!(msg.contains("10000"), "Port {} should be invalid", port);
    }
}

#[test]
fn test_path_normalization_complex() {
    // Complex cases with multiple consecutive slashes
    let test_cases = vec![
        ("////api", "/api"),
        ("/api////v1", "/api/v1"),
        ("//a//b//c//d//", "/a/b/c/d/"),
        ("///////", "/"),
    ];

    for (input, expected) in test_cases {
        let result = validate_path(input).unwrap();
        assert_eq!(
            result.normalized_path, expected,
            "Failed for input: {}",
            input
        );
    }
}

#[test]
fn test_path_validation_edge_cases() {
    // Single character paths
    let result = validate_path("/a").unwrap();
    assert_eq!(result.normalized_path, "/a");
    assert_eq!(result.warnings.len(), 1);

    // Long path should work fine
    let long_path = "/".to_string() + &"a".repeat(1000);
    let result = validate_path(&long_path).unwrap();
    assert_eq!(result.normalized_path, long_path);
    assert!(result.warnings.is_empty());
}

#[test]
fn test_validation_warning_equality() {
    let w1 = ValidationWarning::PathTooShort {
        path: "/foo".to_string(),
        length: 4,
    };
    let w2 = ValidationWarning::PathTooShort {
        path: "/foo".to_string(),
        length: 4,
    };
    assert_eq!(w1, w2);

    let w3 = ValidationWarning::TtlTooShort {
        ttl: Duration::from_secs(60),
    };
    let w4 = ValidationWarning::TtlTooShort {
        ttl: Duration::from_secs(60),
    };
    assert_eq!(w3, w4);
}
