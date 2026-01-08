use thiserror::Error;

#[derive(Debug, Error)]
pub enum FunnelError {
    #[error("LocalAPI unreachable")]
    Unreachable {
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
        context: String,
    },

    #[error("Permission denied")]
    Permission {
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
        context: String,
    },

    #[error("Prerequisites not met")]
    Prerequisites {
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
        context: String,
    },

    #[error("Configuration conflict")]
    Conflict {
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
        context: String,
    },

    #[error("Apply operation failed")]
    ApplyFailed {
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
        context: String,
    },

    #[error("Target port inaccessible")]
    TargetPortInaccessible {
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
        context: String,
    },

    #[error("Version too old")]
    VersionTooOld {
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
        context: String,
    },

    #[error("Invalid argument: {0}")]
    InvalidArgument(String),

    #[error("{0}")]
    Other(String),
}

impl FunnelError {
    pub fn exit_code(&self) -> i32 {
        match self {
            FunnelError::Unreachable { .. } => 10,
            FunnelError::Permission { .. } => 11,
            FunnelError::Prerequisites { .. } => 12,
            FunnelError::Conflict { .. } => 13,
            FunnelError::ApplyFailed { .. } => 14,
            FunnelError::TargetPortInaccessible { .. } => 15,
            FunnelError::VersionTooOld { .. } => 16,
            FunnelError::InvalidArgument(_) => 2,
            FunnelError::Other(_) => 1,
        }
    }

    pub fn format_detailed(&self, use_color: bool) -> String {
        let (error_label, cause_label, fix_label) = if use_color {
            (
                "\x1b[1;31mError:\x1b[0m",
                "\x1b[1;33mCause:\x1b[0m",
                "\x1b[1;32mFix:\x1b[0m",
            )
        } else {
            ("Error:", "Cause:", "Fix:")
        };

        let (cause, fix) = self.get_cause_and_fix();

        let mut output = format!("{} {}", error_label, self);

        if let Some(cause_text) = cause {
            output.push_str(&format!("\n{} {}", cause_label, cause_text));
        }

        if let Some(fix_text) = fix {
            output.push_str(&format!("\n{} {}", fix_label, fix_text));
        }

        output
    }

    pub fn get_fix(&self) -> Option<String> {
        let (_, fix) = self.get_cause_and_fix();
        fix
    }

    fn get_cause_and_fix(&self) -> (Option<String>, Option<String>) {
        match self {
            FunnelError::Unreachable { context, .. } => (
                Some(context.clone()),
                Some("Is tailscaled running? Try: sudo systemctl start tailscaled".to_string()),
            ),
            FunnelError::Permission { context, .. } => (
                Some(context.clone()),
                Some("Run with sudo or add your user to the operator group".to_string()),
            ),
            FunnelError::Prerequisites { context, .. } => (
                Some(context.clone()),
                Some("Run 'funnelctl doctor' to diagnose the issue".to_string()),
            ),
            FunnelError::Conflict { context, .. } => (
                Some(context.clone()),
                Some("Use a different --path or add --force to override".to_string()),
            ),
            FunnelError::ApplyFailed { context, .. } => (
                Some(context.clone()),
                Some(
                    "Check tailscaled logs for more details. Route may still exist; run `tailscale serve off` to clean up."
                        .to_string(),
                ),
            ),
            FunnelError::TargetPortInaccessible { context, .. } => (
                Some(context.clone()),
                Some("Start your service before running funnelctl".to_string()),
            ),
            FunnelError::VersionTooOld { context, .. } => (
                Some(context.clone()),
                Some("Upgrade tailscaled. See https://tailscale.com/download".to_string()),
            ),
            FunnelError::InvalidArgument(msg) => (Some(msg.clone()), None),
            FunnelError::Other(msg) => (Some(msg.clone()), None),
        }
    }
}

pub type Result<T> = std::result::Result<T, FunnelError>;
