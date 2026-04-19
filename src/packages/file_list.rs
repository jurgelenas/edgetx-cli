use std::path::{Path, PathBuf};

use super::path::PackagePath;
use super::store::StoreError;

/// Leaf filename inside each per-package directory.
const LIST_FILENAME: &str = "files.list";

/// Per-package list of files installed on the SD card.
/// Used to track what was copied so it can be cleanly removed.
pub struct PackageFileList {
    id: String,
    files: Vec<PackagePath>,
}

impl PackageFileList {
    pub fn new(id: String, files: Vec<PackagePath>) -> Self {
        Self { id, files }
    }

    /// Load the file list for a package id. Returns empty list if missing.
    pub fn load(file_list_root: &Path, id: &str) -> Self {
        let path = list_path(file_list_root, id);

        let file = match std::fs::File::open(&path) {
            Ok(f) => f,
            Err(_) => {
                return Self {
                    id: id.to_string(),
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
            id: id.to_string(),
            files,
        }
    }

    /// Persist the file list for this package into its per-package directory.
    pub fn save(&self, file_list_root: &Path) -> Result<(), StoreError> {
        let pkg_dir = package_dir(file_list_root, &self.id);
        std::fs::create_dir_all(&pkg_dir).map_err(|e| StoreError::Io {
            context: "creating file list directory",
            source: e,
        })?;

        let path = pkg_dir.join(LIST_FILENAME);
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

    /// Delete the per-package file list and prune empty parent directories up to file_list_root.
    pub fn remove(file_list_root: &Path, id: &str) {
        let pkg_dir = package_dir(file_list_root, id);
        let _ = std::fs::remove_file(pkg_dir.join(LIST_FILENAME));
        prune_empty_dirs(file_list_root, &pkg_dir);
    }

    pub fn files(&self) -> &[PackagePath] {
        &self.files
    }
}

/// Per-package directory: `{file_list_root}/{id-segments-as-path}/`
fn package_dir(file_list_root: &Path, id: &str) -> PathBuf {
    let mut p = file_list_root.to_path_buf();
    for segment in id.split('/') {
        p.push(segment);
    }
    p
}

/// Full path to the per-package files.list.
fn list_path(file_list_root: &Path, id: &str) -> PathBuf {
    package_dir(file_list_root, id).join(LIST_FILENAME)
}

/// Remove `start` if empty, then walk up removing each empty parent,
/// stopping at (but not removing) `root`.
fn prune_empty_dirs(root: &Path, start: &Path) {
    let mut cur = start.to_path_buf();
    loop {
        if !cur.starts_with(root) || cur == root {
            break;
        }
        match std::fs::read_dir(&cur) {
            Ok(mut it) => {
                if it.next().is_some() {
                    break; // not empty
                }
            }
            Err(_) => break, // gone or unreadable
        }
        if std::fs::remove_dir(&cur).is_err() {
            break;
        }
        match cur.parent() {
            Some(p) => cur = p.to_path_buf(),
            None => break,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    const TEST_ID: &str = "github.com/Org/Repo";

    #[test]
    fn test_roundtrip() {
        let dir = TempDir::new().unwrap();
        let files: Vec<PackagePath> = vec![
            "SCRIPTS/TOOLS/Test/main.lua".into(),
            "SCRIPTS/ELRS/crsf.lua".into(),
        ];
        let list = PackageFileList::new(TEST_ID.into(), files.clone());
        list.save(dir.path()).unwrap();

        // File lives at `{root}/github.com/Org/Repo/files.list`
        let expected = dir
            .path()
            .join("github.com")
            .join("Org")
            .join("Repo")
            .join("files.list");
        assert!(expected.is_file());

        let loaded = PackageFileList::load(dir.path(), TEST_ID);
        assert_eq!(loaded.files(), &files[..]);
    }

    #[test]
    fn test_missing_returns_empty() {
        let dir = TempDir::new().unwrap();
        let loaded = PackageFileList::load(dir.path(), "github.com/nobody/nothing");
        assert!(loaded.files().is_empty());
    }

    #[test]
    fn test_remove_prunes_empty_dirs() {
        let dir = TempDir::new().unwrap();
        let list = PackageFileList::new(TEST_ID.into(), vec!["SCRIPTS/TOOLS/X/main.lua".into()]);
        list.save(dir.path()).unwrap();

        PackageFileList::remove(dir.path(), TEST_ID);

        // Leaf file gone
        assert!(!dir.path().join("github.com/Org/Repo/files.list").exists());
        // All empty parent dirs pruned up to the root
        assert!(!dir.path().join("github.com/Org/Repo").exists());
        assert!(!dir.path().join("github.com/Org").exists());
        assert!(!dir.path().join("github.com").exists());
    }

    #[test]
    fn test_remove_keeps_sibling() {
        let dir = TempDir::new().unwrap();
        let a = PackageFileList::new(
            "github.com/Org/Repo-A".into(),
            vec!["SCRIPTS/TOOLS/A/main.lua".into()],
        );
        let b = PackageFileList::new(
            "github.com/Org/Repo-B".into(),
            vec!["SCRIPTS/TOOLS/B/main.lua".into()],
        );
        a.save(dir.path()).unwrap();
        b.save(dir.path()).unwrap();

        PackageFileList::remove(dir.path(), "github.com/Org/Repo-A");

        // A gone
        assert!(!dir.path().join("github.com/Org/Repo-A").exists());
        // B still there
        assert!(dir.path().join("github.com/Org/Repo-B/files.list").exists());
        // Shared parents remain
        assert!(dir.path().join("github.com/Org").exists());
        assert!(dir.path().join("github.com").exists());
    }

    #[test]
    fn test_subpackage_layout() {
        let dir = TempDir::new().unwrap();
        let list = PackageFileList::new(
            "github.com/Org/Tools/widget-a".into(),
            vec!["WIDGETS/widget-a/main.lua".into()],
        );
        list.save(dir.path()).unwrap();
        assert!(
            dir.path()
                .join("github.com/Org/Tools/widget-a/files.list")
                .is_file()
        );
    }
}
