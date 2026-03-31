use std::path::{Path, PathBuf};

use super::path::PackagePath;
use super::store::StoreError;

/// Per-package list of files installed on the SD card.
/// Used to track what was copied so it can be cleanly removed.
pub struct PackageFileList {
    name: String,
    files: Vec<PackagePath>,
}

impl PackageFileList {
    pub fn new(name: String, files: Vec<PackagePath>) -> Self {
        Self { name, files }
    }

    /// Load the file list from a directory. Returns empty list if the file doesn't exist.
    pub fn load(dir: &Path, name: &str) -> Self {
        let path = list_path(dir, name);

        let file = match std::fs::File::open(&path) {
            Ok(f) => f,
            Err(_) => {
                return Self {
                    name: name.to_string(),
                    files: Vec::new(),
                };
            }
        };

        let mut rdr = csv::ReaderBuilder::new()
            .has_headers(false)
            .from_reader(file);

        let mut files = Vec::new();
        for record in rdr.records().flatten() {
            if let Some(f) = record.get(0)
                && !f.is_empty()
            {
                files.push(PackagePath::from(f));
            }
        }

        Self {
            name: name.to_string(),
            files,
        }
    }

    /// Persist the file list to a directory as CSV.
    pub fn save(&self, dir: &Path) -> Result<(), StoreError> {
        std::fs::create_dir_all(dir).map_err(|e| StoreError::Io {
            context: "creating file list directory",
            source: e,
        })?;

        let path = list_path(dir, &self.name);
        let file = std::fs::File::create(&path).map_err(|e| StoreError::Io {
            context: "creating file list",
            source: e,
        })?;

        let mut wtr = csv::Writer::from_writer(file);
        for f in &self.files {
            wtr.write_record([f.as_str()]).map_err(|e| StoreError::Io {
                context: "writing file list",
                source: e.into(),
            })?;
        }
        wtr.flush().map_err(|e| StoreError::Io {
            context: "flushing file list",
            source: e,
        })?;

        Ok(())
    }

    /// Delete the .list file from disk.
    pub fn remove(dir: &Path, name: &str) {
        let _ = std::fs::remove_file(list_path(dir, name));
    }

    pub fn files(&self) -> &[PackagePath] {
        &self.files
    }
}

fn list_path(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!("{name}.list"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_roundtrip() {
        let dir = TempDir::new().unwrap();
        let files: Vec<PackagePath> = vec![
            "SCRIPTS/TOOLS/Test/main.lua".into(),
            "SCRIPTS/ELRS/crsf.lua".into(),
        ];
        let list = PackageFileList::new("test".into(), files.clone());
        list.save(dir.path()).unwrap();

        let loaded = PackageFileList::load(dir.path(), "test");
        assert_eq!(loaded.files(), &files[..]);
    }

    #[test]
    fn test_missing_returns_empty() {
        let dir = TempDir::new().unwrap();
        let loaded = PackageFileList::load(dir.path(), "nonexistent");
        assert!(loaded.files().is_empty());
    }
}
