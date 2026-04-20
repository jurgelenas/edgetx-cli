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
    #[command(alias = "ls")]
    List(ListArgs),
    /// Show information about a remote package
    Info(InfoArgs),
    /// List installed packages that have updates available
    Outdated(OutdatedArgs),
}

#[derive(Args)]
pub struct InstallArgs {
    /// Package reference (e.g., Org/Repo, Org/Repo@v1.0, ./local)
    package: String,

    /// SD card directory (auto-detect if not set)
    #[arg(long)]
    dir: Option<PathBuf>,

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
    dir: Option<PathBuf>,

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
    dir: Option<PathBuf>,

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
    dir: Option<PathBuf>,
}

#[derive(Args)]
pub struct InfoArgs {
    /// Package reference (e.g., Org/Repo, Org/Repo@v1.0, ./local)
    package: String,

    /// Manifest file or subdirectory within the repo
    #[arg(long)]
    path: Option<String>,
}

#[derive(Args)]
pub struct OutdatedArgs {
    /// SD card directory (auto-detect if not set)
    #[arg(long)]
    dir: Option<PathBuf>,
}

pub fn dispatch(command: PkgCommands) -> Result<()> {
    match command {
        PkgCommands::Install(args) => run_install(args),
        PkgCommands::Update(args) => run_update(args),
        PkgCommands::Remove(args) => run_remove(args),
        PkgCommands::List(args) => run_list(args),
        PkgCommands::Info(args) => run_info(args),
        PkgCommands::Outdated(args) => run_outdated(args),
    }
}

pub fn resolve_sd_root(dir_flag: &Option<PathBuf>) -> Result<PathBuf> {
    if let Some(dir) = dir_flag {
        if !dir.is_dir() {
            bail!(
                "directory {} does not exist or is not a directory",
                dir.display()
            );
        }
        // Auto-create RADIO/ subdir for state file if needed
        let _ = std::fs::create_dir_all(dir.join("RADIO"));
        return Ok(dir.clone());
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
    let mut pkg_ref: PackageRef = args.package.parse()?;

    // --path flag overrides inline ::variant
    if let Some(p) = &args.path {
        pkg_ref.set_variant(p.clone());
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

    let radio = radio::capabilities::detect(&sd_root);
    let cmd = packages::install::InstallCommand::new(packages::install::InstallOptions {
        sd_root: sd_root.clone(),
        pkg_ref: pkg_ref.clone(),
        dev: args.dev,
        radio,
    })?;

    if !pkg_ref.is_local() {
        println!(
            "  {} Fetched {}",
            console::style("✓").green(),
            cmd.package.display_name()
        );
    }

    // Header
    println!();
    println!("  {}", console::style(cmd.package.display_name()).bold());
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

    let result = cmd.execute(args.dry_run, |dest| {
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

    let (query, version_override) = match &args.package {
        Some(raw) => {
            let pkg_ref: PackageRef = raw.parse()?;
            (pkg_ref.canonical(), pkg_ref.version().to_string())
        }
        None => (String::new(), String::new()),
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

    let mut store = packages::store::PackageStore::load(sd_root.clone())?;
    let targets = store.update_targets(&query, args.all)?;

    println!();
    for target in &targets {
        let include_dev = if args.package.is_some() {
            args.dev
        } else {
            target.dev
        };

        let cmd = packages::update::UpdateCommand::new(
            packages::update::UpdateOptions {
                pkg: target,
                version_override: &version_override,
                include_dev,
            },
            store,
        )?;

        let total_files = cmd.total_files();
        let bar = ProgressBar::new(total_files as u64);
        bar.set_style(
            ProgressStyle::with_template("{spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} {msg}")
                .unwrap(),
        );
        bar.set_message("Updating");

        let result = cmd.execute(args.dry_run, |dest| {
            if let Some(name) = Path::new(dest).file_name() {
                bar.set_message(name.to_string_lossy().to_string());
            }
            bar.inc(1);
        })?;
        bar.finish_and_clear();
        store = result.store;

        if result.up_to_date {
            println!(
                "  {} {} is already up to date",
                console::style("ℹ").blue(),
                result.package.display_name()
            );
            continue;
        }

        let info = format!("{} -> {}", result.package.id, result.package.channel_info());

        if result.files_copied > 0 {
            println!(
                "  {} Updated {}: {} file(s) copied",
                console::style("✓").green(),
                info,
                result.files_copied
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
        let pkg_ref: PackageRef = args.package.parse()?;
        pkg_ref.canonical()
    };

    let cmd = packages::remove::RemoveCommand::new(packages::remove::RemoveOptions {
        sd_root: sd_root.clone(),
        query,
    })?;

    println!();
    println!("  {}", console::style(cmd.package.display_name()).bold());
    println!();

    if args.dry_run {
        println!(
            "  {} Would remove the following files:",
            console::style("⚠").yellow()
        );
        for f in &cmd.files {
            println!("    {f}");
        }
        if !cmd.luac_files.is_empty() {
            println!(
                "  {} Would also remove {} compiled .luac file(s):",
                console::style("⚠").yellow(),
                cmd.luac_files.len()
            );
            for f in &cmd.luac_files {
                println!("    {f}");
            }
        }
        if !cmd.dirs.is_empty() {
            println!(
                "  {} Would remove directories (if empty):",
                console::style("⚠").yellow(),
            );
            for d in &cmd.dirs {
                println!("    {d}");
            }
        }
        cmd.execute(true, |_| {})?;
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
            "  {} Removed {} - {} file(s)",
            console::style("✓").green(),
            result.package.id,
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

    let store = packages::store::PackageStore::load(sd_root)?;

    if store.packages().is_empty() {
        println!("  {} No packages installed", console::style("ℹ").blue());
        return Ok(());
    }

    println!();
    println!(
        "  {}",
        console::style(format!("Installed Packages ({})", store.packages().len())).bold()
    );
    println!();
    println!(
        "  {:<50} {:<18} {:<8} {:<10} Commit",
        "ID", "Name", "Channel", "Version"
    );
    println!("  {}", "-".repeat(100));

    for pkg in store.packages() {
        let mut id_display = pkg.id.clone();
        if let Some(ref v) = pkg.variant {
            // Strip .yml/.yaml extension for cleaner annotation
            let v_short = v
                .strip_suffix(".yml")
                .or_else(|| v.strip_suffix(".yaml"))
                .unwrap_or(v);
            id_display.push_str(&format!(" ({v_short})"));
        }
        if let Some(ref o) = pkg.origin {
            id_display.push_str(&format!(" [fork: {o}]"));
        }
        println!(
            "  {:<50} {:<18} {:<8} {:<10} {}",
            id_display,
            pkg.name,
            pkg.channel,
            pkg.version,
            pkg.short_commit()
        );
    }

    Ok(())
}

fn print_channel_info(pkg: &packages::store::InstalledPackage) {
    println!(
        "  {} Channel: {}",
        console::style("ℹ").blue(),
        pkg.channel_info()
    );
}

fn run_info(args: InfoArgs) -> Result<()> {
    let mut pkg_ref: PackageRef = args.package.parse()?;
    if let Some(path) = args.path {
        pkg_ref.set_variant(path);
    }

    let spinner = ProgressBar::new_spinner();
    spinner.set_message("Fetching package info...");
    spinner.enable_steady_tick(Duration::from_millis(80));

    let result = packages::info::fetch_info(&pkg_ref)?;
    spinner.finish_and_clear();

    let m = &result.manifest;
    let pkg = &m.package;

    let display_name = pkg.display_name();
    let keywords: String = pkg
        .keywords
        .iter()
        .map(|k| format!("#{k}"))
        .collect::<Vec<_>>()
        .join(" ");
    if keywords.is_empty() {
        println!("{}", console::style(display_name).bold());
    } else {
        println!("{} {}", console::style(display_name).bold(), keywords);
    }
    println!("{:<14}{}", "id:", pkg.id);

    if !pkg.description.is_empty() {
        println!("{}", pkg.description);
    }
    if !result.version.is_empty() {
        println!("{:<14}{}", "version:", result.version);
    }
    if !pkg.license.is_empty() {
        println!("{:<14}{}", "license:", pkg.license);
    }
    if !pkg.min_edgetx_version.is_empty() {
        println!("{:<14}{}", "min-edgetx:", pkg.min_edgetx_version);
    }
    if let Some(ref caps) = pkg.capabilities
        && let Some(ref display) = caps.display
    {
        let d = display.to_string();
        if !d.is_empty() {
            println!("{:<14}{}", "display:", d);
        }
    }
    for url_entry in &pkg.urls {
        println!("{:<14}{}", format!("{}:", url_entry.name), url_entry.url);
    }
    if let Some(ref url) = result.repository_url {
        println!("{:<14}{}", "repository:", url);
    }
    if !pkg.authors.is_empty() {
        let authors_str: String = pkg
            .authors
            .iter()
            .map(|a| match &a.email {
                Some(email) => format!("{} <{}>", a.name, email),
                None => a.name.clone(),
            })
            .collect::<Vec<_>>()
            .join(", ");
        println!("{:<14}{}", "authors:", authors_str);
    }

    let content_groups: &[(&str, &[_])] = &[
        ("libraries", &m.libraries),
        ("tools", &m.tools),
        ("widgets", &m.widgets),
        ("telemetry", &m.telemetry),
        ("functions", &m.functions),
        ("mixes", &m.mixes),
        ("sounds", &m.sounds),
        ("images", &m.images),
        ("themes", &m.themes),
        ("files", &m.files),
    ];
    let sections: Vec<_> = content_groups
        .iter()
        .filter(|(_, items)| !items.is_empty())
        .map(|(label, items)| {
            let names: Vec<_> = items.iter().map(|i| i.name.as_str()).collect();
            format!("{label}: {}", names.join(", "))
        })
        .collect();
    if !sections.is_empty() {
        println!("contents:");
        for s in &sections {
            println!("  {s}");
        }
    }

    if !pkg.variants.is_empty() {
        println!("variants:");
        for v in &pkg.variants {
            let caps_str = v
                .capabilities
                .display
                .as_ref()
                .map(|d| d.to_string())
                .unwrap_or_default();
            println!("  {} ({})", v.path, caps_str);
        }
    }

    Ok(())
}

fn run_outdated(args: OutdatedArgs) -> Result<()> {
    let sd_root = resolve_sd_root(&args.dir)?;
    print_sd_card_info(&sd_root);

    let spinner = ProgressBar::new_spinner();
    spinner.set_message("Checking for updates...");
    spinner.enable_steady_tick(Duration::from_millis(80));

    let outdated =
        packages::outdated::check_outdated(packages::outdated::OutdatedOptions { sd_root })?;
    spinner.finish_and_clear();

    if outdated.is_empty() {
        println!(
            "  {} All packages are up to date",
            console::style("✓").green()
        );
        return Ok(());
    }

    println!();
    println!(
        "  {}",
        console::style(format!("Updates Available ({})", outdated.len())).bold()
    );
    println!();
    println!("  {:<50} {:<15} {:<15} Channel", "ID", "Current", "Latest");
    println!("  {}", "-".repeat(95));

    for pkg in &outdated {
        println!(
            "  {:<50} {:<15} {:<15} {}",
            pkg.id,
            pkg.current_version,
            console::style(&pkg.latest_version).green(),
            pkg.channel
        );
    }

    std::process::exit(1);
}
