use std::net::IpAddr;
use std::sync::Arc;

use chrono::Utc;
use humantime::format_duration;
use rand::distributions::Alphanumeric;
use rand::Rng;
use tokio::signal;
use tokio::time::{sleep_until, Duration, Instant};

use crate::backend::Backend;
use crate::cli::OpenArgs;
use crate::core::{
    validate_https_port, validate_path, validate_port, validate_ttl, LocalTarget, TunnelSpec,
    ValidationWarning,
};
use crate::error::{FunnelError, Result};
use crate::lock::LockGuard;
use crate::output::{Event, HumanOutput, StopReason};

pub struct OpenCommand {
    args: OpenArgs,
}

impl OpenCommand {
    pub fn new(args: OpenArgs) -> Self {
        Self { args }
    }

    pub async fn run(self, backend: Arc<dyn Backend>, json: bool) -> Result<()> {
        validate_port(self.args.port)?;
        validate_https_port(self.args.https_port)?;

        let bind_ip = resolve_bind(&self.args.bind, self.args.allow_non_loopback).await?;

        let path = self.args.path.unwrap_or_else(generate_random_path);
        let path_result = validate_path(&path)?;
        for warning in &path_result.warnings {
            emit_warning(warning, json);
        }
        let path = path_result.normalized_path;

        let ttl = match self.args.ttl.as_deref() {
            Some(value) => Some(parse_ttl(value)?),
            None => None,
        };
        if let Some(ttl) = ttl {
            let ttl_result = validate_ttl(ttl)?;
            for warning in ttl_result.warnings {
                emit_warning(&warning, json);
            }
        }

        let local_target = LocalTarget::new(bind_ip.to_string(), self.args.port);
        let spec = TunnelSpec::new(local_target, self.args.https_port, path.clone(), true);

        let result = {
            let _lock = LockGuard::acquire()?;
            backend.apply(&spec).await?
        };
        let started_at = result.applied_at;
        let expires_at = ttl
            .and_then(|ttl| chrono::Duration::from_std(ttl).ok())
            .map(|duration| started_at + duration);

        if json {
            let event = Event::Started {
                version: 1,
                url: result.url.to_string(),
                local_target: spec.local_target.to_string(),
                path: path.clone(),
                https_port: spec.https_port,
                started_at,
                expires_at,
            };
            event
                .emit_json()
                .map_err(|err| FunnelError::Other(err.to_string()))?;
        } else {
            let output = HumanOutput::new();
            let local_target = spec.local_target.to_string();
            output
                .print_started(result.url.as_str(), &local_target, expires_at)
                .map_err(|err| FunnelError::Other(err.to_string()))?;
        }

        let stop_reason = wait_for_stop(ttl).await;

        if matches!(stop_reason, StopReason::TtlExpired) && !json {
            eprintln!(
                "TTL expired ({}). Tearing down tunnel.",
                ttl.map(format_duration)
                    .unwrap_or_else(|| format_duration(Duration::from_secs(0)))
            );
        }

        let cleanup = backend.remove(&result.lease_id);
        let second_ctrl_c = signal::ctrl_c();
        let cleanup_result = tokio::select! {
            res = cleanup => res,
            _ = second_ctrl_c => {
                std::process::exit(130);
            }
        };
        cleanup_result.map_err(|err| FunnelError::ApplyFailed {
            source: Some(Box::new(err)),
            context: "Failed to tear down tunnel".to_string(),
        })?;

        let stopped_at = Utc::now();
        let duration_seconds = (stopped_at - started_at).num_seconds().max(0) as u64;

        if json {
            let event = Event::Stopped {
                version: 1,
                reason: stop_reason,
                stopped_at,
                duration_seconds: Some(duration_seconds),
            };
            event
                .emit_json()
                .map_err(|err| FunnelError::Other(err.to_string()))?;
        } else {
            let output = HumanOutput::new();
            output
                .print_stopped(stop_reason, Some(duration_seconds))
                .map_err(|err| FunnelError::Other(err.to_string()))?;
        }

        Ok(())
    }
}

fn generate_random_path() -> String {
    let token: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(8)
        .map(char::from)
        .collect();
    format!("/funnelctl/{token}")
}

async fn resolve_bind(bind: &str, allow_non_loopback: bool) -> Result<IpAddr> {
    let ip = if bind == "localhost" {
        resolve_localhost().await?
    } else {
        bind.parse::<IpAddr>().map_err(|_| {
            FunnelError::InvalidArgument(format!(
                "Invalid bind address '{}'. Use 127.0.0.1, ::1, or localhost",
                bind
            ))
        })?
    };

    if !allow_non_loopback && !ip.is_loopback() {
        return Err(FunnelError::InvalidArgument(
            "Non-loopback bind requires --allow-non-loopback".to_string(),
        ));
    }

    Ok(ip)
}

async fn resolve_localhost() -> Result<IpAddr> {
    let addrs = tokio::net::lookup_host(("localhost", 0))
        .await
        .map_err(|err| {
            FunnelError::InvalidArgument(format!("Failed to resolve localhost: {}", err))
        })?;
    let mut first = None;
    for addr in addrs {
        let ip = addr.ip();
        if first.is_none() {
            first = Some(ip);
        }
        if ip.is_ipv4() {
            return Ok(ip);
        }
    }
    first.ok_or_else(|| FunnelError::InvalidArgument("localhost did not resolve".to_string()))
}

fn parse_ttl(value: &str) -> Result<Duration> {
    humantime::parse_duration(value)
        .map_err(|err| FunnelError::InvalidArgument(format!("Invalid TTL '{}': {}", value, err)))
}

fn emit_warning(warning: &ValidationWarning, json: bool) {
    if json {
        return;
    }
    match warning {
        ValidationWarning::PathTooShort { path, .. } => {
            eprintln!(
                "Warning: Short path '{}' is guessable. Consider a longer path or use default random path.",
                path
            );
        }
        ValidationWarning::TtlTooShort { ttl } => {
            eprintln!(
                "Warning: Short TTL ({}). Tunnel expires quickly.",
                format_duration(*ttl)
            );
        }
    }
}

async fn wait_for_stop(ttl: Option<Duration>) -> StopReason {
    let ctrl_c = async {
        let _ = signal::ctrl_c().await;
        StopReason::UserInterrupt
    };

    let ttl_wait = async {
        match ttl {
            Some(ttl) => {
                let deadline = Instant::now() + ttl;
                sleep_until(deadline).await;
                StopReason::TtlExpired
            }
            None => futures::future::pending().await,
        }
    };

    tokio::select! {
        reason = ctrl_c => reason,
        reason = ttl_wait => reason,
    }
}
