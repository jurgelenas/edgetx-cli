pub mod backup;
pub mod dev;
pub mod pkg;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "edgetx-cli")]
#[command(about = "CLI tool for managing EdgeTX radios")]
#[command(long_about = "A command-line interface for managing EdgeTX radio SD cards, packages, and configurations.")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    /// Enable debug logging
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// Log output format (text, json)
    #[arg(long, global = true, default_value = "text")]
    pub log_format: String,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Back up an EdgeTX radio's SD card contents
    Backup(backup::BackupArgs),

    /// Package management commands
    Pkg {
        #[command(subcommand)]
        command: pkg::PkgCommands,
    },

    /// Development workflow commands
    Dev {
        #[command(subcommand)]
        command: dev::DevCommands,
    },
}

pub fn dispatch(cli: Cli) -> anyhow::Result<()> {
    match cli.command {
        Commands::Backup(args) => backup::run(args),
        Commands::Pkg { command } => pkg::dispatch(command),
        Commands::Dev { command } => dev::dispatch(command),
    }
}
