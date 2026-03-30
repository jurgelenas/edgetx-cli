use std::io::{Read, Write};
use std::path::Path;
use thiserror::Error;
use walkdir::WalkDir;

#[derive(Error, Debug)]
pub enum BackupError {
    #[error("{context}: {source}")]
    Io {
        context: String,
        source: std::io::Error,
    },
    #[error("{context}: {source}")]
    Zip {
        context: &'static str,
        source: zip::result::ZipError,
    },
    #[error("computing relative path for {0}")]
    RelativePath(std::path::PathBuf),
}

/// BackupDir recursively copies all files from src_dir to dest_dir.
/// Returns the total number of files copied.
pub fn backup_dir(
    src_dir: &Path,
    dest_dir: &Path,
    on_file: impl Fn(&str),
) -> Result<usize, BackupError> {
    let mut copied = 0;

    for entry in WalkDir::new(src_dir).into_iter().flatten() {
        if !entry.file_type().is_file() {
            continue;
        }

        let rel = entry
            .path()
            .strip_prefix(src_dir)
            .map_err(|_| BackupError::RelativePath(entry.path().to_path_buf()))?;
        let dest = dest_dir.join(rel);

        on_file(&dest.display().to_string());

        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| BackupError::Io {
                context: format!("creating directory {}", parent.display()),
                source: e,
            })?;
        }

        std::fs::copy(entry.path(), &dest).map_err(|e| BackupError::Io {
            context: format!("copying {}", rel.display()),
            source: e,
        })?;
        copied += 1;
    }

    Ok(copied)
}

/// Count all regular files under dir.
pub fn count_all_files(dir: &Path) -> usize {
    WalkDir::new(dir)
        .into_iter()
        .flatten()
        .filter(|e| e.file_type().is_file())
        .count()
}

/// Create a zip archive at zip_path from the contents of src_dir.
/// On success, src_dir is removed.
pub fn compress_dir(
    src_dir: &Path,
    zip_path: &Path,
    on_file: impl Fn(&str),
) -> Result<(), BackupError> {
    let file = std::fs::File::create(zip_path).map_err(|e| BackupError::Io {
        context: "creating zip file".into(),
        source: e,
    })?;
    let mut writer = zip::ZipWriter::new(file);

    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    for entry in WalkDir::new(src_dir).into_iter().flatten() {
        if !entry.file_type().is_file() {
            continue;
        }

        let rel = entry
            .path()
            .strip_prefix(src_dir)
            .map_err(|_| BackupError::RelativePath(entry.path().to_path_buf()))?;
        let rel_str = rel.to_string_lossy().replace('\\', "/");

        on_file(&rel_str);

        writer
            .start_file(&rel_str, options)
            .map_err(|e| BackupError::Zip {
                context: "starting zip entry",
                source: e,
            })?;

        let mut f = std::fs::File::open(entry.path()).map_err(|e| BackupError::Io {
            context: format!("opening {}", entry.path().display()),
            source: e,
        })?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf).map_err(|e| BackupError::Io {
            context: format!("reading {}", entry.path().display()),
            source: e,
        })?;
        writer.write_all(&buf).map_err(|e| BackupError::Io {
            context: format!("writing zip entry {}", rel_str),
            source: e,
        })?;
    }

    writer.finish().map_err(|e| BackupError::Zip {
        context: "finishing zip",
        source: e,
    })?;

    std::fs::remove_dir_all(src_dir).map_err(|e| BackupError::Io {
        context: "removing source after compression".into(),
        source: e,
    })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_backup_dir() {
        let src = TempDir::new().unwrap();
        let dest = TempDir::new().unwrap();

        std::fs::create_dir_all(src.path().join("sub")).unwrap();
        std::fs::write(src.path().join("file1.txt"), "hello").unwrap();
        std::fs::write(src.path().join("sub/file2.txt"), "world").unwrap();

        let copied = backup_dir(src.path(), dest.path(), |_| {}).unwrap();
        assert_eq!(copied, 2);
        assert!(dest.path().join("file1.txt").exists());
        assert!(dest.path().join("sub/file2.txt").exists());
    }

    #[test]
    fn test_count_all_files() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("a.txt"), "a").unwrap();
        std::fs::write(dir.path().join("sub/b.txt"), "b").unwrap();

        assert_eq!(count_all_files(dir.path()), 2);
    }

    #[test]
    fn test_compress_dir() {
        let src = TempDir::new().unwrap();
        let out = TempDir::new().unwrap();

        let src_path = src.path().join("backup");
        std::fs::create_dir_all(src_path.join("sub")).unwrap();
        std::fs::write(src_path.join("file.txt"), "data").unwrap();
        std::fs::write(src_path.join("sub/other.txt"), "data2").unwrap();

        let zip_path = out.path().join("backup.zip");
        compress_dir(&src_path, &zip_path, |_| {}).unwrap();

        assert!(zip_path.exists());
        assert!(!src_path.exists()); // Source should be removed
    }
}
