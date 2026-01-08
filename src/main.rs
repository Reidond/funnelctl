use clap::Parser;
use std::sync::Arc;

use funnelctl::backend::{localapi::LocalApiBackend, UnreachableBackend};
use funnelctl::cli::{Cli, Commands};
use funnelctl::cmd::{CloseCommand, CompletionsCommand, DoctorCommand, OpenCommand, StatusCommand};
use funnelctl::error::FunnelError;
use funnelctl::output::{self, Event};

#[tokio::main]
async fn main() {
    let exit_code = match run().await {
        Ok(()) => 0,
        Err((err, json_mode)) => {
            if json_mode {
                let event = Event::Error {
                    version: 1,
                    code: err.exit_code(),
                    message: err.to_string(),
                    suggestion: err.get_fix(),
                };
                let _ = event.emit_json();
            } else {
                let use_color = output::use_color();
                eprintln!("{}", err.format_detailed(use_color));
            }
            err.exit_code()
        }
    };

    std::process::exit(exit_code);
}

async fn run() -> Result<(), (FunnelError, bool)> {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(err) => return Err((map_parse_error(err), false)),
    };

    if let Err(err) = init_tracing(cli.verbose) {
        return Err((err, false));
    }

    let json_mode = matches!(cli.command, Commands::Open(ref args) if args.json);

    match cli.command {
        Commands::Open(args) => {
            let transport = LocalApiBackend::build_transport(
                args.socket.clone(),
                args.localapi_port,
                args.localapi_password_file.clone(),
            )
            .map_err(|err| (err, json_mode))?;
            let backend = Arc::new(LocalApiBackend::new(transport, args.force));
            let cmd = OpenCommand::new(args);
            cmd.run(backend, json_mode)
                .await
                .map_err(|err| (err, json_mode))
        }
        Commands::Doctor(args) => {
            let tcp_mode = args.localapi_port.is_some();
            let backend: Arc<dyn funnelctl::backend::Backend> =
                match LocalApiBackend::build_transport(
                    args.socket.clone(),
                    args.localapi_port,
                    args.localapi_password_file.clone(),
                ) {
                    Ok(transport) => Arc::new(LocalApiBackend::new(transport, false)),
                    Err(err) if !tcp_mode => match err {
                        FunnelError::Unreachable { context, .. } => {
                            Arc::new(UnreachableBackend::new(context))
                        }
                        other => return Err((other, false)),
                    },
                    Err(err) => return Err((err, false)),
                };
            DoctorCommand::run(backend, tcp_mode)
                .await
                .map_err(|err| (err, false))
        }
        Commands::Close => CloseCommand::run().await.map_err(|err| (err, false)),
        Commands::Status => StatusCommand::run().await.map_err(|err| (err, false)),
        Commands::Completions(args) => {
            let cmd = CompletionsCommand { shell: args.shell };
            cmd.run().map_err(|err| (err, false))
        }
    }
}

fn map_parse_error(err: clap::Error) -> FunnelError {
    use clap::error::ErrorKind;
    if matches!(
        err.kind(),
        ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
    ) {
        let _ = err.print();
        std::process::exit(0);
    }
    FunnelError::InvalidArgument(err.to_string())
}

fn init_tracing(verbose: u8) -> Result<(), FunnelError> {
    use tracing_subscriber::EnvFilter;

    let filter = match std::env::var("RUST_LOG") {
        Ok(value) if !value.trim().is_empty() => EnvFilter::try_new(value)
            .map_err(|err| FunnelError::Other(format!("Invalid RUST_LOG value: {}", err)))?,
        _ => {
            let level = match verbose {
                0 => "error",
                1 => "info",
                2 => "debug",
                _ => "trace",
            };
            EnvFilter::new(level)
        }
    };

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_target(false)
        .without_time()
        .try_init()
        .map_err(|err| FunnelError::Other(format!("Failed to initialize logging: {}", err)))
}
