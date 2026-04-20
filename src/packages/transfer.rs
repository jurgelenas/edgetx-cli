use std::path::Path;

use crate::manifest::{ContentItem, Manifest};
use crate::packages::path::PackagePath;
use crate::radio;

use super::PackageError;

/// Copy all content items from a manifest to the SD card root.
///
/// Returns the number of files copied and the list of destination paths
/// (including trailing-slash directory entries for cleanup tracking).
pub(crate) fn copy_content_items(
    manifest: &Manifest,
    manifest_dir: &Path,
    sd_root: &Path,
    include_dev: bool,
    on_file: &mut impl FnMut(&str),
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

/// Build exclude patterns for a content item.
fn build_exclude(binary: bool, item: &ContentItem) -> Vec<String> {
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
