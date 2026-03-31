use crate::source::version::Channel;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use thiserror::Error;

use super::PackageError;
use super::file_list::PackageFileList;
use super::path::PackagePath;

const STATE_FILE: &str = "RADIO/packages.yml";
const FILE_LIST_DIR: &str = "RADIO/packages";

#[derive(Error, Debug)]
pub enum StoreError {
    #[error("{context}: {source}")]
    Io {
        context: &'static str,
        source: std::io::Error,
    },
    #[error("parsing state file {path}: {source}")]
    Parse {
        path: PathBuf,
        source: serde_yml::Error,
    },
    #[error("serializing state: {0}")]
    Serialize(#[source] serde_yml::Error),
}

/// InstalledPackage describes a single package installed on the SD card.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledPackage {
    /// Canonical ID: "Org/Repo", "host/org/repo", or "local::/abs/path"
    pub source: String,
    /// Display name from remote edgetx.yml package name
    pub name: String,
    pub channel: Channel,
    /// Tag name or branch name (empty for commit/local)
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub version: String,
    /// Full SHA (empty for local)
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub commit: String,
    /// Relative paths on SD card
    pub paths: Vec<PackagePath>,
    /// True if dev dependencies were included
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub dev: bool,
}

impl InstalledPackage {
    /// Returns the first 7 characters of the commit hash, or the full string if shorter.
    pub fn short_commit(&self) -> &str {
        if self.commit.len() > 7 {
            &self.commit[..7]
        } else {
            &self.commit
        }
    }

    /// Returns a display string like `"tag v1.0.0 (abc1234)"`.
    pub fn channel_info(&self) -> String {
        let mut info = self.channel.to_string();
        if !self.version.is_empty() {
            info = format!("{info} {}", self.version);
        }
        if !self.commit.is_empty() {
            info = format!("{info} ({})", self.short_commit());
        }
        info
    }
}

/// Serializable shape of the packages.yml file.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PackageState {
    packages: Vec<InstalledPackage>,
}

/// PackageStore manages the list of installed packages on an SD card.
pub struct PackageStore {
    sd_root: PathBuf,
    state_file: PathBuf,
    pub file_list_dir: PathBuf,
    packages: Vec<InstalledPackage>,
}

impl PackageStore {
    /// Load the package store from the SD card. Returns empty store if no state file exists.
    pub fn load(sd_root: PathBuf) -> Result<Self, StoreError> {
        let state_file = sd_root.join(STATE_FILE);
        let file_list_dir = sd_root.join(FILE_LIST_DIR);

        let packages = match std::fs::read_to_string(&state_file) {
            Ok(data) => {
                let state: PackageState =
                    serde_yml::from_str(&data).map_err(|e| StoreError::Parse {
                        path: state_file.clone(),
                        source: e,
                    })?;
                state.packages
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => {
                return Err(StoreError::Io {
                    context: "reading state file",
                    source: e,
                });
            }
        };

        Ok(Self {
            sd_root,
            state_file,
            file_list_dir,
            packages,
        })
    }

    /// Persist the package list to RADIO/packages.yml.
    pub fn save(&self) -> Result<(), StoreError> {
        let path = &self.state_file;

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| StoreError::Io {
                context: "creating state directory",
                source: e,
            })?;
        }

        let state = PackageState {
            packages: self.packages.clone(),
        };
        let data = serde_yml::to_string(&state).map_err(StoreError::Serialize)?;

        std::fs::write(path, data).map_err(|e| StoreError::Io {
            context: "writing state file",
            source: e,
        })?;

        Ok(())
    }

    pub fn sd_root(&self) -> &Path {
        &self.sd_root
    }

    pub fn packages(&self) -> &[InstalledPackage] {
        &self.packages
    }

    /// Find by canonical source, or nil if not found.
    pub fn find_by_source(&self, canonical: &str) -> Option<&InstalledPackage> {
        self.packages.iter().find(|p| p.source == canonical)
    }

    /// Find all packages whose name matches.
    pub fn find_by_name(&self, name: &str) -> Vec<&InstalledPackage> {
        self.packages.iter().filter(|p| p.name == name).collect()
    }

    /// Find by source first, then by name. Returns error if ambiguous or not found.
    pub fn find(&self, query: &str) -> Result<&InstalledPackage, PackageError> {
        if let Some(pkg) = self.find_by_source(query) {
            return Ok(pkg);
        }

        let matches = self.find_by_name(query);
        match matches.len() {
            0 => Err(PackageError::NotFound(query.to_string())),
            1 => Ok(matches[0]),
            _ => {
                let sources: Vec<String> = matches.iter().map(|m| m.source.clone()).collect();
                Err(PackageError::Ambiguous {
                    name: query.to_string(),
                    sources,
                })
            }
        }
    }

    /// Remove the package with the given canonical source and clean up its tracked files from disk.
    pub fn remove(&mut self, canonical: &str) {
        if let Some(pkg) = self.packages.iter().find(|p| p.source == canonical) {
            self.remove_tracked_files(&pkg.name.clone());
        }
        self.packages.retain(|p| p.source != canonical);
    }

    /// Remove the package entry without cleaning up tracked files.
    /// Use this when the caller handles file deletion separately (e.g., with progress callbacks).
    pub fn remove_entry(&mut self, canonical: &str) {
        self.packages.retain(|p| p.source != canonical);
    }

    /// Add or replace a package. If same source exists, replace it.
    pub fn add(&mut self, pkg: InstalledPackage) {
        if let Some(existing) = self.packages.iter_mut().find(|p| p.source == pkg.source) {
            *existing = pkg;
        } else {
            self.packages.push(pkg);
        }
    }

    /// Find packages to update. If `all` is true, returns all packages.
    /// Otherwise, finds a single matching package by canonical source or name.
    pub fn update_targets(
        &self,
        query: &str,
        all: bool,
    ) -> Result<Vec<InstalledPackage>, PackageError> {
        if query.is_empty() && !all {
            return Err(PackageError::NotFound(
                "specify a package name or use --all".into(),
            ));
        }

        if all {
            return Ok(self.packages.clone());
        }

        match self.find(query) {
            Ok(pkg) => Ok(vec![pkg.clone()]),
            Err(_) => Err(PackageError::NotFound(format!(
                "package {:?} not found",
                query
            ))),
        }
    }

    /// Remove files installed by a package using the tracked file list.
    fn remove_tracked_files(&self, name: &str) {
        let file_list = PackageFileList::load(&self.file_list_dir, name);

        // Delete file entries + compiled .luac companions
        for f in file_list.files().iter().filter(|e| !e.is_dir()) {
            let _ = std::fs::remove_file(self.sd_root.join(f.as_str()));
            if let Some(compiled) = f.compiled_path() {
                let _ = std::fs::remove_file(self.sd_root.join(compiled.as_str()));
            }
        }

        // Remove tracked directories (deepest first)
        for d in file_list.files().iter().filter(|e| e.is_dir()) {
            self.remove_empty_tree(d.as_str().trim_end_matches('/'));
        }

        PackageFileList::remove(&self.file_list_dir, name);
    }

    /// Remove empty subdirectories within a tracked directory, bottom-up.
    /// Removes the directory itself if it ends up empty.
    /// Never walks above the given directory.
    pub(crate) fn remove_empty_tree(&self, rel_dir: &str) {
        let root = self.sd_root.join(rel_dir);
        if !root.is_dir() {
            return;
        }

        let mut dirs: Vec<PathBuf> = walkdir::WalkDir::new(&root)
            .into_iter()
            .flatten()
            .filter(|e| e.file_type().is_dir())
            .map(|e| e.into_path())
            .collect();
        dirs.sort_by(|a, b| b.cmp(a)); // deepest first

        for dir in dirs {
            let is_empty = std::fs::read_dir(&dir)
                .map(|mut entries| entries.next().is_none())
                .unwrap_or(false);
            if is_empty {
                let _ = std::fs::remove_dir(&dir);
            }
        }
    }

    /// Check if any of new_paths overlap with paths owned by already-installed packages.
    /// skip_source is excluded from checks (used during update to skip the package being updated).
    ///
    /// Overlap is determined by segment-based prefix matching (split on "/") to
    /// avoid false positives like "SCRIPTS/TOOLS" vs "SCRIPTS/TOOLSET".
    pub fn check_conflicts(
        &self,
        new_paths: &[PackagePath],
        skip_source: &str,
    ) -> Result<(), PackageError> {
        let mut installed: std::collections::HashMap<&PackagePath, &str> =
            std::collections::HashMap::new();
        for pkg in &self.packages {
            if pkg.source == skip_source {
                continue;
            }
            for p in &pkg.paths {
                installed.insert(p, pkg.source.as_str());
            }
        }

        let mut conflicts = Vec::new();
        for np in new_paths {
            let np_segs = np.segments();
            for (ip, owner) in &installed {
                let ip_segs = ip.segments();
                if segment_prefix_match(&np_segs, &ip_segs) {
                    conflicts.push(format!("{np} conflicts with {ip} (owned by {owner})"));
                }
            }
        }

        if !conflicts.is_empty() {
            return Err(PackageError::Conflicts(conflicts.join("\n  ")));
        }

        Ok(())
    }
}

/// Returns true if a is a prefix of b, b is a prefix of a, or they are equal
/// — all at segment boundaries.
fn segment_prefix_match(a: &[&str], b: &[&str]) -> bool {
    let shorter = a.len().min(b.len());
    for i in 0..shorter {
        if a[i] != b[i] {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup() -> TempDir {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("RADIO")).unwrap();
        dir
    }

    #[test]
    fn test_load_empty_store() {
        let dir = setup();
        let store = PackageStore::load(dir.path().to_path_buf()).unwrap();
        assert!(store.packages().is_empty());
    }

    #[test]
    fn test_save_and_load() {
        let dir = setup();
        let mut store = PackageStore::load(dir.path().to_path_buf()).unwrap();
        store.add(InstalledPackage {
            source: "Org/Repo".into(),
            name: "test".into(),
            channel: Channel::Tag,
            version: "v1.0.0".into(),
            commit: "abc123def456".into(),
            paths: vec!["SCRIPTS/TOOLS/Test".into()],
            dev: false,
        });
        store.save().unwrap();

        let loaded = PackageStore::load(dir.path().to_path_buf()).unwrap();
        assert_eq!(loaded.packages().len(), 1);
        assert_eq!(loaded.packages()[0].source, "Org/Repo");
        assert_eq!(loaded.packages()[0].name, "test");
    }

    #[test]
    fn test_find_by_source() {
        let dir = setup();
        let mut store = PackageStore::load(dir.path().to_path_buf()).unwrap();
        store.add(InstalledPackage {
            source: "Org/Repo".into(),
            name: "test".into(),
            channel: Channel::Tag,
            version: String::new(),
            commit: String::new(),
            paths: vec![],
            dev: false,
        });
        assert!(store.find_by_source("Org/Repo").is_some());
        assert!(store.find_by_source("Other/Repo").is_none());
    }

    #[test]
    fn test_find_by_name() {
        let dir = setup();
        let mut store = PackageStore::load(dir.path().to_path_buf()).unwrap();
        store.add(InstalledPackage {
            source: "Org/Repo".into(),
            name: "test".into(),
            channel: Channel::Tag,
            version: String::new(),
            commit: String::new(),
            paths: vec![],
            dev: false,
        });
        assert_eq!(store.find_by_name("test").len(), 1);
        assert_eq!(store.find_by_name("other").len(), 0);
    }

    #[test]
    fn test_find_ambiguous() {
        let dir = setup();
        let mut store = PackageStore::load(dir.path().to_path_buf()).unwrap();
        store.add(InstalledPackage {
            source: "Org1/Repo".into(),
            name: "test".into(),
            channel: Channel::Tag,
            version: String::new(),
            commit: String::new(),
            paths: vec![],
            dev: false,
        });
        store.add(InstalledPackage {
            source: "Org2/Repo".into(),
            name: "test".into(),
            channel: Channel::Tag,
            version: String::new(),
            commit: String::new(),
            paths: vec![],
            dev: false,
        });
        assert!(store.find("test").is_err());
    }

    #[test]
    fn test_remove() {
        let dir = setup();
        let mut store = PackageStore::load(dir.path().to_path_buf()).unwrap();
        store.add(InstalledPackage {
            source: "Org/Repo".into(),
            name: "test".into(),
            channel: Channel::Tag,
            version: String::new(),
            commit: String::new(),
            paths: vec![],
            dev: false,
        });
        store.remove("Org/Repo");
        assert!(store.packages().is_empty());
    }

    #[test]
    fn test_add_replaces() {
        let dir = setup();
        let mut store = PackageStore::load(dir.path().to_path_buf()).unwrap();
        store.add(InstalledPackage {
            source: "Org/Repo".into(),
            name: "test".into(),
            channel: Channel::Tag,
            version: "v1.0.0".into(),
            commit: String::new(),
            paths: vec![],
            dev: false,
        });
        store.add(InstalledPackage {
            source: "Org/Repo".into(),
            name: "test".into(),
            channel: Channel::Tag,
            version: "v2.0.0".into(),
            commit: String::new(),
            paths: vec![],
            dev: false,
        });
        assert_eq!(store.packages().len(), 1);
        assert_eq!(store.packages()[0].version, "v2.0.0");
    }

    fn store_with_packages(packages: Vec<(&str, Vec<&str>)>) -> PackageStore {
        let dir = setup();
        let mut store = PackageStore::load(dir.path().to_path_buf()).unwrap();
        for (source, paths) in packages {
            store.add(InstalledPackage {
                source: source.into(),
                name: source.into(),
                channel: Channel::Tag,
                version: String::new(),
                commit: String::new(),
                paths: paths.into_iter().map(PackagePath::from).collect(),
                dev: false,
            });
        }
        store
    }

    #[test]
    fn test_no_conflicts() {
        let store = store_with_packages(vec![("pkg-a", vec!["SCRIPTS/TOOLS/A"])]);
        let result = store.check_conflicts(&["SCRIPTS/TOOLS/B".into()], "");
        assert!(result.is_ok());
    }

    #[test]
    fn test_exact_match_conflict() {
        let store = store_with_packages(vec![("pkg-a", vec!["SCRIPTS/TOOLS/A"])]);
        let result = store.check_conflicts(&["SCRIPTS/TOOLS/A".into()], "");
        assert!(result.is_err());
    }

    #[test]
    fn test_prefix_overlap() {
        let store = store_with_packages(vec![("pkg-a", vec!["SCRIPTS/TOOLS"])]);
        let result = store.check_conflicts(&["SCRIPTS/TOOLS/B".into()], "");
        assert!(result.is_err());
    }

    #[test]
    fn test_no_false_positive() {
        let store = store_with_packages(vec![("pkg-a", vec!["SCRIPTS/TOOLS"])]);
        let result = store.check_conflicts(&["SCRIPTS/TOOLSET".into()], "");
        assert!(result.is_ok());
    }

    #[test]
    fn test_skip_source() {
        let store = store_with_packages(vec![("pkg-a", vec!["SCRIPTS/TOOLS/A"])]);
        let result = store.check_conflicts(&["SCRIPTS/TOOLS/A".into()], "pkg-a");
        assert!(result.is_ok());
    }

    #[test]
    fn test_multiple_conflicts() {
        let store = store_with_packages(vec![
            ("pkg-a", vec!["SCRIPTS/TOOLS/A"]),
            ("pkg-b", vec!["WIDGETS/B"]),
        ]);
        let result = store.check_conflicts(&["SCRIPTS/TOOLS/A".into(), "WIDGETS/B".into()], "");
        assert!(result.is_err());
    }

    #[test]
    fn test_remove_empty_tree() {
        let dir = setup();
        let sd = dir.path();
        let store = PackageStore::load(sd.to_path_buf()).unwrap();

        std::fs::create_dir_all(sd.join("SCRIPTS/TOOLS/MyTool/sub")).unwrap();

        store.remove_empty_tree("SCRIPTS/TOOLS/MyTool");
        assert!(!sd.join("SCRIPTS/TOOLS/MyTool").exists());
        assert!(sd.join("SCRIPTS/TOOLS").exists());
    }

    #[test]
    fn test_remove_empty_tree_deeply_nested() {
        let dir = setup();
        let sd = dir.path();
        let store = PackageStore::load(sd.to_path_buf()).unwrap();

        std::fs::create_dir_all(sd.join("SCRIPTS/TOOLS/MyTool/lib/utils/deep")).unwrap();

        store.remove_empty_tree("SCRIPTS/TOOLS/MyTool");
        assert!(!sd.join("SCRIPTS/TOOLS/MyTool").exists());
        assert!(sd.join("SCRIPTS/TOOLS").exists());
    }

    #[test]
    fn test_remove_empty_tree_keeps_nonempty() {
        let dir = setup();
        let sd = dir.path();
        let store = PackageStore::load(sd.to_path_buf()).unwrap();

        std::fs::create_dir_all(sd.join("SCRIPTS/TOOLS/MyTool/sub")).unwrap();
        std::fs::write(sd.join("SCRIPTS/TOOLS/MyTool/keep.txt"), "data").unwrap();

        store.remove_empty_tree("SCRIPTS/TOOLS/MyTool");
        assert!(sd.join("SCRIPTS/TOOLS/MyTool").exists());
        assert!(!sd.join("SCRIPTS/TOOLS/MyTool/sub").exists());
    }

    #[test]
    fn test_remove_cleans_tracked_files() {
        let dir = setup();
        let sd = dir.path();

        std::fs::create_dir_all(sd.join("RADIO/packages")).unwrap();
        std::fs::create_dir_all(sd.join("SCRIPTS/TOOLS/MyTool")).unwrap();
        std::fs::write(sd.join("SCRIPTS/TOOLS/MyTool/main.lua"), "-- lua").unwrap();
        std::fs::write(sd.join("SCRIPTS/TOOLS/MyTool/main.luac"), "bytecode").unwrap();

        let mut store = PackageStore::load(sd.to_path_buf()).unwrap();
        store.add(InstalledPackage {
            source: "Org/Repo".into(),
            name: "test-pkg".into(),
            channel: Channel::Tag,
            version: String::new(),
            commit: String::new(),
            paths: vec!["SCRIPTS/TOOLS/MyTool".into()],
            dev: false,
        });

        PackageFileList::new(
            "test-pkg".into(),
            vec![
                "SCRIPTS/TOOLS/MyTool/main.lua".into(),
                "SCRIPTS/TOOLS/MyTool/".into(),
            ],
        )
        .save(&store.file_list_dir)
        .unwrap();

        store.remove("Org/Repo");

        assert!(!sd.join("SCRIPTS/TOOLS/MyTool/main.lua").exists());
        assert!(!sd.join("SCRIPTS/TOOLS/MyTool/main.luac").exists());
        assert!(!sd.join("SCRIPTS/TOOLS/MyTool").exists());
        assert!(sd.join("SCRIPTS/TOOLS").exists());
        assert!(store.packages().is_empty());
    }
}
