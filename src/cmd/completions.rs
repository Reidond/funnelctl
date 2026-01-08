use clap::CommandFactory;
use clap_complete::{generate, Shell};
use std::io;

use crate::cli::Cli;
use crate::error::Result;

pub struct CompletionsCommand {
    pub shell: Shell,
}

impl CompletionsCommand {
    pub fn run(self) -> Result<()> {
        let mut cmd = Cli::command();
        let bin_name = cmd.get_name().to_string();
        generate(self.shell, &mut cmd, bin_name, &mut io::stdout());
        Ok(())
    }
}
