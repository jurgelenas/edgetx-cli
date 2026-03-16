mod cli;
mod device;
mod error;
mod manifest;
mod packages;
mod radio;
mod radio_catalog;
mod source;
mod scaffold;
mod simulator;
mod simulator_ui;
mod sync;

use clap::Parser;
use cli::Cli;

fn main() {
    let cli = Cli::parse();

    let level = if cli.verbose { "debug" } else { "info" };
    env_logger::Builder::new()
        .filter_level(level.parse().unwrap_or(log::LevelFilter::Info))
        .format_timestamp(if cli.log_format == "json" {
            None
        } else {
            Some(env_logger::fmt::TimestampPrecision::Seconds)
        })
        .init();

    if let Err(e) = cli::dispatch(cli) {
        eprintln!("Error: {e:#}");
        // Print full error chain
        let mut source = e.source();
        while let Some(cause) = source {
            eprintln!("Caused by: {cause}");
            source = std::error::Error::source(cause);
        }
        std::process::exit(1);
    }
}
