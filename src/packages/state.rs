use crate::source::version::Channel;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use thiserror::Error;

use super::PackageError;
use super::path::PackagePath;

const STATE_FILE_NAME: &str = "RADIO/packages.yml";
const FILE_LIST_DIR: &str = "RADIO/packages";

#[derive(Error, Debug)]
pub enum StateError {
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

/// State holds the list of installed packages on an SD card.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct State {
    pub packages: Vec<InstalledPackage>,
}

/// Load the state file from the SD card root. Returns empty state if file doesn't exist.
pub fn load_state(sd_root: &Path) -> Result<State, StateError> {
    let path = sd_root.join(STATE_FILE_NAME);

    match std::fs::read_to_string(&path) {
        Ok(data) => {
            let s: State = serde_yml::from_str(&data).map_err(|e| StateError::Parse {
                path: path.clone(),
                source: e,
            })?;
            Ok(s)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(State::default()),
        Err(e) => Err(StateError::Io {
            context: "reading state file",
            source: e,
        }),
    }
}

impl State {
    /// Save writes the state file to the SD card root.
    pub fn save(&self, sd_root: &Path) -> Result<(), StateError> {
        let path = sd_root.join(STATE_FILE_NAME);

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| StateError::Io {
                context: "creating state directory",
                source: e,
            })?;
        }

        let data = serde_yml::to_string(self).map_err(StateError::Serialize)?;

        std::fs::write(&path, data).map_err(|e| StateError::Io {
            context: "writing state file",
            source: e,
        })?;

        Ok(())
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

    /// Remove the package with the given canonical source.
    pub fn remove(&mut self, canonical: &str) {
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
}

fn file_list_path(sd_root: &Path, name: &str) -> PathBuf {
    sd_root.join(FILE_LIST_DIR).join(format!("{name}.list"))
}

/// Save the list of installed files for a package as CSV.
pub fn save_file_list(sd_root: &Path, name: &str, files: &[PackagePath]) -> Result<(), StateError> {
    let path = file_list_path(sd_root, name);

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| StateError::Io {
            context: "creating file list directory",
            source: e,
        })?;
    }

    let file = std::fs::File::create(&path).map_err(|e| StateError::Io {
        context: "creating file list",
        source: e,
    })?;

    let mut wtr = csv::Writer::from_writer(file);
    for f in files {
        wtr.write_record([f.as_str()]).map_err(|e| StateError::Io {
            context: "writing file list",
            source: e.into(),
        })?;
    }
    wtr.flush().map_err(|e| StateError::Io {
        context: "flushing file list",
        source: e,
    })?;

    Ok(())
}

/// Load the list of installed files for a package from CSV.
/// Returns empty vec if file doesn't exist.
pub fn load_file_list(sd_root: &Path, name: &str) -> Vec<PackagePath> {
    let path = file_list_path(sd_root, name);

    let file = match std::fs::File::open(&path) {
        Ok(f) => f,
        Err(_) => return Vec::new(),
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
    files
}

/// Remove the .list file for a package.
pub fn remove_file_list(sd_root: &Path, name: &str) {
    let _ = std::fs::remove_file(file_list_path(sd_root, name));
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
    fn test_load_empty_state() {
        let dir = setup();
        let state = load_state(dir.path()).unwrap();
        assert!(state.packages.is_empty());
    }

    #[test]
    fn test_save_and_load_state() {
        let dir = setup();
        let mut state = State::default();
        state.add(InstalledPackage {
            source: "Org/Repo".into(),
            name: "test".into(),
            channel: Channel::Tag,
            version: "v1.0.0".into(),
            commit: "abc123def456".into(),
            paths: vec!["SCRIPTS/TOOLS/Test".into()],
            dev: false,
        });
        state.save(dir.path()).unwrap();

        let loaded = load_state(dir.path()).unwrap();
        assert_eq!(loaded.packages.len(), 1);
        assert_eq!(loaded.packages[0].source, "Org/Repo");
        assert_eq!(loaded.packages[0].name, "test");
    }

    #[test]
    fn test_find_by_source() {
        let mut state = State::default();
        state.add(InstalledPackage {
            source: "Org/Repo".into(),
            name: "test".into(),
            channel: Channel::Tag,
            version: String::new(),
            commit: String::new(),
            paths: vec![],
            dev: false,
        });
        assert!(state.find_by_source("Org/Repo").is_some());
        assert!(state.find_by_source("Other/Repo").is_none());
    }

    #[test]
    fn test_find_by_name() {
        let mut state = State::default();
        state.add(InstalledPackage {
            source: "Org/Repo".into(),
            name: "test".into(),
            channel: Channel::Tag,
            version: String::new(),
            commit: String::new(),
            paths: vec![],
            dev: false,
        });
        assert_eq!(state.find_by_name("test").len(), 1);
        assert_eq!(state.find_by_name("other").len(), 0);
    }

    #[test]
    fn test_find_ambiguous() {
        let mut state = State::default();
        state.add(InstalledPackage {
            source: "Org1/Repo".into(),
            name: "test".into(),
            channel: Channel::Tag,
            version: String::new(),
            commit: String::new(),
            paths: vec![],
            dev: false,
        });
        state.add(InstalledPackage {
            source: "Org2/Repo".into(),
            name: "test".into(),
            channel: Channel::Tag,
            version: String::new(),
            commit: String::new(),
            paths: vec![],
            dev: false,
        });
        assert!(state.find("test").is_err());
    }

    #[test]
    fn test_remove() {
        let mut state = State::default();
        state.add(InstalledPackage {
            source: "Org/Repo".into(),
            name: "test".into(),
            channel: Channel::Tag,
            version: String::new(),
            commit: String::new(),
            paths: vec![],
            dev: false,
        });
        state.remove("Org/Repo");
        assert!(state.packages.is_empty());
    }

    #[test]
    fn test_add_replaces() {
        let mut state = State::default();
        state.add(InstalledPackage {
            source: "Org/Repo".into(),
            name: "test".into(),
            channel: Channel::Tag,
            version: "v1.0.0".into(),
            commit: String::new(),
            paths: vec![],
            dev: false,
        });
        state.add(InstalledPackage {
            source: "Org/Repo".into(),
            name: "test".into(),
            channel: Channel::Tag,
            version: "v2.0.0".into(),
            commit: String::new(),
            paths: vec![],
            dev: false,
        });
        assert_eq!(state.packages.len(), 1);
        assert_eq!(state.packages[0].version, "v2.0.0");
    }

    #[test]
    fn test_file_list_roundtrip() {
        let dir = setup();
        let files: Vec<PackagePath> = vec![
            "SCRIPTS/TOOLS/Test/main.lua".into(),
            "SCRIPTS/ELRS/crsf.lua".into(),
        ];
        save_file_list(dir.path(), "test", &files).unwrap();

        let loaded = load_file_list(dir.path(), "test");
        assert_eq!(loaded, files);
    }

    #[test]
    fn test_file_list_missing() {
        let dir = setup();
        let loaded = load_file_list(dir.path(), "nonexistent");
        assert!(loaded.is_empty());
    }
}
