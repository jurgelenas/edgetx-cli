use std::path::{Path, PathBuf};
use thiserror::Error;
use walkdir::WalkDir;

/// Default glob patterns excluded from copy (e.g. compiled Lua bytecode).
pub const DEFAULT_EXCLUDE: &[&str] = &["*.luac"];

#[derive(Error, Debug)]
pub enum CopyError {
    #[error("{context}: {source}")]
    Io {
        context: String,
        source: std::io::Error,
    },
    #[error("computing relative path for {0}")]
    RelativePath(PathBuf),
}

/// CopyOptions configures a copy operation.
pub struct CopyOptions<'a> {
    pub dry_run: bool,
    pub exclude: &'a [String],
}

/// Copy each relative path from src_dir to dest_dir. Directories are
/// copied recursively. Returns the number of files copied.
pub fn copy_paths(
    src_dir: &Path,
    dest_dir: &Path,
    paths: &[&str],
    opts: &CopyOptions,
    on_file: &mut dyn FnMut(&Path),
) -> Result<usize, CopyError> {
    let mut copied = 0;

    for rel_path in paths {
        let src = src_dir.join(rel_path);

        let meta = match std::fs::metadata(&src) {
            Ok(m) => m,
            Err(_) => {
                log::warn!("source does not exist, skipping: {}", rel_path);
                continue;
            }
        };

        if meta.is_dir() {
            let n = copy_dir(&src, src_dir, dest_dir, opts, on_file)?;
            copied += n;
        } else {
            if is_excluded(&src, opts.exclude) {
                log::debug!("excluded: {}", rel_path);
                continue;
            }
            let dest = dest_dir.join(rel_path);
            copy_single_file(&src, &dest, opts, on_file)?;
            if !opts.dry_run {
                copied += 1;
            }
        }
    }

    Ok(copied)
}

/// Count files under the given relative paths, excluding matching patterns.
pub fn count_files(src_dir: &Path, paths: &[&str], exclude: &[String]) -> usize {
    let mut count = 0;
    for rel_path in paths {
        let src = src_dir.join(rel_path);
        let meta = match std::fs::metadata(&src) {
            Ok(m) => m,
            Err(_) => continue,
        };

        if !meta.is_dir() {
            if !is_excluded(&src, exclude) {
                count += 1;
            }
            continue;
        }

        for entry in WalkDir::new(&src).into_iter().flatten() {
            if entry.file_type().is_file() && !is_excluded(entry.path(), exclude) {
                count += 1;
            }
        }
    }
    count
}

fn copy_dir(
    src_root: &Path,
    src_base: &Path,
    dest_base: &Path,
    opts: &CopyOptions,
    on_file: &mut dyn FnMut(&Path),
) -> Result<usize, CopyError> {
    let mut copied = 0;

    for entry in WalkDir::new(src_root).into_iter().flatten() {
        if !entry.file_type().is_file() {
            continue;
        }

        if is_excluded(entry.path(), opts.exclude) {
            log::debug!("excluded: {}", entry.path().display());
            continue;
        }

        let rel = entry
            .path()
            .strip_prefix(src_base)
            .map_err(|_| CopyError::RelativePath(entry.path().to_path_buf()))?;
        let dest = dest_base.join(rel);

        copy_single_file(entry.path(), &dest, opts, on_file)?;
        if !opts.dry_run {
            copied += 1;
        }
    }

    Ok(copied)
}

fn copy_single_file(
    src: &Path,
    dest: &Path,
    opts: &CopyOptions,
    on_file: &mut dyn FnMut(&Path),
) -> Result<(), CopyError> {
    on_file(dest);

    if opts.dry_run {
        return Ok(());
    }

    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| CopyError::Io {
            context: format!("creating parent directory {}", parent.display()),
            source: e,
        })?;
    }

    std::fs::copy(src, dest).map_err(|e| CopyError::Io {
        context: format!("copying {} to {}", src.display(), dest.display()),
        source: e,
    })?;

    Ok(())
}

/// Check if a path's filename matches any exclude pattern.
pub fn is_excluded(path: &Path, patterns: &[impl AsRef<str>]) -> bool {
    let filename = match path.file_name().and_then(|n| n.to_str()) {
        Some(name) => name,
        None => return false,
    };

    for pattern in patterns {
        if glob_match(pattern.as_ref(), filename) {
            return true;
        }
    }
    false
}

/// Simple glob match supporting only * wildcard (matches filepath.Match behavior).
fn glob_match(pattern: &str, name: &str) -> bool {
    if let Ok(glob) = globset::Glob::new(pattern) {
        let matcher = glob.compile_matcher();
        return matcher.is_match(name);
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_is_excluded() {
        assert!(is_excluded(Path::new("test.luac"), &["*.luac".to_string()]));
        assert!(!is_excluded(Path::new("test.lua"), &["*.luac".to_string()]));
    }

    #[test]
    fn test_copy_paths() {
        let src = TempDir::new().unwrap();
        let dest = TempDir::new().unwrap();

        // Create source structure
        std::fs::create_dir_all(src.path().join("SCRIPTS/TOOLS/MyTool")).unwrap();
        std::fs::write(src.path().join("SCRIPTS/TOOLS/MyTool/main.lua"), "-- lua").unwrap();
        std::fs::write(
            src.path().join("SCRIPTS/TOOLS/MyTool/compiled.luac"),
            "bytecode",
        )
        .unwrap();

        let exclude = vec!["*.luac".to_string()];
        let n = copy_paths(
            src.path(),
            dest.path(),
            &["SCRIPTS/TOOLS/MyTool"],
            &CopyOptions {
                dry_run: false,
                exclude: &exclude,
            },
            &mut |_| {},
        )
        .unwrap();

        assert_eq!(n, 1);
        assert!(dest.path().join("SCRIPTS/TOOLS/MyTool/main.lua").exists());
        assert!(
            !dest
                .path()
                .join("SCRIPTS/TOOLS/MyTool/compiled.luac")
                .exists()
        );
    }

    #[test]
    fn test_count_files() {
        let src = TempDir::new().unwrap();

        std::fs::create_dir_all(src.path().join("SCRIPTS/TOOLS/MyTool")).unwrap();
        std::fs::write(src.path().join("SCRIPTS/TOOLS/MyTool/main.lua"), "-- lua").unwrap();
        std::fs::write(src.path().join("SCRIPTS/TOOLS/MyTool/helper.lua"), "-- lua").unwrap();
        std::fs::write(
            src.path().join("SCRIPTS/TOOLS/MyTool/compiled.luac"),
            "bytecode",
        )
        .unwrap();

        let exclude = vec!["*.luac".to_string()];
        let count = count_files(src.path(), &["SCRIPTS/TOOLS/MyTool"], &exclude);
        assert_eq!(count, 2);
    }
}
