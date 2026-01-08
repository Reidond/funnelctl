use crate::error::{FunnelError, Result};

pub struct CloseCommand;

impl CloseCommand {
    pub async fn run() -> Result<()> {
        Err(FunnelError::Other(
            "close command not yet implemented (MVP uses foreground sessions only)".to_string(),
        ))
    }
}
