use crate::manifest;
use crate::scaffold;
use crate::simulator;
use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand, builder::PossibleValue};
use indicatif::{ProgressBar, ProgressStyle};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

// Enables clap to parse CLI strings into ScriptType variants,
// list valid values in --help, and generate shell completions.
impl clap::ValueEnum for scaffold::ScriptType {
    fn value_variants<'a>() -> &'a [Self] {
        &[
            Self::Tool,
            Self::Telemetry,
            Self::Function,
            Self::Mix,
            Self::Widget,
            Self::Library,
        ]
    }

    fn to_possible_value(&self) -> Option<PossibleValue> {
        Some(PossibleValue::new(match self {
            Self::Tool => "tool",
            Self::Telemetry => "telemetry",
            Self::Function => "function",
            Self::Mix => "mix",
            Self::Widget => "widget",
            Self::Library => "library",
        }))
    }
}

#[derive(Subcommand)]
pub enum DevCommands {
    /// Initialize a new edgetx.yml manifest
    Init(InitArgs),
    /// Generate boilerplate for a new EdgeTX Lua script
    Scaffold(ScaffoldArgs),
    /// Watch source files and sync changes to a target directory
    Sync(SyncArgs),
    /// Run the EdgeTX WASM simulator
    Simulator(SimulatorArgs),
}

#[derive(Args)]
pub struct InitArgs {
    /// Package name (defaults to directory name)
    name: Option<String>,

    /// Directory to create edgetx.yml in
    #[arg(long, default_value = ".")]
    src_dir: PathBuf,
}

#[derive(Args)]
pub struct ScaffoldArgs {
    /// Script type
    #[arg(name = "type", value_enum)]
    script_type: scaffold::ScriptType,

    /// Script name
    name: String,

    /// Source directory containing edgetx.yml
    #[arg(long, default_value = ".")]
    src_dir: PathBuf,

    /// Comma-separated library dependencies
    #[arg(long)]
    depends: Option<String>,

    /// Mark as a development dependency
    #[arg(long)]
    dev: bool,
}

#[derive(Args)]
pub struct SyncArgs {
    /// Target directory to sync to
    target_dir: PathBuf,

    /// Source directory containing edgetx.yml
    #[arg(long, default_value = ".")]
    src_dir: PathBuf,

    /// Exclude development dependencies from sync
    #[arg(long)]
    no_dev: bool,
}

#[derive(Args)]
pub struct SimulatorArgs {
    /// Radio model (e.g., tx16s). Interactive picker if omitted
    #[arg(long)]
    radio: Option<String>,

    /// Custom SD card directory
    #[arg(long)]
    sdcard: Option<PathBuf>,

    /// Disable auto-sync when package detected
    #[arg(long)]
    no_watch: bool,

    /// Reset simulator SD card to defaults before starting
    #[arg(long)]
    reset: bool,

    /// Run without GUI window (for testing/CI)
    #[arg(long)]
    headless: bool,

    /// Auto-exit after duration (e.g., 5s, 30s)
    #[arg(long)]
    timeout: Option<String>,

    /// Save LCD framebuffer as PNG at exit
    #[arg(long)]
    screenshot: Option<PathBuf>,

    /// Execute a Lua test script (use "-" for stdin)
    #[arg(long)]
    script: Option<PathBuf>,

    /// Read Lua commands from stdin
    #[arg(long)]
    script_stdin: bool,

    /// List available radio models
    #[command(subcommand)]
    subcommand: Option<SimulatorSubcommands>,
}

#[derive(Subcommand)]
pub enum SimulatorSubcommands {
    /// List available radio models for the simulator
    List,
}

pub fn dispatch(command: DevCommands) -> Result<()> {
    match command {
        DevCommands::Init(args) => run_init(args),
        DevCommands::Scaffold(args) => run_scaffold(args),
        DevCommands::Sync(args) => run_sync(args),
        DevCommands::Simulator(args) => run_simulator(args),
    }
}

fn run_init(args: InitArgs) -> Result<()> {
    let dir = std::fs::canonicalize(&args.src_dir)
        .with_context(|| format!("resolving directory {}", args.src_dir.display()))?;

    let yml_path = dir.join(manifest::FILE_NAME);
    if yml_path.exists() {
        bail!(
            "{} already exists in {}",
            manifest::FILE_NAME,
            dir.display()
        );
    }

    let name = args.name.unwrap_or_else(|| {
        dir.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "my-package".to_string())
    });

    let content = format!("package:\n  name: {name}\n  description: \"\"\n  license: \"\"\n");

    std::fs::write(&yml_path, content).context("writing manifest")?;

    println!(
        "  {} Created {}",
        console::style("✓").green(),
        yml_path.display()
    );
    Ok(())
}

fn run_scaffold(args: ScaffoldArgs) -> Result<()> {
    let src_dir = std::fs::canonicalize(&args.src_dir)
        .with_context(|| format!("resolving source directory {}", args.src_dir.display()))?;

    let depends: Vec<String> = args
        .depends
        .map(|d| d.split(',').map(|s| s.trim().to_string()).collect())
        .unwrap_or_default();

    let result = scaffold::run(scaffold::Options {
        script_type: args.script_type,
        name: args.name.clone(),
        depends,
        src_dir: src_dir.clone(),
        dev: args.dev,
    })?;

    for f in &result.files {
        println!("  {} Created {}", console::style("✓").green(), f.display());
    }

    let yaml_key = args.script_type.yaml_key();

    println!(
        "  {} Added {} entry for {:?} to edgetx.yml",
        console::style("ℹ").blue(),
        yaml_key,
        args.name
    );

    Ok(())
}

fn run_sync(args: SyncArgs) -> Result<()> {
    let src_dir = std::fs::canonicalize(&args.src_dir)
        .with_context(|| format!("resolving source directory {}", args.src_dir.display()))?;
    let target_dir = std::fs::canonicalize(&args.target_dir)
        .with_context(|| format!("resolving target directory {}", args.target_dir.display()))?;

    if !target_dir.is_dir() {
        bail!("target {:?} is not a directory", target_dir);
    }

    log::debug!("loading manifest from {}", src_dir.display());
    let m = manifest::load(&src_dir)?;

    let source_roots = m.source_roots(&src_dir);
    println!();
    println!("  {}", console::style(&m.package.name).bold());
    if !m.package.description.is_empty() {
        println!("  {}", m.package.description);
    }
    println!();
    for root in &source_roots {
        println!(
            "  {} Source: {}",
            console::style("ℹ").blue(),
            root.display()
        );
    }
    println!(
        "  {} Target: {}",
        console::style("ℹ").blue(),
        target_dir.display()
    );
    println!();

    let include_dev = !args.no_dev;
    let items = m.content_items(include_dev);

    // Initial sync
    let bar = ProgressBar::new(0);
    bar.set_style(
        ProgressStyle::with_template("{spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} {msg}")
            .unwrap(),
    );
    bar.set_message("Initial sync");

    let copied = crate::sync::initial_sync(crate::sync::SyncOptions {
        manifest: &m,
        manifest_dir: &src_dir,
        target_dir: &target_dir,
        items: &items,
        on_initial_copy_start: Some(&|total| {
            bar.set_length(total as u64);
        }),
        on_file_copied: Some(&|rel_path| {
            if let Some(name) = Path::new(rel_path).file_name() {
                bar.set_message(name.to_string_lossy().to_string());
            }
            bar.inc(1);
        }),
    })?;
    bar.finish_and_clear();

    println!(
        "  {} Initial sync: {} file(s) copied to {}",
        console::style("✓").green(),
        copied,
        target_dir.display()
    );
    println!();

    // Watch phase
    let sync_count = Arc::new(AtomicU32::new(0));
    let count_clone = sync_count.clone();

    println!(
        "  {} Watching for changes... (Ctrl+C to stop)",
        console::style("⏳").yellow()
    );

    crate::sync::watch(crate::sync::WatchOptions {
        manifest: &m,
        manifest_dir: &src_dir,
        target_dir: &target_dir,
        items: &items,
        on_sync_event: Some(&move |event| {
            let n = count_clone.fetch_add(1, Ordering::Relaxed) + 1;
            println!("  [{n}] {}: {}", event.op, event.rel_path);
        }),
        on_error: Some(&|err| {
            log::warn!("sync error: {err}");
        }),
    })?;

    let total = sync_count.load(Ordering::Relaxed);
    println!("  {} Sync stopped", console::style("✓").green());
    if total > 0 {
        println!(
            "  {} {total} file(s) synced during session",
            console::style("✓").green()
        );
    }

    Ok(())
}

fn run_simulator(args: SimulatorArgs) -> Result<()> {
    // Handle subcommand (list)
    if let Some(SimulatorSubcommands::List) = args.subcommand {
        return run_simulator_list();
    }

    // Fetch radio catalog
    println!(
        "  {} Fetching radio catalog...",
        console::style("⏳").yellow()
    );

    let catalog = crate::radio_catalog::fetch_catalog()?;
    println!(
        "  {} Loaded {} radios",
        console::style("✓").green(),
        catalog.len()
    );

    // Select radio
    let radio = if let Some(ref query) = args.radio {
        crate::radio_catalog::find_radio(&catalog, query)?.clone()
    } else {
        // Interactive picker
        let names: Vec<String> = catalog
            .iter()
            .map(|r| {
                format!(
                    "{} ({}x{}, {}-bit)",
                    r.name, r.display.w, r.display.h, r.display.depth
                )
            })
            .collect();

        let selection = dialoguer::Select::new()
            .with_prompt("Select a radio")
            .items(&names)
            .default(0)
            .interact()?;

        catalog[selection].clone()
    };

    println!(
        "  {} Radio: {} ({}x{}, {}-bit depth)",
        console::style("ℹ").blue(),
        radio.name,
        radio.display.w,
        radio.display.h,
        radio.display.depth
    );

    // Download WASM binary
    println!(
        "  {} Downloading {} firmware...",
        console::style("⏳").yellow(),
        radio.name
    );

    let wasm_path = crate::radio_catalog::ensure_wasm(&radio, |downloaded, total| {
        if total > 0 {
            let pct = downloaded as f64 / total as f64 * 100.0;
            eprint!("\r  Downloading firmware... {pct:.0}%");
        }
    })?;
    eprintln!();
    println!("  {} Firmware ready", console::style("✓").green());

    // Resolve SD card directory
    let radio_key = radio.key();
    let sdcard_dir = match args.sdcard {
        Some(dir) => dir,
        None => simulator::sdcard::sd_card_path(&radio_key)?,
    };
    let settings_dir = simulator::sdcard::settings_path(&radio_key)?;

    // Reset if requested
    if args.reset {
        log::info!("resetting simulator SD card...");
        simulator::sdcard::reset(&sdcard_dir, &settings_dir)?;
    }

    // Ensure directory structure
    simulator::sdcard::ensure_structure(&sdcard_dir, &settings_dir)?;

    println!(
        "  {} SD card: {}",
        console::style("ℹ").blue(),
        sdcard_dir.display()
    );

    // Check for package in CWD
    let cwd = std::env::current_dir()?;
    let mut watch_dir = None;

    if let Ok(m) = manifest::load(&cwd) {
        println!(
            "  {} Package detected: {}",
            console::style("ℹ").blue(),
            m.package.name
        );

        println!(
            "  {} Installing package into simulator...",
            console::style("⏳").yellow()
        );

        simulator::sdcard::install_package(&sdcard_dir, &m, &cwd)?;
        println!("  {} Package installed", console::style("✓").green());

        if !args.no_watch {
            watch_dir = Some(cwd.clone());
        }
    }

    // Parse timeout
    let timeout = args
        .timeout
        .as_ref()
        .map(|t| parse_duration(t))
        .transpose()?;

    // Resolve script path and stdin mode
    let dash = PathBuf::from("-");
    let stdin_script = args.script_stdin || args.script.as_deref() == Some(dash.as_path());

    if args.script_stdin && args.script.is_some() && args.script.as_deref() != Some(dash.as_path())
    {
        bail!("cannot use both --script <file> and --script-stdin");
    }

    let script_path = if stdin_script {
        None
    } else {
        args.script
            .map(|s| {
                std::fs::canonicalize(&s)
                    .with_context(|| format!("resolving script path {}", s.display()))
            })
            .transpose()?
    };

    // Print keyboard shortcuts
    if !args.headless {
        println!();
        println!(
            "  {} {}",
            console::style("ℹ").blue(),
            crate::simulator_ui::input::print_keyboard_shortcuts()
        );
    }

    println!();
    println!(
        "  {} Starting simulator... (Ctrl+C to stop)",
        console::style("ℹ").blue()
    );

    // Read WASM bytes
    let wasm_bytes = std::fs::read(&wasm_path)
        .with_context(|| format!("reading WASM binary {:?}", wasm_path))?;

    let opts = simulator::SimulatorOptions {
        radio,
        sdcard_dir,
        settings_dir,
        watch_dir,
        timeout,
        screenshot_path: args.screenshot,
        script_path,
        stdin_script,
    };

    if args.headless {
        simulator::run(opts, &wasm_bytes)
    } else {
        crate::simulator_ui::run(opts, &wasm_bytes)
    }
}

fn parse_duration(s: &str) -> Result<std::time::Duration> {
    use std::time::Duration;
    if let Some(rest) = s.strip_suffix("ms") {
        let ms: u64 = rest.parse()?;
        return Ok(Duration::from_millis(ms));
    }
    if let Some(rest) = s.strip_suffix('s') {
        let secs: f64 = rest.parse()?;
        return Ok(Duration::from_secs_f64(secs));
    }
    if let Some(rest) = s.strip_suffix('m') {
        let mins: f64 = rest.parse()?;
        return Ok(Duration::from_secs_f64(mins * 60.0));
    }
    anyhow::bail!("invalid duration {:?}", s);
}

fn run_simulator_list() -> Result<()> {
    println!(
        "  {} Fetching radio catalog...",
        console::style("⏳").yellow()
    );

    let catalog = crate::radio_catalog::fetch_catalog()?;
    println!(
        "  {} Loaded {} radios",
        console::style("✓").green(),
        catalog.len()
    );

    println!();
    println!(
        "  {}",
        console::style(format!("Available Radios ({})", catalog.len())).bold()
    );
    println!();
    println!("  {:<20} {:<12} {:<8} WASM", "Name", "Display", "Depth");
    println!("  {}", "-".repeat(70));

    for r in &catalog {
        println!(
            "  {:<20} {}x{:<8} {}-bit    {}",
            r.name, r.display.w, r.display.h, r.display.depth, r.wasm
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_parse_duration_seconds() {
        assert_eq!(parse_duration("5s").unwrap(), Duration::from_secs(5));
        assert_eq!(
            parse_duration("1.5s").unwrap(),
            Duration::from_secs_f64(1.5)
        );
    }

    #[test]
    fn test_parse_duration_millis() {
        assert_eq!(parse_duration("100ms").unwrap(), Duration::from_millis(100));
        assert_eq!(parse_duration("500ms").unwrap(), Duration::from_millis(500));
    }

    #[test]
    fn test_parse_duration_minutes() {
        assert_eq!(parse_duration("1m").unwrap(), Duration::from_secs(60));
        assert_eq!(parse_duration("0.5m").unwrap(), Duration::from_secs(30));
    }

    #[test]
    fn test_parse_duration_invalid() {
        assert!(parse_duration("5").is_err());
        assert!(parse_duration("abc").is_err());
    }
}
