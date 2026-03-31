use std::path::PathBuf;

use crate::source::PackageRef;

use super::PackageError;
use super::install::remove_empty_tree;
use super::state::{self, InstalledPackage, State};

/// RemoveOptions configures a remove operation.
pub struct RemoveOptions {
    pub sd_root: PathBuf,
    pub query: String,
}

/// RemoveResult holds the outcome of a remove operation.
pub struct RemoveResult {
    pub package: InstalledPackage,
    pub files_removed: usize,
}

/// PreparedRemove holds the state needed to execute a package removal.
pub struct PreparedRemove {
    pub package: InstalledPackage,
    /// File entries from the .list (no trailing /)
    pub files: Vec<String>,
    /// Directory entries from the .list (trailing / stripped)
    pub dirs: Vec<String>,
    /// .luac companions that exist on disk
    pub luac_files: Vec<String>,
    state: State,
    sd_root: PathBuf,
}

impl PreparedRemove {
    /// Returns the number of files that will be removed.
    pub fn total_files(&self) -> usize {
        self.files.len() + self.luac_files.len()
    }

    /// Execute performs the removal. If dry_run is true, no files are deleted.
    pub fn execute(
        self,
        dry_run: bool,
        on_file: impl Fn(&str),
    ) -> Result<RemoveResult, PackageError> {
        if dry_run {
            return Ok(RemoveResult {
                package: self.package,
                files_removed: 0,
            });
        }

        // Delete tracked files
        for f in &self.files {
            let _ = std::fs::remove_file(self.sd_root.join(f));
            on_file(f);
        }

        // Delete .luac companions
        for f in &self.luac_files {
            let _ = std::fs::remove_file(self.sd_root.join(f));
            on_file(f);
        }

        let files_removed = self.files.len() + self.luac_files.len();

        // Remove tracked directories (deepest first handled by remove_empty_tree)
        for d in &self.dirs {
            remove_empty_tree(&self.sd_root, d);
        }

        state::remove_file_list(&self.sd_root, &self.package.name);

        let mut state = self.state;
        state.remove(&self.package.source);
        state.save(&self.sd_root)?;

        Ok(RemoveResult {
            package: self.package,
            files_removed,
        })
    }
}

/// Prepare the removal: resolve the package and file list without deleting anything.
pub fn prepare_remove(opts: RemoveOptions) -> Result<PreparedRemove, PackageError> {
    let state = state::load_state(&opts.sd_root)?;

    let pkg_ref: PackageRef = opts.query.parse()?;
    let pkg = state.find(&pkg_ref.canonical())?;

    let entries = state::load_file_list(&opts.sd_root, &pkg.name);

    // Partition into files and directories
    let mut files = Vec::new();
    let mut dirs = Vec::new();
    for entry in entries {
        if entry.is_dir() {
            dirs.push(entry.into_inner().trim_end_matches('/').to_string());
        } else {
            files.push(entry.into_inner());
        }
    }

    // Find compiled .luac companions that exist on disk
    let luac_files: Vec<String> = files
        .iter()
        .filter(|f| f.ends_with(".lua"))
        .map(|f| format!("{f}c"))
        .filter(|luac| opts.sd_root.join(luac).exists())
        .collect();

    Ok(PreparedRemove {
        package: pkg.clone(),
        files,
        dirs,
        luac_files,
        state,
        sd_root: opts.sd_root,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    use crate::source::version::Channel;

    fn setup_installed_package() -> (TempDir, String) {
        let sd_dir = TempDir::new().unwrap();
        let sd = sd_dir.path();

        std::fs::create_dir_all(sd.join("RADIO/packages")).unwrap();
        std::fs::create_dir_all(sd.join("SCRIPTS/TOOLS/MyTool")).unwrap();
        std::fs::write(sd.join("SCRIPTS/TOOLS/MyTool/main.lua"), "-- lua").unwrap();
        // Simulate radio-generated .luac file
        std::fs::write(sd.join("SCRIPTS/TOOLS/MyTool/main.luac"), "bytecode").unwrap();

        // Write state
        let state = State {
            packages: vec![InstalledPackage {
                source: "Org/Repo".into(),
                name: "test-pkg".into(),
                channel: Channel::Tag,
                version: "v1.0.0".into(),
                commit: "abc123".into(),
                paths: vec!["SCRIPTS/TOOLS/MyTool".into()],
                dev: false,
            }],
        };
        state.save(sd).unwrap();

        // Write file list with directory entry
        state::save_file_list(
            sd,
            "test-pkg",
            &[
                "SCRIPTS/TOOLS/MyTool/main.lua".into(),
                "SCRIPTS/TOOLS/MyTool/".into(),
            ],
        )
        .unwrap();

        (sd_dir, "Org/Repo".into())
    }

    #[test]
    fn test_remove_package_with_luac() {
        let (sd_dir, source) = setup_installed_package();
        let sd = sd_dir.path();

        let prepared = prepare_remove(RemoveOptions {
            sd_root: sd.to_path_buf(),
            query: source,
        })
        .unwrap();

        assert_eq!(prepared.package.name, "test-pkg");
        assert_eq!(prepared.files.len(), 1);
        assert_eq!(prepared.luac_files.len(), 1);
        assert_eq!(prepared.dirs.len(), 1);
        assert_eq!(prepared.total_files(), 2); // 1 file + 1 luac

        let result = prepared.execute(false, |_| {}).unwrap();
        assert_eq!(result.files_removed, 2);

        // Both .lua and .luac should be gone
        assert!(!sd.join("SCRIPTS/TOOLS/MyTool/main.lua").exists());
        assert!(!sd.join("SCRIPTS/TOOLS/MyTool/main.luac").exists());

        // Package directory should be removed (was empty after file deletion)
        assert!(!sd.join("SCRIPTS/TOOLS/MyTool").exists());

        // System directory should still exist
        assert!(sd.join("SCRIPTS/TOOLS").exists());

        // State should be empty
        let state = state::load_state(sd).unwrap();
        assert!(state.packages.is_empty());
    }

    #[test]
    fn test_remove_keeps_dir_with_other_files() {
        let (sd_dir, source) = setup_installed_package();
        let sd = sd_dir.path();

        // Add a non-package file to the directory
        std::fs::write(sd.join("SCRIPTS/TOOLS/MyTool/user_config.txt"), "custom").unwrap();

        let prepared = prepare_remove(RemoveOptions {
            sd_root: sd.to_path_buf(),
            query: source,
        })
        .unwrap();

        let result = prepared.execute(false, |_| {}).unwrap();
        assert_eq!(result.files_removed, 2);

        // Package files gone
        assert!(!sd.join("SCRIPTS/TOOLS/MyTool/main.lua").exists());
        assert!(!sd.join("SCRIPTS/TOOLS/MyTool/main.luac").exists());

        // Directory should still exist (has user_config.txt)
        assert!(sd.join("SCRIPTS/TOOLS/MyTool").exists());
        assert!(sd.join("SCRIPTS/TOOLS/MyTool/user_config.txt").exists());
    }

    #[test]
    fn test_remove_dry_run() {
        let (sd_dir, source) = setup_installed_package();
        let sd = sd_dir.path();

        let prepared = prepare_remove(RemoveOptions {
            sd_root: sd.to_path_buf(),
            query: source,
        })
        .unwrap();

        let result = prepared.execute(true, |_| {}).unwrap();
        assert_eq!(result.files_removed, 0);

        // All files should still exist
        assert!(sd.join("SCRIPTS/TOOLS/MyTool/main.lua").exists());
        assert!(sd.join("SCRIPTS/TOOLS/MyTool/main.luac").exists());
    }

    #[test]
    fn test_remove_not_found() {
        let (sd_dir, _) = setup_installed_package();

        let result = prepare_remove(RemoveOptions {
            sd_root: sd_dir.path().to_path_buf(),
            query: "NonExistent/Repo".into(),
        });
        assert!(result.is_err());
    }
}
