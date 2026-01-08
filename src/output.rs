use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::io::{self, IsTerminal, Write};

pub fn use_color() -> bool {
    std::io::stdout().is_terminal() && supports_color::on(supports_color::Stream::Stdout).is_some()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    Started {
        version: u32,
        url: String,
        local_target: String,
        path: String,
        https_port: u16,
        started_at: DateTime<Utc>,
        expires_at: Option<DateTime<Utc>>,
    },
    Stopped {
        version: u32,
        reason: StopReason,
        stopped_at: DateTime<Utc>,
        duration_seconds: Option<u64>,
    },
    Error {
        version: u32,
        code: i32,
        message: String,
        suggestion: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    UserInterrupt,
    TtlExpired,
    Error,
}

impl Event {
    pub fn emit_json(&self) -> io::Result<()> {
        let mut stdout = io::stdout();
        serde_json::to_writer(&mut stdout, self).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("JSON serialization failed: {}", e),
            )
        })?;
        writeln!(stdout)?;
        stdout.flush()
    }
}

pub struct HumanOutput {
    use_color: bool,
}

impl HumanOutput {
    pub fn new() -> Self {
        Self {
            use_color: use_color(),
        }
    }

    pub fn print_started(
        &self,
        url: &str,
        local_target: &str,
        expires_at: Option<DateTime<Utc>>,
    ) -> io::Result<()> {
        let mut stdout = io::stdout();

        writeln!(stdout, "{}", url)?;

        let (branch, local_label, expires_label, ctrl_label) = if self.use_color {
            (
                "\x1b[2m├─\x1b[0m",
                "\x1b[1mLocal:\x1b[0m",
                "\x1b[1mExpires:\x1b[0m",
                "\x1b[1mPress Ctrl-C to stop\x1b[0m",
            )
        } else {
            ("├─", "Local:", "Expires:", "Press Ctrl-C to stop")
        };

        writeln!(stdout, "{} {} {}", branch, local_label, local_target)?;

        let expiry_text = if let Some(exp) = expires_at {
            format!("{}", exp.format("%Y-%m-%d %H:%M:%S UTC"))
        } else {
            "never (Ctrl-C to stop)".to_string()
        };

        let last_branch = if self.use_color {
            "\x1b[2m└─\x1b[0m"
        } else {
            "└─"
        };

        writeln!(stdout, "{} {} {}", branch, expires_label, expiry_text)?;
        writeln!(stdout, "{} {}", last_branch, ctrl_label)?;

        stdout.flush()
    }

    pub fn print_stopped(
        &self,
        reason: StopReason,
        duration_seconds: Option<u64>,
    ) -> io::Result<()> {
        let mut stderr = io::stderr();

        let reason_text = match reason {
            StopReason::UserInterrupt => "Stopped by user",
            StopReason::TtlExpired => "TTL expired",
            StopReason::Error => "Stopped due to error",
        };

        let duration_text = if let Some(secs) = duration_seconds {
            format!(" (ran for {}s)", secs)
        } else {
            String::new()
        };

        writeln!(stderr, "{}{}", reason_text, duration_text)?;
        stderr.flush()
    }
}

impl Default for HumanOutput {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_serialization() {
        let event = Event::Started {
            version: 1,
            url: "https://node.tailnet.ts.net/funnelctl/abc123".to_string(),
            local_target: "http://127.0.0.1:8081".to_string(),
            path: "/funnelctl/abc123".to_string(),
            https_port: 443,
            started_at: Utc::now(),
            expires_at: None,
        };

        let json = serde_json::to_string(&event).expect("Failed to serialize");
        assert!(json.contains("\"event\":\"started\""));
        assert!(json.contains("\"version\":1"));
    }

    #[test]
    fn test_stopped_event() {
        let event = Event::Stopped {
            version: 1,
            reason: StopReason::UserInterrupt,
            stopped_at: Utc::now(),
            duration_seconds: Some(1800),
        };

        let json = serde_json::to_string(&event).expect("Failed to serialize");
        assert!(json.contains("\"event\":\"stopped\""));
        assert!(json.contains("\"reason\":\"user_interrupt\""));
    }

    #[test]
    fn test_error_event() {
        let event = Event::Error {
            version: 1,
            code: 10,
            message: "LocalAPI unreachable".to_string(),
            suggestion: Some("Is tailscaled running?".to_string()),
        };

        let json = serde_json::to_string(&event).expect("Failed to serialize");
        assert!(json.contains("\"event\":\"error\""));
        assert!(json.contains("\"code\":10"));
    }
}
