use std::path::PathBuf;

use clap::{ArgAction, Args, Parser, Subcommand};
use clap_complete::Shell;

const EXAMPLES: &str = "EXAMPLES:\n    funnelctl open 8081                    # Quick tunnel with random path\n    funnelctl open 8081 --path /webhook    # Custom path\n    funnelctl open 8081 --ttl 30m          # Auto-expire after 30 minutes\n";

#[derive(Parser, Debug)]
#[command(
    name = "funnelctl",
    version,
    about = "Short-lived public HTTPS tunnels via Tailscale Funnel",
    long_about = "Create short-lived public HTTPS tunnels to a local port using Tailscale Funnel (LocalAPI backend).",
    after_long_help = EXAMPLES,
    propagate_version = true,
    arg_required_else_help = true,
    disable_help_subcommand = true
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    #[arg(
        short = 'v',
        long = "verbose",
        action = ArgAction::Count,
        global = true,
        help = "Increase log verbosity (-v, -vv, -vvv)",
    )]
    pub verbose: u8,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    #[command(alias = "o", after_long_help = EXAMPLES)]
    Open(OpenArgs),
    #[command(alias = "doc")]
    Doctor(DoctorArgs),
    #[command(alias = "c")]
    Close,
    #[command(alias = "s")]
    Status,
    Completions(CompletionsArgs),
}

#[derive(Args, Debug)]
pub struct OpenArgs {
    #[arg(value_name = "port", help = "Local port on loopback")]
    pub port: u16,

    #[arg(
        long,
        default_value = "127.0.0.1",
        value_name = "ip",
        help = "Bind IP (127.0.0.1, ::1, localhost)"
    )]
    pub bind: String,

    #[arg(
        long,
        value_name = "path",
        help = "URL path (default: /funnelctl/<random>)"
    )]
    pub path: Option<String>,

    #[arg(
        long,
        default_value = "443",
        value_name = "port",
        help = "Public HTTPS port (443, 8443, or 10000)"
    )]
    pub https_port: u16,

    #[arg(
        long,
        value_name = "duration",
        help = "Keep tunnel up for duration, then tear down"
    )]
    pub ttl: Option<String>,

    #[arg(long, help = "Allow overwriting conflicting serve routes")]
    pub force: bool,

    #[arg(long, help = "NDJSON output for scripting")]
    pub json: bool,

    #[arg(long, value_name = "path", help = "Unix socket path override")]
    pub socket: Option<PathBuf>,

    #[arg(long, value_name = "port", help = "LocalAPI TCP port (macOS/Windows)")]
    pub localapi_port: Option<u16>,

    #[arg(
        long,
        value_name = "path",
        help = "File containing LocalAPI password (0600 permissions)"
    )]
    pub localapi_password_file: Option<PathBuf>,

    #[arg(long, help = "Allow non-loopback bind addresses")]
    pub allow_non_loopback: bool,
}

#[derive(Args, Debug)]
pub struct DoctorArgs {
    #[arg(long, value_name = "path", help = "Unix socket path override")]
    pub socket: Option<PathBuf>,

    #[arg(long, value_name = "port", help = "LocalAPI TCP port (macOS/Windows)")]
    pub localapi_port: Option<u16>,

    #[arg(
        long,
        value_name = "path",
        help = "File containing LocalAPI password (0600 permissions)"
    )]
    pub localapi_password_file: Option<PathBuf>,
}

#[derive(Args, Debug)]
pub struct CompletionsArgs {
    #[arg(value_enum, help = "Shell to generate completions for")]
    pub shell: Shell,
}
