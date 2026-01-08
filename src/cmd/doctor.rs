use std::sync::Arc;

use crate::backend::{Backend, BackendStatus};
use crate::error::{FunnelError, Result};
use crate::output::use_color;

pub struct DoctorCommand;

#[derive(Debug)]
struct CheckResult {
    name: String,
    passed: bool,
    message: String,
    error_code: Option<i32>,
}

impl DoctorCommand {
    pub async fn run(backend: Arc<dyn Backend>, tcp_mode: bool) -> Result<()> {
        let use_color = use_color();
        let status_result = backend.status().await;

        let mut checks = Vec::new();
        checks.push(check_tailscaled_reachable(&status_result));
        if tcp_mode {
            checks.push(check_localapi_auth(&status_result));
        }

        match &status_result {
            Ok(status) => {
                checks.push(check_version(status));
                checks.push(check_permissions(status));
                checks.push(check_https_enabled(status));
                checks.push(check_funnel_capability(status));
                checks.push(check_dns_name(status));
            }
            Err(FunnelError::Permission { .. }) => {
                checks.push(CheckResult {
                    name: "tailscaled version".to_string(),
                    passed: false,
                    message: "Cannot check version (permission denied)".to_string(),
                    error_code: Some(11),
                });
                checks.push(CheckResult {
                    name: "Permissions".to_string(),
                    passed: false,
                    message: "Permission denied — need root or operator group".to_string(),
                    error_code: Some(11),
                });
                checks.push(CheckResult {
                    name: "HTTPS enabled".to_string(),
                    passed: false,
                    message: "Cannot check HTTPS (permission denied)".to_string(),
                    error_code: Some(11),
                });
                checks.push(CheckResult {
                    name: "Funnel capability".to_string(),
                    passed: false,
                    message: "Cannot check Funnel capability (permission denied)".to_string(),
                    error_code: Some(11),
                });
                checks.push(CheckResult {
                    name: "DNS name available".to_string(),
                    passed: false,
                    message: "Cannot check DNS name (permission denied)".to_string(),
                    error_code: Some(11),
                });
            }
            Err(_) => {
                checks.push(CheckResult {
                    name: "tailscaled version".to_string(),
                    passed: false,
                    message: "Cannot check version (tailscaled unreachable)".to_string(),
                    error_code: Some(10),
                });
                checks.push(CheckResult {
                    name: "Permissions".to_string(),
                    passed: false,
                    message: "Cannot check permissions (tailscaled unreachable)".to_string(),
                    error_code: Some(10),
                });
                checks.push(CheckResult {
                    name: "HTTPS enabled".to_string(),
                    passed: false,
                    message: "Cannot check HTTPS (tailscaled unreachable)".to_string(),
                    error_code: Some(10),
                });
                checks.push(CheckResult {
                    name: "Funnel capability".to_string(),
                    passed: false,
                    message: "Cannot check Funnel capability (tailscaled unreachable)".to_string(),
                    error_code: Some(10),
                });
                checks.push(CheckResult {
                    name: "DNS name available".to_string(),
                    passed: false,
                    message: "Cannot check DNS name (tailscaled unreachable)".to_string(),
                    error_code: Some(10),
                });
            }
        }

        Self::print_results(&checks, use_color);

        let exit_code = select_exit_code(&checks);
        if exit_code != 0 {
            std::process::exit(exit_code);
        }

        Ok(())
    }

    fn print_results(checks: &[CheckResult], use_color: bool) {
        let (pass_mark, fail_mark) = if use_color {
            ("\x1b[1;32m✓\x1b[0m", "\x1b[1;31m✗\x1b[0m")
        } else {
            ("✓", "✗")
        };

        for check in checks {
            let mark = if check.passed { pass_mark } else { fail_mark };
            println!("{} {}: {}", mark, check.name, check.message);
        }
    }
}

fn check_tailscaled_reachable(status: &Result<BackendStatus>) -> CheckResult {
    match status {
        Ok(_) => CheckResult {
            name: "tailscaled reachable".to_string(),
            passed: true,
            message: "Socket exists and responds".to_string(),
            error_code: None,
        },
        Err(FunnelError::Permission { .. }) => CheckResult {
            name: "tailscaled reachable".to_string(),
            passed: true,
            message: "Socket exists and responds".to_string(),
            error_code: None,
        },
        Err(FunnelError::Unreachable { .. }) => CheckResult {
            name: "tailscaled reachable".to_string(),
            passed: false,
            message: "tailscaled not running".to_string(),
            error_code: Some(10),
        },
        Err(_) => CheckResult {
            name: "tailscaled reachable".to_string(),
            passed: false,
            message: "tailscaled not running".to_string(),
            error_code: Some(10),
        },
    }
}

fn check_localapi_auth(status: &Result<BackendStatus>) -> CheckResult {
    match status {
        Ok(_) => CheckResult {
            name: "LocalAPI auth".to_string(),
            passed: true,
            message: "Password accepted".to_string(),
            error_code: None,
        },
        Err(FunnelError::Permission { .. }) => CheckResult {
            name: "LocalAPI auth".to_string(),
            passed: false,
            message: "Invalid LocalAPI password".to_string(),
            error_code: Some(11),
        },
        Err(_) => CheckResult {
            name: "LocalAPI auth".to_string(),
            passed: false,
            message: "Cannot check (tailscaled unreachable)".to_string(),
            error_code: Some(10),
        },
    }
}

fn check_version(status: &BackendStatus) -> CheckResult {
    match status.version.as_deref() {
        Some(version) => {
            if version_supported(version) {
                CheckResult {
                    name: "tailscaled version".to_string(),
                    passed: true,
                    message: format!("Version {} (>= 1.50.0)", version),
                    error_code: None,
                }
            } else {
                CheckResult {
                    name: "tailscaled version".to_string(),
                    passed: false,
                    message: format!("tailscaled too old (got {}, need 1.50.0+)", version),
                    error_code: Some(16),
                }
            }
        }
        None => CheckResult {
            name: "tailscaled version".to_string(),
            passed: false,
            message: "Version unknown".to_string(),
            error_code: Some(16),
        },
    }
}

fn check_permissions(status: &BackendStatus) -> CheckResult {
    match status.permissions_ok {
        Some(true) => CheckResult {
            name: "Permissions".to_string(),
            passed: true,
            message: "Can read/write ServeConfig".to_string(),
            error_code: None,
        },
        Some(false) => CheckResult {
            name: "Permissions".to_string(),
            passed: false,
            message: "Permission denied — need root or operator group".to_string(),
            error_code: Some(11),
        },
        None => CheckResult {
            name: "Permissions".to_string(),
            passed: false,
            message: "Permission check unavailable".to_string(),
            error_code: Some(11),
        },
    }
}

fn check_https_enabled(status: &BackendStatus) -> CheckResult {
    match status.https_enabled {
        Some(true) => CheckResult {
            name: "HTTPS enabled".to_string(),
            passed: true,
            message: "Node has HTTPS cert".to_string(),
            error_code: None,
        },
        Some(false) | None => CheckResult {
            name: "HTTPS enabled".to_string(),
            passed: false,
            message: "HTTPS not enabled. Run `tailscale cert`".to_string(),
            error_code: Some(12),
        },
    }
}

fn check_funnel_capability(status: &BackendStatus) -> CheckResult {
    match status.funnel_enabled {
        Some(true) => CheckResult {
            name: "Funnel capability".to_string(),
            passed: true,
            message: "Tailnet allows Funnel".to_string(),
            error_code: None,
        },
        Some(false) | None => CheckResult {
            name: "Funnel capability".to_string(),
            passed: false,
            message: "Funnel not enabled in tailnet policy".to_string(),
            error_code: Some(12),
        },
    }
}

fn check_dns_name(status: &BackendStatus) -> CheckResult {
    match status.dns_name.as_deref() {
        Some(name) => CheckResult {
            name: "DNS name available".to_string(),
            passed: true,
            message: format!("Node name: {}", name),
            error_code: None,
        },
        None => CheckResult {
            name: "DNS name available".to_string(),
            passed: false,
            message: "Node not yet assigned DNS name".to_string(),
            error_code: Some(12),
        },
    }
}

fn select_exit_code(checks: &[CheckResult]) -> i32 {
    let priority = [10, 11, 16, 12, 13, 14, 15, 2, 1];
    for code in priority {
        if checks
            .iter()
            .any(|check| !check.passed && check.error_code == Some(code))
        {
            return code;
        }
    }
    0
}

fn version_supported(version: &str) -> bool {
    let mut parts = version.split(['.', '-']);
    let major: u32 = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0);
    let minor: u32 = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0);
    let patch: u32 = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0);
    (major, minor, patch) >= (1, 50, 0)
}
