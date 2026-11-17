use anyhow::Context;
use clap::Parser;
use miden_faucet::{Cli, logging, run_faucet_command};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Configure tracing with optional OpenTelemetry exporting support.
    let _otel_guard = logging::setup_tracing(cli.command.open_telemetry())
        .context("failed to initialize logging")?;

    Box::pin(run_faucet_command(cli)).await
}
