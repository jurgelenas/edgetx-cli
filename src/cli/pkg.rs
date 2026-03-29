use anyhow::{Result, bail};
use clap::{Args, Subcommand};
use indicatif::{ProgressBar, ProgressStyle};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::packages;
use crate::radio;
use crate::source::PackageRef;

use super::backup::print_sd_card_info;

#[derive(Subcommand)]
pub enum PkgCommands {
    /// Install a package to the SD card
    Install(InstallArgs),
    /// Update installed package(s)
    Update(UpdateArgs),
    /// Remove an installed package from the SD card
    Remove(RemoveArgs),
    /// List installed packages
    List(ListArgs),
}

#[derive(Args)]
pub struct InstallArgs {
    /// Package reference (e.g., Org/Repo, Org/Repo@v1.0, ./local)
    package: String,

    /// SD card directory (auto-detect if not set)
    #[arg(long)]
    dir: Option<String>,

    /// Safely unmount radio after install
    #[arg(long)]
    eject: bool,

    /// Show what would be installed without writing anything
    #[arg(long)]
    dry_run: bool,

    /// Include development dependencies
    #[arg(long)]
    dev: bool,

    /// Manifest file or subdirectory within the repo
    #[arg(long)]
    path: Option<String>,
}

#[derive(Args)]
pub struct UpdateArgs {
    /// Package to update (source, name, or omit with --all)
    package: Option<String>,

    /// SD card directory (auto-detect if not set)
    #[arg(long)]
    dir: Option<String>,

    /// Manifest file or subdirectory within the repo
    #[arg(long)]
    path: Option<String>,

    /// Update all installed packages
    #[arg(long)]
    all: bool,

    /// Safely unmount radio after update
    #[arg(long)]
    eject: bool,

    /// Show what would be updated without writing anything
    #[arg(long)]
    dry_run: bool,

    /// Include development dependencies
    #[arg(long)]
    dev: bool,
}

#[derive(Args)]
pub struct RemoveArgs {
    /// Package to remove (source or name)
    package: String,

    /// SD card directory (auto-detect if not set)
    #[arg(long)]
    dir: Option<String>,

    /// Manifest file or subdirectory within the repo
    #[arg(long)]
    path: Option<String>,

    /// Safely unmount radio after removal
    #[arg(long)]
    eject: bool,

    /// Show what would be removed without deleting anything
    #[arg(long)]
    dry_run: bool,
}

#[derive(Args)]
pub struct ListArgs {
    /// SD card directory (auto-detect if not set)
    #[arg(long)]
    dir: Option<String>,
}

pub fn dispatch(command: PkgCommands) -> Result<()> {
    match command {
        PkgCommands::Install(args) => run_install(args),
        PkgCommands::Update(args) => run_update(args),
        PkgCommands::Remove(args) => run_remove(args),
        PkgCommands::List(args) => run_list(args),
    }
}

pub fn resolve_sd_root(dir_flag: &Option<String>) -> Result<PathBuf> {
    if let Some(dir) = dir_flag {
        let path = PathBuf::from(dir);
        if !path.is_dir() {
            bail!("directory {:?} does not exist or is not a directory", dir);
        }
        // Auto-create RADIO/ subdir for state file if needed
        let _ = std::fs::create_dir_all(path.join("RADIO"));
        return Ok(path);
    }

    let media_dir = crate::device::detect::default_media_dir()?;

    println!(
        "  {} Waiting for EdgeTX radio...",
        console::style("⏳").yellow()
    );

    let timeout = Duration::from_secs(60);
    match radio::detect::wait_for_mount(&media_dir, timeout) {
        Ok(sd_root) => {
            println!(
                "  {} EdgeTX radio detected at {}",
                console::style("✓").green(),
                sd_root.display()
            );
            Ok(sd_root)
        }
        Err(e) => {
            println!("  {} No EdgeTX radio detected", console::style("✗").red());
            Err(e.into())
        }
    }
}

fn run_install(args: InstallArgs) -> Result<()> {
    let mut pkg_ref: PackageRef = args.package.parse().map_err(|e| anyhow::anyhow!("{e}"))?;

    // --path flag overrides inline ::
    if let Some(p) = &args.path {
        pkg_ref.set_sub_path(p.clone());
    }

    let sd_root = resolve_sd_root(&args.dir)?;
    print_sd_card_info(&sd_root);

    if args.dry_run {
        println!(
            "  {} Dry-run mode: no files will be written",
            console::style("⚠").yellow()
        );
        println!();
    }

    // Prepare
    let canonical = pkg_ref.canonical();
    if !pkg_ref.is_local() {
        println!(
            "  {} Fetching {}...",
            console::style("⏳").yellow(),
            canonical
        );
    }

    let cmd = packages::install::InstallCommand::resolve(packages::install::InstallOptions {
        sd_root: sd_root.clone(),
        pkg_ref: pkg_ref.clone(),
        dev: args.dev,
    })?;

    if !pkg_ref.is_local() {
        println!(
            "  {} Fetched {}",
            console::style("✓").green(),
            cmd.package.name
        );
    }

    // Header
    println!();
    println!("  {}", console::style(&cmd.package.name).bold());
    if !cmd.manifest.package.description.is_empty() {
        println!("  {}", cmd.manifest.package.description);
    }
    println!();

    // Progress bar
    let total_files = cmd.total_files();
    let bar = ProgressBar::new(total_files as u64);
    bar.set_style(
        ProgressStyle::with_template("{spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} {msg}")
            .unwrap(),
    );
    bar.set_message("Installing");

    let result = cmd.execute(&sd_root, args.dry_run, |dest| {
        if let Some(name) = Path::new(dest).file_name() {
            bar.set_message(name.to_string_lossy().to_string());
        }
        bar.inc(1);
    })?;
    bar.finish_and_clear();

    println!();
    if args.dry_run {
        println!(
            "  {} Dry-run: would install {} file(s) to {}",
            console::style("⚠").yellow(),
            total_files,
            sd_root.display()
        );
    } else {
        println!(
            "  {} Installed {} file(s) to {}",
            console::style("✓").green(),
            result.files_copied,
            sd_root.display()
        );
    }

    print_channel_info(&result.package);

    if args.eject && !args.dry_run {
        crate::device::eject::eject(&sd_root)?;
    }

    Ok(())
}

fn run_update(args: UpdateArgs) -> Result<()> {
    let sd_root = resolve_sd_root(&args.dir)?;
    print_sd_card_info(&sd_root);

    let query = match &args.package {
        Some(q) => {
            let pkg_ref: PackageRef = q.parse().map_err(|e| anyhow::anyhow!("{e}"))?;
            let pkg_ref = pkg_ref.with_sub_path(args.path.as_deref().unwrap_or(""));
            pkg_ref.full()
        }
        None => String::new(),
    };

    if args.dry_run {
        println!(
            "  {} Dry-run mode: no files will be written",
            console::style("⚠").yellow()
        );
        println!();
    }

    println!(
        "  {} Checking for updates...",
        console::style("⏳").yellow()
    );

    let dev_set = args.package.is_some(); // simplified: dev is explicitly set if package is given
    let results = packages::update::update(packages::update::UpdateOptions {
        sd_root: sd_root.clone(),
        query,
        all: args.all,
        dev: args.dev,
        dev_set,
        dry_run: args.dry_run,
        before_copy: None,
        on_file: None,
    })?;

    println!();
    for r in &results {
        if r.up_to_date {
            println!(
                "  {} {} ({}) is already up to date",
                console::style("ℹ").blue(),
                r.package.name,
                r.package.source
            );
            continue;
        }

        let mut info = format!("{} -> {}", r.package.source, r.package.channel);
        if !r.package.version.is_empty() {
            info = format!("{info} {}", r.package.version);
        }
        if r.package.commit.len() > 7 {
            info = format!("{info} ({})", &r.package.commit[..7]);
        }

        if r.files_copied > 0 {
            println!(
                "  {} Updated {}: {} file(s) copied",
                console::style("✓").green(),
                info,
                r.files_copied
            );
        } else {
            println!("  {} Would update {}", console::style("⚠").yellow(), info);
        }
    }

    if args.eject && !args.dry_run {
        crate::device::eject::eject(&sd_root)?;
    }

    Ok(())
}

fn run_remove(args: RemoveArgs) -> Result<()> {
    let sd_root = resolve_sd_root(&args.dir)?;
    print_sd_card_info(&sd_root);

    if args.dry_run {
        println!(
            "  {} Dry-run mode: no files will be deleted",
            console::style("⚠").yellow()
        );
        println!();
    }

    let query = {
        let pkg_ref: PackageRef = args.package.parse().map_err(|e| anyhow::anyhow!("{e}"))?;
        let pkg_ref = pkg_ref.with_sub_path(args.path.as_deref().unwrap_or(""));
        pkg_ref.full()
    };

    let cmd = packages::remove::prepare_remove(packages::remove::RemoveOptions {
        sd_root: sd_root.clone(),
        query,
    })?;

    println!();
    println!("  {}", console::style(&cmd.package.name).bold());
    println!();

    if args.dry_run {
        let result = cmd.execute(true, |_| {})?;
        println!(
            "  {} Would remove the following paths:",
            console::style("⚠").yellow()
        );
        for p in &result.package.paths {
            println!("    {p}");
        }
    } else {
        let total = cmd.total_files();
        let bar = ProgressBar::new(total as u64);
        bar.set_style(
            ProgressStyle::with_template("{spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} {msg}")
                .unwrap(),
        );
        bar.set_message("Removing");

        let result = cmd.execute(false, |path| {
            if let Some(name) = Path::new(path).file_name() {
                bar.set_message(name.to_string_lossy().to_string());
            }
            bar.inc(1);
        })?;
        bar.finish_and_clear();

        println!();
        println!(
            "  {} Removed {} ({}) - {} file(s)",
            console::style("✓").green(),
            result.package.name,
            result.package.source,
            result.files_removed
        );
        for p in &result.package.paths {
            println!("    {p}");
        }
    }

    if args.eject && !args.dry_run {
        crate::device::eject::eject(&sd_root)?;
    }

    Ok(())
}

fn run_list(args: ListArgs) -> Result<()> {
    let sd_root = resolve_sd_root(&args.dir)?;
    print_sd_card_info(&sd_root);

    let state = packages::state::load_state(&sd_root)?;

    if state.packages.is_empty() {
        println!("  {} No packages installed", console::style("ℹ").blue());
        return Ok(());
    }

    println!();
    println!(
        "  {}",
        console::style(format!("Installed Packages ({})", state.packages.len())).bold()
    );
    println!();
    println!(
        "  {:<30} {:<20} {:<10} {:<12} Commit",
        "Source", "Name", "Channel", "Version"
    );
    println!("  {}", "-".repeat(80));

    for pkg in &state.packages {
        let commit = if pkg.commit.len() > 7 {
            &pkg.commit[..7]
        } else {
            &pkg.commit
        };
        println!(
            "  {:<30} {:<20} {:<10} {:<12} {}",
            pkg.source, pkg.name, pkg.channel, pkg.version, commit
        );
    }

    Ok(())
}

fn print_channel_info(pkg: &packages::state::InstalledPackage) {
    let mut info = pkg.channel.to_string();
    if !pkg.version.is_empty() {
        info = format!("{info} {}", pkg.version);
    }
    if pkg.commit.len() > 7 {
        info = format!("{info} ({})", &pkg.commit[..7]);
    }
    println!("  {} Channel: {}", console::style("ℹ").blue(), info);
}
