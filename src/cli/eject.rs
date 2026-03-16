use anyhow::Result;
use clap::Args;

use super::backup::print_sd_card_info;

#[derive(Args)]
pub struct EjectArgs {
    /// SD card directory (auto-detect if not set)
    #[arg(long)]
    dir: Option<String>,
}

pub fn run(args: EjectArgs) -> Result<()> {
    let sd_root = super::pkg::resolve_sd_root(&args.dir)?;
    print_sd_card_info(&sd_root);

    crate::device::eject::eject(&sd_root)?;

    println!(
        "  {} Radio safely ejected",
        console::style("✓").green(),
    );

    Ok(())
}
