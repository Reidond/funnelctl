use crate::error::{FunnelError, Result};
use std::time::Duration;

/// Validation warnings returned for informational purposes
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationWarning {
    /// Path is less than 8 characters and may be guessable
    PathTooShort { path: String, length: usize },
    /// TTL is less than 5 minutes
    TtlTooShort { ttl: Duration },
}

/// Result of path validation including normalized path and any warnings
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathValidationResult {
    pub normalized_path: String,
    pub warnings: Vec<ValidationWarning>,
}

/// Result of TTL validation including any warnings
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TtlValidationResult {
    pub ttl: Duration,
    pub warnings: Vec<ValidationWarning>,
}

/// Validates and normalizes a path according to the specification:
/// - Must start with '/'
/// - No '..' segments
/// - No control characters (0x00-0x1F)
/// - Normalizes double slashes to single slashes
/// - Preserves trailing slash
/// - Warns if path < 8 characters
pub fn validate_path(path: &str) -> Result<PathValidationResult> {
    // Must start with '/'
    if !path.starts_with('/') {
        return Err(FunnelError::InvalidArgument(
            "path must start with '/'".to_string(),
        ));
    }

    // Check for control characters (0x00-0x1F)
    if path.bytes().any(|b| b < 0x20) {
        return Err(FunnelError::InvalidArgument(
            "path contains control characters".to_string(),
        ));
    }

    // Check for '..' segments (as path components, not as part of filenames)
    // Split by '/' and check each segment
    for segment in path.split('/') {
        if segment == ".." {
            return Err(FunnelError::InvalidArgument(
                "path cannot contain '..' segments".to_string(),
            ));
        }
    }

    // Normalize double slashes while preserving trailing slash
    let has_trailing_slash = path.ends_with('/') && path.len() > 1;
    let normalized = normalize_slashes(path);
    let normalized_path = if has_trailing_slash && !normalized.ends_with('/') {
        format!("{}/", normalized)
    } else {
        normalized
    };

    // Warn if path < 8 characters (guessable)
    let mut warnings = Vec::new();
    if normalized_path.len() < 8 {
        warnings.push(ValidationWarning::PathTooShort {
            path: normalized_path.clone(),
            length: normalized_path.len(),
        });
    }

    Ok(PathValidationResult {
        normalized_path,
        warnings,
    })
}

/// Normalizes consecutive slashes to single slashes
fn normalize_slashes(path: &str) -> String {
    let mut result = String::with_capacity(path.len());
    let mut prev_was_slash = false;

    for ch in path.chars() {
        if ch == '/' {
            if !prev_was_slash {
                result.push(ch);
            }
            prev_was_slash = true;
        } else {
            result.push(ch);
            prev_was_slash = false;
        }
    }

    result
}

/// Validates TTL according to the specification:
/// - Minimum 30 seconds (hard requirement)
/// - Warns if < 5 minutes
pub fn validate_ttl(ttl: Duration) -> Result<TtlValidationResult> {
    const MIN_TTL: Duration = Duration::from_secs(30);
    const WARN_TTL: Duration = Duration::from_secs(5 * 60);

    if ttl < MIN_TTL {
        return Err(FunnelError::InvalidArgument(format!(
            "TTL must be at least {} seconds, got {} seconds",
            MIN_TTL.as_secs(),
            ttl.as_secs()
        )));
    }

    let mut warnings = Vec::new();
    if ttl < WARN_TTL {
        warnings.push(ValidationWarning::TtlTooShort { ttl });
    }

    Ok(TtlValidationResult { ttl, warnings })
}

/// Validates port number is in valid range (1-65535)
pub fn validate_port(port: u16) -> Result<()> {
    if port == 0 {
        return Err(FunnelError::InvalidArgument(
            "port must be between 1 and 65535".to_string(),
        ));
    }
    Ok(())
}

/// Validates HTTPS port is one of the allowed values (443, 8443, 10000)
pub fn validate_https_port(port: u16) -> Result<()> {
    const ALLOWED_PORTS: &[u16] = &[443, 8443, 10000];

    if !ALLOWED_PORTS.contains(&port) {
        return Err(FunnelError::InvalidArgument(format!(
            "HTTPS port must be one of {:?}, got {}",
            ALLOWED_PORTS, port
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_path_valid() {
        let result = validate_path("/api/v1/users").unwrap();
        assert_eq!(result.normalized_path, "/api/v1/users");
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn test_validate_path_normalize_slashes() {
        let result = validate_path("//api///v1//users").unwrap();
        assert_eq!(result.normalized_path, "/api/v1/users");
    }

    #[test]
    fn test_validate_path_preserve_trailing_slash() {
        let result = validate_path("/api/v1/").unwrap();
        assert_eq!(result.normalized_path, "/api/v1/");

        let result2 = validate_path("//api//v1//").unwrap();
        assert_eq!(result2.normalized_path, "/api/v1/");
    }

    #[test]
    fn test_validate_path_short_warning() {
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
    fn test_validate_path_must_start_with_slash() {
        let err = validate_path("api/v1").unwrap_err();
        match err {
            FunnelError::InvalidArgument(msg) => {
                assert!(msg.contains("must start with"));
            }
            _ => panic!("Expected InvalidArgument error"),
        }
    }

    #[test]
    fn test_validate_path_no_dotdot() {
        let err = validate_path("/api/../etc/passwd").unwrap_err();
        match err {
            FunnelError::InvalidArgument(msg) => {
                assert!(msg.contains(".."));
            }
            _ => panic!("Expected InvalidArgument error"),
        }
    }

    #[test]
    fn test_validate_path_no_control_chars() {
        let err = validate_path("/api\x00/v1").unwrap_err();
        match err {
            FunnelError::InvalidArgument(msg) => {
                assert!(msg.contains("control characters"));
            }
            _ => panic!("Expected InvalidArgument error"),
        }
    }

    #[test]
    fn test_validate_ttl_valid() {
        let result = validate_ttl(Duration::from_secs(600)).unwrap();
        assert_eq!(result.ttl, Duration::from_secs(600));
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn test_validate_ttl_minimum() {
        let result = validate_ttl(Duration::from_secs(30)).unwrap();
        assert_eq!(result.ttl, Duration::from_secs(30));
        assert_eq!(result.warnings.len(), 1);
    }

    #[test]
    fn test_validate_ttl_below_minimum() {
        let err = validate_ttl(Duration::from_secs(29)).unwrap_err();
        match err {
            FunnelError::InvalidArgument(msg) => {
                assert!(msg.contains("at least 30"));
            }
            _ => panic!("Expected InvalidArgument error"),
        }
    }

    #[test]
    fn test_validate_ttl_warn_below_5min() {
        let result = validate_ttl(Duration::from_secs(60)).unwrap();
        assert_eq!(result.warnings.len(), 1);
        match &result.warnings[0] {
            ValidationWarning::TtlTooShort { ttl } => {
                assert_eq!(*ttl, Duration::from_secs(60));
            }
            _ => panic!("Expected TtlTooShort warning"),
        }
    }

    #[test]
    fn test_validate_port_valid() {
        assert!(validate_port(8080).is_ok());
        assert!(validate_port(1).is_ok());
        assert!(validate_port(65535).is_ok());
    }

    #[test]
    fn test_validate_port_zero() {
        let err = validate_port(0).unwrap_err();
        match err {
            FunnelError::InvalidArgument(_) => {}
            _ => panic!("Expected InvalidArgument error"),
        }
    }

    #[test]
    fn test_validate_https_port_valid() {
        assert!(validate_https_port(443).is_ok());
        assert!(validate_https_port(8443).is_ok());
        assert!(validate_https_port(10000).is_ok());
    }

    #[test]
    fn test_validate_https_port_invalid() {
        let err = validate_https_port(8080).unwrap_err();
        match err {
            FunnelError::InvalidArgument(msg) => {
                assert!(msg.contains("443"));
                assert!(msg.contains("8443"));
                assert!(msg.contains("10000"));
            }
            _ => panic!("Expected InvalidArgument error"),
        }
    }
}
