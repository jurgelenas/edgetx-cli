use anyhow::Result;
use std::path::PathBuf;

use crate::source::PackageRef;

use super::install::clean_empty_parents;
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
    pub files: Vec<String>,
    state: State,
    sd_root: PathBuf,
}

impl PreparedRemove {
    /// Returns the number of files that will be removed.
    pub fn total_files(&self) -> usize {
        self.files.len()
    }

    /// Execute performs the removal. If dry_run is true, no files are deleted.
    pub fn execute(self, dry_run: bool, on_file: impl Fn(&str)) -> Result<RemoveResult> {
        if dry_run {
            return Ok(RemoveResult {
                package: self.package,
                files_removed: 0,
            });
        }

        for f in &self.files {
            let full = self.sd_root.join(f);
            let _ = std::fs::remove_file(&full);
            on_file(f);
        }

        let files_removed = self.files.len();

        for f in &self.files {
            clean_empty_parents(&self.sd_root, f);
        }

        state::remove_file_list(&self.sd_root, &self.package.name);

        let mut state = self.state;
        state.remove(&self.package.source);
        state
            .save(&self.sd_root)
            .map_err(|e| anyhow::anyhow!("saving state: {e}"))?;

        Ok(RemoveResult {
            package: self.package,
            files_removed,
        })
    }
}

/// Prepare the removal: resolve the package and file list without deleting anything.
pub fn prepare_remove(opts: RemoveOptions) -> Result<PreparedRemove> {
    let state = state::load_state(&opts.sd_root).map_err(|e| anyhow::anyhow!("{e}"))?;

    let pkg_ref: PackageRef = opts.query.parse().map_err(|e| anyhow::anyhow!("{e}"))?;
    let pkg = state
        .find(&pkg_ref.canonical())
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    let files = state::load_file_list(&opts.sd_root, &pkg.name);

    Ok(PreparedRemove {
        package: pkg.clone(),
        files,
        state,
        sd_root: opts.sd_root,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_installed_package() -> (TempDir, String) {
        let sd_dir = TempDir::new().unwrap();
        let sd = sd_dir.path();

        std::fs::create_dir_all(sd.join("RADIO/packages")).unwrap();
        std::fs::create_dir_all(sd.join("SCRIPTS/TOOLS/MyTool")).unwrap();
        std::fs::write(sd.join("SCRIPTS/TOOLS/MyTool/main.lua"), "-- lua").unwrap();

        // Write state
        let state = State {
            packages: vec![InstalledPackage {
                source: "Org/Repo".into(),
                name: "test-pkg".into(),
                channel: "tag".into(),
                version: "v1.0.0".into(),
                commit: "abc123".into(),
                paths: vec!["SCRIPTS/TOOLS/MyTool".into()],
                dev: false,
            }],
        };
        state.save(sd).unwrap();

        // Write file list
        state::save_file_list(sd, "test-pkg", &["SCRIPTS/TOOLS/MyTool/main.lua".into()]).unwrap();

        (sd_dir, "Org/Repo".into())
    }

    #[test]
    fn test_remove_package() {
        let (sd_dir, source) = setup_installed_package();
        let sd = sd_dir.path();

        let prepared = prepare_remove(RemoveOptions {
            sd_root: sd.to_path_buf(),
            query: source,
        })
        .unwrap();

        assert_eq!(prepared.package.name, "test-pkg");
        assert_eq!(prepared.total_files(), 1);

        let result = prepared.execute(false, |_| {}).unwrap();
        assert_eq!(result.files_removed, 1);

        // File should be gone
        assert!(!sd.join("SCRIPTS/TOOLS/MyTool/main.lua").exists());

        // State should be empty
        let state = state::load_state(sd).unwrap();
        assert!(state.packages.is_empty());
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

        // File should still exist
        assert!(sd.join("SCRIPTS/TOOLS/MyTool/main.lua").exists());
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
