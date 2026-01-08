mod client;

use std::net::SocketAddr;
use std::path::PathBuf;

use chrono::Utc;
use serde_json::Value;
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::time::{timeout, Duration};

use crate::backend::{Backend, BackendStatus};
use crate::core::{
    apply_patch, detect_conflicts, LocalTarget, ServeConfig, TunnelResult, TunnelSpec,
};
use crate::error::{FunnelError, Result};
use crate::net::{LocalApiError, LocalApiTransport};

pub use client::{LocalApiClient, WatchIpnBus};

const MIN_SUPPORTED_VERSION: (u32, u32, u32) = (1, 50, 0);
const SOCKET_CANDIDATES: &[&str] = &[
    "/var/run/tailscale/tailscaled.sock",
    "/run/tailscale/tailscaled.sock",
];

pub struct LocalApiBackend {
    client: LocalApiClient,
    watch: Mutex<Option<WatchIpnBus>>,
    force: bool,
}

impl LocalApiBackend {
    pub fn new(transport: LocalApiTransport, force: bool) -> Self {
        Self {
            client: LocalApiClient::new(transport),
            watch: Mutex::new(None),
            force,
        }
    }

    pub fn build_transport(
        socket: Option<PathBuf>,
        localapi_port: Option<u16>,
        localapi_password_file: Option<PathBuf>,
    ) -> Result<LocalApiTransport> {
        if let Some(port) = localapi_port {
            let password_file = localapi_password_file.ok_or_else(|| {
                FunnelError::InvalidArgument(
                    "--localapi-password-file is required when using --localapi-port".to_string(),
                )
            })?;
            return LocalApiTransport::tcp_auth_password_file("127.0.0.1", port, password_file)
                .map_err(map_transport_error);
        }

        if let Some(path) = socket {
            if !path.exists() {
                return Err(FunnelError::Unreachable {
                    source: None,
                    context: format!("Socket {} not found", path.display()),
                });
            }
            return Ok(LocalApiTransport::unix_socket(path));
        }

        if let Some(path) = find_first_socket() {
            return Ok(LocalApiTransport::unix_socket(path));
        }

        Err(FunnelError::Unreachable {
            source: None,
            context: "No LocalAPI socket found; try --localapi-port with --localapi-password-file"
                .to_string(),
        })
    }

    async fn check_port_liveness(&self, target: &LocalTarget) -> Result<()> {
        let addr = resolve_socket_addr(target).await?;
        let result = timeout(Duration::from_secs(2), TcpStream::connect(addr)).await;
        match result {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(err)) => Err(FunnelError::TargetPortInaccessible {
                source: Some(Box::new(err)),
                context: format!("Connection refused to {}", target),
            }),
            Err(_) => Err(FunnelError::TargetPortInaccessible {
                source: None,
                context: format!("Timed out connecting to {}", target),
            }),
        }
    }

    async fn fetch_status(&self) -> Result<BackendStatus> {
        let value = self
            .client
            .get_status()
            .await
            .map_err(map_transport_error)?;

        let version = value
            .get("Version")
            .and_then(Value::as_str)
            .map(str::to_string);

        let dns_name = parse_dns_name(&value);
        let https_enabled = parse_https_enabled(&value);
        let funnel_enabled = parse_funnel_enabled(&value);

        Ok(BackendStatus {
            dns_name,
            version,
            https_enabled,
            funnel_enabled,
            permissions_ok: None,
        })
    }
}

#[async_trait::async_trait]
impl Backend for LocalApiBackend {
    async fn apply(&self, spec: &TunnelSpec) -> Result<TunnelResult> {
        let watch = self
            .client
            .watch_ipn_bus()
            .await
            .map_err(map_transport_error)?;
        let session_id = watch.session_id().to_string();

        self.check_port_liveness(&spec.local_target).await?;

        let status = self.fetch_status().await?;
        ensure_version_supported(status.version.as_deref())?;

        let dns_name = status.dns_name.ok_or_else(|| FunnelError::Prerequisites {
            source: None,
            context: "Node not yet assigned DNS name".to_string(),
        })?;

        if status.https_enabled != Some(true) {
            return Err(FunnelError::Prerequisites {
                source: None,
                context: "HTTPS not enabled. Run `tailscale cert`".to_string(),
            });
        }

        if status.funnel_enabled != Some(true) {
            return Err(FunnelError::Prerequisites {
                source: None,
                context: "Funnel not enabled in tailnet policy".to_string(),
            });
        }

        let host_port = format!("{}:{}", dns_name, spec.https_port);

        let mut attempt = 0u8;
        loop {
            attempt += 1;
            let response = self
                .client
                .get_serve_config()
                .await
                .map_err(map_transport_error)?;

            let etag = response.etag.ok_or_else(|| FunnelError::VersionTooOld {
                source: None,
                context: "ServeConfig ETag missing; LocalAPI too old".to_string(),
            })?;

            let mut config = value_to_config(response.config)?;

            match detect_conflicts(
                &config,
                &host_port,
                &spec.path,
                &spec.local_target.to_string(),
                spec.funnel,
            ) {
                Ok(Some(true)) => {
                    // Idempotent; no conflict. Still proceed to set foreground config.
                }
                Ok(Some(false)) => {}
                Ok(None) => {}
                Err(conflict) => {
                    if !self.force {
                        return Err(FunnelError::Conflict {
                            source: None,
                            context: conflict.describe(),
                        });
                    }
                }
            }

            if let Some(foreground) = &config.foreground {
                for (session, value) in foreground {
                    let session_config = value_to_config(value.clone())?;
                    match detect_conflicts(
                        &session_config,
                        &host_port,
                        &spec.path,
                        &spec.local_target.to_string(),
                        spec.funnel,
                    ) {
                        Ok(None) => continue,
                        Ok(Some(_)) => {
                            if !self.force {
                                return Err(FunnelError::Conflict {
                                    source: None,
                                    context: format!(
                                        "Path {} already in use by foreground session {}",
                                        spec.path, session
                                    ),
                                });
                            }
                        }
                        Err(conflict) => {
                            if !self.force {
                                return Err(FunnelError::Conflict {
                                    source: None,
                                    context: format!(
                                        "{} (session {})",
                                        conflict.describe(),
                                        session
                                    ),
                                });
                            }
                        }
                    }
                }
            }

            apply_patch(
                &mut config,
                &session_id,
                &host_port,
                &spec.path,
                &spec.local_target.to_string(),
                spec.funnel,
            )?;

            let value = serde_json::to_value(config).map_err(|err| FunnelError::ApplyFailed {
                source: Some(Box::new(err)),
                context: "Failed to serialize ServeConfig".to_string(),
            })?;

            match self.client.set_serve_config(&value, Some(&etag)).await {
                Ok(()) => break,
                Err(LocalApiError::HttpStatus { status, .. })
                    if status == hyper::StatusCode::PRECONDITION_FAILED
                        || status == hyper::StatusCode::CONFLICT =>
                {
                    if attempt >= 3 {
                        return Err(FunnelError::ApplyFailed {
                            source: None,
                            context: "ServeConfig changed concurrently; retry later".to_string(),
                        });
                    }
                    continue;
                }
                Err(err) => return Err(map_transport_error(err)),
            }
        }

        let mut guard = self.watch.lock().await;
        *guard = Some(watch);

        let url = build_url(&dns_name, spec.https_port, &spec.path)?;

        Ok(TunnelResult {
            url,
            lease_id: session_id,
            applied_at: Utc::now(),
            expires_at: None,
        })
    }

    async fn remove(&self, _lease_id: &str) -> Result<()> {
        let mut guard = self.watch.lock().await;
        if let Some(mut watch) = guard.take() {
            watch.close();
        }
        Ok(())
    }

    async fn status(&self) -> Result<BackendStatus> {
        let mut status = self.fetch_status().await?;

        match self.client.get_serve_config().await {
            Ok(_) => status.permissions_ok = Some(true),
            Err(LocalApiError::HttpStatus { status: code, .. })
                if code == hyper::StatusCode::FORBIDDEN
                    || code == hyper::StatusCode::UNAUTHORIZED =>
            {
                status.permissions_ok = Some(false);
            }
            Err(_) => {
                status.permissions_ok = None;
            }
        }

        Ok(status)
    }
}

fn find_first_socket() -> Option<PathBuf> {
    SOCKET_CANDIDATES
        .iter()
        .map(PathBuf::from)
        .find(|path| path.exists())
}

fn value_to_config(value: Value) -> Result<ServeConfig> {
    if value.is_null() {
        return Ok(ServeConfig::new());
    }
    serde_json::from_value(value).map_err(|err| FunnelError::ApplyFailed {
        source: Some(Box::new(err)),
        context: "Failed to parse ServeConfig".to_string(),
    })
}

fn parse_dns_name(value: &Value) -> Option<String> {
    let dns = value
        .pointer("/Self/DNSName")
        .and_then(Value::as_str)
        .map(trim_trailing_dot);
    if dns.is_some() {
        return dns.map(str::to_string);
    }

    let host = value.pointer("/Self/HostName").and_then(Value::as_str);
    let suffix = value
        .pointer("/CurrentTailnet/MagicDNSSuffix")
        .and_then(Value::as_str)
        .or_else(|| value.pointer("/MagicDNSSuffix").and_then(Value::as_str))
        .or_else(|| {
            value
                .pointer("/CurrentTailnet/Name")
                .and_then(Value::as_str)
        });

    match (host, suffix) {
        (Some(host), Some(suffix)) => Some(format!("{}.{}", host, suffix)),
        _ => None,
    }
}

fn parse_https_enabled(value: &Value) -> Option<bool> {
    if let Some(domains) = value.pointer("/Self/CertDomains").and_then(Value::as_array) {
        return Some(!domains.is_empty());
    }
    if let Some(domains) = value.get("CertDomains").and_then(Value::as_array) {
        return Some(!domains.is_empty());
    }
    value.pointer("/Self/HTTPS").and_then(Value::as_bool)
}

fn parse_funnel_enabled(value: &Value) -> Option<bool> {
    if let Some(enabled) = value.pointer("/Funnel/Enabled").and_then(Value::as_bool) {
        return Some(enabled);
    }

    if let Some(enabled) = value
        .pointer("/Self/Capabilities/Funnel")
        .and_then(Value::as_bool)
    {
        return Some(enabled);
    }

    if let Some(capabilities) = value
        .pointer("/Self/Capabilities")
        .and_then(Value::as_array)
    {
        let has_funnel = capabilities.iter().any(|entry| {
            entry
                .as_str()
                .map(|value| value.eq_ignore_ascii_case("funnel"))
                .unwrap_or(false)
        });
        return Some(has_funnel);
    }

    if let Some(capabilities) = value
        .pointer("/Self/Capabilities")
        .and_then(Value::as_object)
    {
        if let Some(enabled) = capabilities.get("Funnel").and_then(Value::as_bool) {
            return Some(enabled);
        }
    }

    None
}

fn trim_trailing_dot(input: &str) -> &str {
    input.strip_suffix('.').unwrap_or(input)
}

fn ensure_version_supported(version: Option<&str>) -> Result<()> {
    let version = version.ok_or_else(|| FunnelError::VersionTooOld {
        source: None,
        context: "tailscaled version missing".to_string(),
    })?;
    let parsed = parse_version(version).ok_or_else(|| FunnelError::VersionTooOld {
        source: None,
        context: format!("Unsupported tailscaled version {}", version),
    })?;
    if parsed < MIN_SUPPORTED_VERSION {
        return Err(FunnelError::VersionTooOld {
            source: None,
            context: format!("tailscaled version {} is not supported", version),
        });
    }
    Ok(())
}

fn parse_version(version: &str) -> Option<(u32, u32, u32)> {
    let mut parts = version.split(['.', '-']);
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next().unwrap_or("0").parse().ok()?;
    Some((major, minor, patch))
}

fn build_url(dns_name: &str, https_port: u16, path: &str) -> Result<url::Url> {
    let base = if https_port == 443 {
        format!("https://{}", dns_name)
    } else {
        format!("https://{}:{}", dns_name, https_port)
    };
    let mut url = url::Url::parse(&base)
        .map_err(|err| FunnelError::Other(format!("Failed to build URL: {}", err)))?;
    url.set_path(path);
    Ok(url)
}

fn map_transport_error(err: LocalApiError) -> FunnelError {
    match err {
        LocalApiError::HttpStatus {
            status,
            method,
            path,
            body,
        } => {
            if status == hyper::StatusCode::UNAUTHORIZED || status == hyper::StatusCode::FORBIDDEN {
                return FunnelError::Permission {
                    source: None,
                    context: format!("LocalAPI auth rejected for {} {}", method, path),
                };
            }
            if status == hyper::StatusCode::NOT_FOUND {
                return FunnelError::VersionTooOld {
                    source: None,
                    context: format!("LocalAPI endpoint {} {} not found", method, path),
                };
            }
            FunnelError::ApplyFailed {
                source: None,
                context: format!("LocalAPI {} {} failed: {}", method, path, body),
            }
        }
        LocalApiError::PasswordPermissions { path, mode } => FunnelError::InvalidArgument(format!(
            "LocalAPI password file {} must be 0600 (got {:03o})",
            path.display(),
            mode
        )),
        LocalApiError::PasswordRead { path, source } => FunnelError::InvalidArgument(format!(
            "LocalAPI password file {} could not be read: {}",
            path.display(),
            source
        )),
        LocalApiError::EmptyPasswordFile { path } => FunnelError::InvalidArgument(format!(
            "LocalAPI password file {} is empty",
            path.display()
        )),
        LocalApiError::MissingSessionId => FunnelError::ApplyFailed {
            source: None,
            context: "watch-ipn-bus did not provide a session id".to_string(),
        },
        LocalApiError::Io(err) => FunnelError::Unreachable {
            source: Some(Box::new(err)),
            context: "LocalAPI unreachable".to_string(),
        },
        LocalApiError::InvalidUri(err) => FunnelError::ApplyFailed {
            source: Some(Box::new(err)),
            context: "Invalid LocalAPI URI".to_string(),
        },
        LocalApiError::Http(err) => FunnelError::ApplyFailed {
            source: Some(Box::new(err)),
            context: "LocalAPI request failed".to_string(),
        },
        LocalApiError::Hyper(err) => FunnelError::ApplyFailed {
            source: Some(Box::new(err)),
            context: "LocalAPI response error".to_string(),
        },
        LocalApiError::Json(err) => FunnelError::ApplyFailed {
            source: Some(Box::new(err)),
            context: "Failed to parse LocalAPI JSON".to_string(),
        },
        LocalApiError::InvalidHeaderValue { name } => FunnelError::ApplyFailed {
            source: None,
            context: format!("Invalid header value for {}", name),
        },
    }
}

async fn resolve_socket_addr(target: &LocalTarget) -> Result<SocketAddr> {
    let host = target.bind.clone();
    let port = target.port;
    let mut addrs = tokio::net::lookup_host((host.as_str(), port))
        .await
        .map_err(|err| FunnelError::TargetPortInaccessible {
            source: Some(Box::new(err)),
            context: format!("Failed to resolve {}", target),
        })?;
    addrs
        .next()
        .ok_or_else(|| FunnelError::TargetPortInaccessible {
            source: None,
            context: format!("No address resolved for {}", target),
        })
}
