use crate::error::{FunnelError, Result};

pub struct StatusCommand;

impl StatusCommand {
    pub async fn run() -> Result<()> {
        Err(FunnelError::Other(
            "status command not yet implemented (Phase 2 feature)".to_string(),
        ))
    }
}
