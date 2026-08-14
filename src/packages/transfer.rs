use std::path::Path;

use crate::luac::LuaCompiler;
use crate::manifest::{ContentItem, Manifest};
use crate::packages::path::PackagePath;
use crate::radio;

use super::PackageError;
use super::build::{BuildLayout, BuildOptions, build_package, copy_mtime};

/// Copy a package to the SD card, optionally pre-compiling its Lua scripts.
///
/// With a compiler the package is first built into a local staging directory --
/// compiling against an SD card would be far too slow -- and the finished tree,
/// sources and bytecode alike, is copied across in a single pass.
pub(crate) fn stage_and_copy(
    manifest: &Manifest,
    manifest_dir: &Path,
    sd_root: &Path,
    include_dev: bool,
    compiler: Option<&mut dyn LuaCompiler>,
    on_file: &mut dyn FnMut(&str),
) -> Result<(usize, Vec<PackagePath>), PackageError> {
    let Some(compiler) = compiler else {
        return copy_content_items(manifest, manifest_dir, sd_root, include_dev, on_file);
    };

    let staging = tempfile::TempDir::new().map_err(|e| PackageError::Io {
        context: "creating a staging directory".into(),
        source: e,
    })?;

    let built = build_package(
        &BuildOptions {
            manifest,
            manifest_dir,
            out_dir: staging.path(),
            include_dev,
            layout: BuildLayout::Package,
        },
        compiler,
        // Staging is local and fast, so only the compile step reports progress.
        &mut |_| {},
        &mut *on_file,
    )?;

    // The built manifest is marked binary, so the bytecode survives the copy.
    let staged = built
        .manifest
        .expect("the package layout always emits a manifest");

    let (copied, files) =
        copy_content_items(&staged, staging.path(), sd_root, include_dev, on_file)?;
    fix_luac_mtimes(sd_root, &files);

    Ok((copied, files))
}

/// Give each copied `.luac` the timestamp of its source on the SD card.
///
/// `std::fs::copy` does not preserve timestamps and the copy order is arbitrary,
/// so without this a `.lua` can end up looking newer than its bytecode -- which
/// is exactly when the radio throws the bytecode away and recompiles.
fn fix_luac_mtimes(sd_root: &Path, files: &[PackagePath]) {
    let all: std::collections::HashSet<&str> = files.iter().map(|f| f.as_str()).collect();

    for file in files {
        let name = file.as_str();
        if !name.ends_with(".luac") {
            continue;
        }
        // "main.luac" -> "main.lua"
        let Some(source) = name.strip_suffix('c').filter(|s| all.contains(s)) else {
            continue;
        };
        copy_mtime(&sd_root.join(source), &sd_root.join(name));
    }
}

/// Copy all content items from a manifest to the SD card root.
///
/// Returns the number of files copied and the list of destination paths
/// (including trailing-slash directory entries for cleanup tracking).
pub(crate) fn copy_content_items(
    manifest: &Manifest,
    manifest_dir: &Path,
    sd_root: &Path,
    include_dev: bool,
    on_file: &mut dyn FnMut(&str),
) -> Result<(usize, Vec<PackagePath>), PackageError> {
    let mut total_copied = 0;
    let mut copied_files = Vec::new();

    for item in manifest.content_items(include_dev) {
        let source_root = manifest
            .resolve_content_path(manifest_dir, &item.path)
            .map_err(|e| PackageError::ContentResolve {
                path: item.path.clone(),
                source: e,
            })?;

        let exclude = build_exclude(manifest.package.binary, &item);
        let opts = radio::copy::CopyOptions {
            dry_run: false,
            exclude: &exclude,
        };
        let mut on = |dest: &Path| {
            if let Ok(rel) = dest.strip_prefix(sd_root) {
                copied_files.push(PackagePath::new(rel.to_string_lossy()));
            }
            on_file(&dest.display().to_string());
        };

        let n = radio::copy::copy_paths(
            &source_root,
            sd_root,
            &[radio::copy::CopyPath {
                src: item.path.as_str(),
                dest: item.sd_dest().as_str(),
            }],
            &opts,
            &mut on,
        )?;
        total_copied += n;
    }

    Ok((total_copied, copied_files))
}

/// Count the total number of files that would be copied.
pub(crate) fn count_files(manifest_dir: &Path, m: &Manifest, include_dev: bool) -> usize {
    let mut total = 0;
    for item in m.content_items(include_dev) {
        if let Ok(source_root) = m.resolve_content_path(manifest_dir, &item.path) {
            let exclude = build_exclude(m.package.binary, &item);
            total += radio::copy::count_files(&source_root, &[item.path.as_str()], &exclude);
        }
    }
    total
}

/// Count the Lua sources a pre-compile pass would compile.
pub(crate) fn count_lua_files(manifest_dir: &Path, m: &Manifest, include_dev: bool) -> usize {
    let mut total = 0;
    for item in m.content_items(include_dev) {
        let Ok(source_root) = m.resolve_content_path(manifest_dir, &item.path) else {
            continue;
        };
        let exclude = build_exclude(m.package.binary, &item);

        // WalkDir yields a single-file root as its only entry, so this covers
        // both directory and single-file content items.
        for entry in walkdir::WalkDir::new(source_root.join(item.path.as_str()))
            .into_iter()
            .flatten()
        {
            if entry.file_type().is_file()
                && entry.path().extension().is_some_and(|e| e == "lua")
                && !radio::copy::is_excluded(entry.path(), &exclude)
            {
                total += 1;
            }
        }
    }
    total
}

/// Build exclude patterns for a content item.
pub(crate) fn build_exclude(binary: bool, item: &ContentItem) -> Vec<String> {
    if binary {
        item.exclude.clone()
    } else {
        let mut excludes = radio::copy::DEFAULT_EXCLUDE
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>();
        excludes.extend(item.exclude.iter().cloned());
        excludes
    }
}
