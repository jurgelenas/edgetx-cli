use anyhow::{Context, Result, bail};
use clap::Args;
use indicatif::{ProgressBar, ProgressStyle};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::radio;

#[derive(Args)]
pub struct BackupArgs {
    /// Create a .zip archive instead of a directory
    #[arg(long)]
    compress: bool,

    /// Output directory for the backup
    #[arg(long, default_value = ".")]
    directory: String,

    /// Custom backup name prefix (date is always appended)
    #[arg(long)]
    name: Option<String>,

    /// Safely unmount radio after backup
    #[arg(long)]
    eject: bool,
}

pub fn run(args: BackupArgs) -> Result<()> {
    let out_dir = std::fs::canonicalize(&args.directory)
        .with_context(|| format!("resolving output directory {:?}", args.directory))?;
    if !out_dir.is_dir() {
        bail!("output path {:?} is not a directory", out_dir);
    }

    let now = SystemTime::now();
    let date_suffix = format_date(now);

    let name = match &args.name {
        Some(n) => format!("{n}-{date_suffix}"),
        None => format!("backup-{date_suffix}"),
    };

    // Detect radio
    let radio_dir = super::pkg::resolve_sd_root(&None)?;

    print_sd_card_info(&radio_dir);

    println!();
    console::Term::stdout()
        .write_line(&format!("  Backup"))
        .ok();
    println!();

    let total_files = radio::backup::count_all_files(&radio_dir);
    let dest_dir = out_dir.join(&name);
    std::fs::create_dir_all(&dest_dir).context("creating backup directory")?;

    let bar = ProgressBar::new(total_files as u64);
    bar.set_style(
        ProgressStyle::with_template("{spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} {msg}")
            .unwrap(),
    );
    bar.set_message("Backing up files");

    let copied = radio::backup::backup_dir(&radio_dir, &dest_dir, |dest| {
        if let Some(name) = Path::new(dest).file_name() {
            bar.set_message(name.to_string_lossy().to_string());
        }
        bar.inc(1);
    })?;
    bar.finish_and_clear();

    let mut output_path = dest_dir.clone();

    if args.compress {
        let zip_path = PathBuf::from(format!("{}.zip", dest_dir.display()));

        let zip_total = radio::backup::count_all_files(&dest_dir);
        let zip_bar = ProgressBar::new(zip_total as u64);
        zip_bar.set_style(
            ProgressStyle::with_template(
                "{spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} {msg}",
            )
            .unwrap(),
        );
        zip_bar.set_message("Compressing");

        radio::backup::compress_dir(&dest_dir, &zip_path, |rel| {
            if let Some(name) = Path::new(rel).file_name() {
                zip_bar.set_message(name.to_string_lossy().to_string());
            }
            zip_bar.inc(1);
        })?;
        zip_bar.finish_and_clear();

        output_path = zip_path;
    }

    println!(
        "  {} Backed up {} files to {}",
        console::style("✓").green(),
        copied,
        output_path.display()
    );

    if args.eject {
        crate::device::eject::eject(&radio_dir)?;
    }

    Ok(())
}

fn format_date(_now: SystemTime) -> String {
    // Simple date formatting without chrono
    let duration = _now
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = duration.as_secs();
    let days = secs / 86400;
    // Approximate date calculation
    let years = 1970 + days / 365;
    let remaining_days = days % 365;
    let month = remaining_days / 30 + 1;
    let day = remaining_days % 30 + 1;
    format!("{years}-{month:02}-{day:02}")
}

pub fn print_sd_card_info(sd_root: &Path) {
    let version_file = sd_root.join("edgetx.sdcard.version");
    if let Ok(version) = std::fs::read_to_string(&version_file) {
        let sd_version = version.trim();
        println!(
            "  {} SD card at {} (v{})",
            console::style("ℹ").blue(),
            sd_root.display(),
            sd_version
        );
    } else {
        println!(
            "  {} SD card at {}",
            console::style("ℹ").blue(),
            sd_root.display()
        );
    }
}

// Re-export for use by other CLI modules
pub use print_sd_card_info as print_info;
