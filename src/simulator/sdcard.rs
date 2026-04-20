use std::path::{Path, PathBuf};

use super::SimulatorError;
use crate::manifest::Manifest;
use crate::radio;

const SDCARD_DIRS: &[&str] = &[
    "RADIO",
    "MODELS",
    "SCRIPTS/TOOLS",
    "SCRIPTS/TELEMETRY",
    "SCRIPTS/FUNCTIONS",
    "SCRIPTS/MIXES",
    "SCRIPTS/WIDGETS",
    "SOUNDS",
    "IMAGES",
];

/// Returns the default SD card directory for a radio.
pub fn sd_card_path(radio_key: &str) -> Result<PathBuf, SimulatorError> {
    let cache = cache_dir()?;
    Ok(cache.join(radio_key).join("sdcard"))
}

/// Returns the default settings directory for a radio.
pub fn settings_path(radio_key: &str) -> Result<PathBuf, SimulatorError> {
    let cache = cache_dir()?;
    Ok(cache.join(radio_key).join("settings"))
}

fn cache_dir() -> Result<PathBuf, SimulatorError> {
    let base = directories::BaseDirs::new()
        .ok_or_else(|| SimulatorError::Runtime("cannot determine cache directory".into()))?;
    Ok(base.cache_dir().join("edgetx-cli").join("simulator"))
}

/// Create the standard EdgeTX SD card directory structure.
pub fn ensure_structure(sdcard_dir: &Path, settings_dir: &Path) -> Result<(), SimulatorError> {
    for dir in SDCARD_DIRS {
        std::fs::create_dir_all(sdcard_dir.join(dir)).map_err(|e| SimulatorError::Io {
            context: format!("creating {dir}"),
            source: e,
        })?;
    }
    std::fs::create_dir_all(settings_dir).map_err(|e| SimulatorError::Io {
        context: "creating settings dir".into(),
        source: e,
    })?;
    Ok(())
}

/// Remove and recreate the SD card and settings directories.
pub fn reset(sdcard_dir: &Path, settings_dir: &Path) -> Result<(), SimulatorError> {
    if sdcard_dir.exists() {
        std::fs::remove_dir_all(sdcard_dir).map_err(|e| SimulatorError::Io {
            context: "removing SD card dir".into(),
            source: e,
        })?;
    }
    if settings_dir.exists() {
        std::fs::remove_dir_all(settings_dir).map_err(|e| SimulatorError::Io {
            context: "removing settings dir".into(),
            source: e,
        })?;
    }
    ensure_structure(sdcard_dir, settings_dir)
}

/// Copy a package's content items into the simulator SD card.
pub fn install_package(
    sdcard_dir: &Path,
    m: &Manifest,
    manifest_dir: &Path,
) -> Result<(), SimulatorError> {
    let items = m.content_items(true);
    for item in &items {
        let source_root = m
            .resolve_content_path(manifest_dir, &item.path)
            .map_err(|e| SimulatorError::Runtime(format!("resolving {}: {e}", item.path)))?;

        let mut exclude: Vec<String> = radio::copy::DEFAULT_EXCLUDE
            .iter()
            .map(|s| s.to_string())
            .collect();
        exclude.extend(item.exclude.iter().cloned());

        radio::copy::copy_paths(
            &source_root,
            sdcard_dir,
            &[radio::copy::CopyPath {
                src: item.path.as_str(),
                dest: item.sd_dest().as_str(),
            }],
            &radio::copy::CopyOptions {
                dry_run: false,
                exclude: &exclude,
            },
            &mut |_| {},
        )
        .map_err(|e| SimulatorError::Runtime(format!("copying package content: {e}")))?;
    }
    Ok(())
}
