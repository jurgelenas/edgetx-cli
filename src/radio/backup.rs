use anyhow::{Context, Result};
use std::io::{Read, Write};
use std::path::Path;
use walkdir::WalkDir;

/// BackupDir recursively copies all files from src_dir to dest_dir.
/// Returns the total number of files copied.
pub fn backup_dir(src_dir: &Path, dest_dir: &Path, on_file: impl Fn(&str)) -> Result<usize> {
    let mut copied = 0;

    for entry in WalkDir::new(src_dir).into_iter().flatten() {
        if !entry.file_type().is_file() {
            continue;
        }

        let rel = entry
            .path()
            .strip_prefix(src_dir)
            .context("computing relative path")?;
        let dest = dest_dir.join(rel);

        on_file(&dest.display().to_string());

        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }

        std::fs::copy(entry.path(), &dest).with_context(|| format!("copying {}", rel.display()))?;
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
pub fn compress_dir(src_dir: &Path, zip_path: &Path, on_file: impl Fn(&str)) -> Result<()> {
    let file = std::fs::File::create(zip_path).context("creating zip file")?;
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
            .context("computing relative path")?;
        let rel_str = rel.to_string_lossy().replace('\\', "/");

        on_file(&rel_str);

        writer
            .start_file(&rel_str, options)
            .context("starting zip entry")?;

        let mut f = std::fs::File::open(entry.path())?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf)?;
        writer.write_all(&buf)?;
    }

    writer.finish().context("finishing zip")?;

    std::fs::remove_dir_all(src_dir).context("removing source after compression")?;

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
