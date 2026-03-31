use std::path::PathBuf;

use crate::manifest::{self, Manifest};
use crate::packages::path::PackagePath;
use crate::radio;
use crate::source::version::Channel;
use crate::source::{PackageRef, resolve};

use super::PackageError;
use super::file_list::PackageFileList;
use super::store::{InstalledPackage, PackageStore};
use super::transfer::{copy_content_items, count_files};

/// InstallOptions configures an install operation.
pub struct InstallOptions {
    pub sd_root: PathBuf,
    pub pkg_ref: PackageRef,
    pub dev: bool,
}

/// InstallResult holds the outcome of an install operation.
pub struct InstallResult {
    pub package: InstalledPackage,
    pub files_copied: usize,
}

/// InstallCommand holds the resolved manifest and metadata, ready for execution.
pub struct InstallCommand {
    pub manifest: Manifest,
    pub manifest_dir: PathBuf,
    pub package: InstalledPackage,
    include_dev: bool,
    store: PackageStore,
}

impl InstallCommand {
    /// Create a new install command by resolving the package ref, loading the manifest, and checking for conflicts.
    pub fn new(opts: InstallOptions) -> Result<InstallCommand, PackageError> {
        let mut store = PackageStore::load(opts.sd_root)?;

        let canonical = opts.pkg_ref.canonical();

        let (m, manifest_dir, channel, version, commit) = match &opts.pkg_ref {
            PackageRef::Local { path, sub_path } => {
                let (m, mdir) = manifest::load_with_sub_path(path, sub_path)
                    .map_err(|e| PackageError::Source(e.into()))?;
                (m, mdir, Channel::Local, String::new(), String::new())
            }
            PackageRef::Remote { .. } => {
                let result = resolve::resolve_package(&opts.pkg_ref)?;
                (
                    result.manifest,
                    result.manifest_dir,
                    result.resolved.channel,
                    result.resolved.version,
                    result.resolved.hash,
                )
            }
        };

        // Check min_edgetx_version
        if !m.package.min_edgetx_version.is_empty() {
            if let Some(info) = radio::radioinfo::load_radio_info(store.sd_root())? {
                if !info.semver.is_empty() {
                    radio::version::check_version_compatibility(
                        &info.semver,
                        &m.package.min_edgetx_version,
                    )?;
                }
            } else {
                log::warn!("could not determine radio firmware version, skipping version check");
            }
        }

        // Remove existing package with same source or name
        let existing_source = if store.find_by_source(&canonical).is_some() {
            Some(canonical.clone())
        } else {
            let by_name = store.find_by_name(&m.package.name);
            if by_name.len() == 1 {
                Some(by_name[0].source.clone())
            } else {
                None
            }
        };

        if let Some(src) = existing_source {
            store.remove(&src);
        }

        let paths = m.all_paths(opts.dev);
        store.check_conflicts(&paths, "")?;

        Ok(InstallCommand {
            manifest: m.clone(),
            manifest_dir,
            package: InstalledPackage {
                source: canonical,
                name: m.package.name,
                channel,
                version,
                commit,
                paths,
                dev: opts.dev,
            },
            include_dev: opts.dev,
            store,
        })
    }

    /// Returns the number of files that will be copied.
    pub fn total_files(&self) -> usize {
        count_files(&self.manifest_dir, &self.manifest, self.include_dev)
    }

    /// Execute copies the files and updates the state.
    pub fn execute(
        self,
        dry_run: bool,
        mut on_file: impl FnMut(&str),
    ) -> Result<InstallResult, PackageError> {
        let mut total_copied = 0;
        let mut store = self.store;
        let sd_root = store.sd_root().to_path_buf();

        if !dry_run {
            let (n, mut copied_files) = copy_content_items(
                &self.manifest,
                &self.manifest_dir,
                &sd_root,
                self.include_dev,
                &mut on_file,
            )?;
            total_copied = n;

            store.add(self.package.clone());
            store.save()?;

            // Track content directories for cleanup on removal
            for item in self.manifest.content_items(self.include_dev) {
                copied_files.push(PackagePath::new(format!("{}/", item.path)));
            }

            PackageFileList::new(self.package.name.clone(), copied_files)
                .save(&store.file_list_dir)?;
        }

        Ok(InstallResult {
            package: self.package,
            files_copied: total_copied,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_local_package(content: &str, content_paths: &[&str]) -> (TempDir, TempDir) {
        let pkg_dir = TempDir::new().unwrap();
        std::fs::write(pkg_dir.path().join("edgetx.yml"), content).unwrap();
        for p in content_paths {
            let full = pkg_dir.path().join(p);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(&full, "-- lua content").unwrap();
        }

        let sd_dir = TempDir::new().unwrap();
        std::fs::create_dir_all(sd_dir.path().join("RADIO")).unwrap();

        (pkg_dir, sd_dir)
    }

    #[test]
    fn test_install_local_package() {
        let (pkg_dir, sd_dir) = setup_local_package(
            r#"
package:
  name: test-pkg
tools:
  - name: MyTool
    path: SCRIPTS/TOOLS/MyTool
"#,
            &["SCRIPTS/TOOLS/MyTool/main.lua"],
        );

        let cmd = InstallCommand::new(InstallOptions {
            sd_root: sd_dir.path().to_path_buf(),
            pkg_ref: PackageRef::Local {
                path: pkg_dir.path().to_path_buf(),
                sub_path: String::new(),
            },
            dev: false,
        })
        .unwrap();

        assert_eq!(cmd.package.name, "test-pkg");
        assert_eq!(cmd.package.channel, Channel::Local);

        let result = cmd.execute(false, |_| {}).unwrap();
        assert_eq!(result.files_copied, 1);

        // Verify state
        let store = PackageStore::load(sd_dir.path().to_path_buf()).unwrap();
        assert_eq!(store.packages().len(), 1);
        assert_eq!(store.packages()[0].name, "test-pkg");

        // Verify file was copied
        assert!(sd_dir.path().join("SCRIPTS/TOOLS/MyTool/main.lua").exists());
    }

    #[test]
    fn test_install_saves_directory_entries() {
        let (pkg_dir, sd_dir) = setup_local_package(
            r#"
package:
  name: test-pkg
tools:
  - name: MyTool
    path: SCRIPTS/TOOLS/MyTool
"#,
            &["SCRIPTS/TOOLS/MyTool/main.lua"],
        );

        let cmd = InstallCommand::new(InstallOptions {
            sd_root: sd_dir.path().to_path_buf(),
            pkg_ref: PackageRef::Local {
                path: pkg_dir.path().to_path_buf(),
                sub_path: String::new(),
            },
            dev: false,
        })
        .unwrap();

        cmd.execute(false, |_| {}).unwrap();

        let store = PackageStore::load(sd_dir.path().to_path_buf()).unwrap();
        let file_list = PackageFileList::load(&store.file_list_dir, "test-pkg");
        assert!(
            file_list
                .files()
                .iter()
                .any(|e| e == "SCRIPTS/TOOLS/MyTool/")
        );
        assert!(
            file_list
                .files()
                .iter()
                .any(|e| e == "SCRIPTS/TOOLS/MyTool/main.lua")
        );
    }

    #[test]
    fn test_install_dry_run() {
        let (pkg_dir, sd_dir) = setup_local_package(
            r#"
package:
  name: test-pkg
tools:
  - name: MyTool
    path: SCRIPTS/TOOLS/MyTool
"#,
            &["SCRIPTS/TOOLS/MyTool/main.lua"],
        );

        let cmd = InstallCommand::new(InstallOptions {
            sd_root: sd_dir.path().to_path_buf(),
            pkg_ref: PackageRef::Local {
                path: pkg_dir.path().to_path_buf(),
                sub_path: String::new(),
            },
            dev: false,
        })
        .unwrap();

        let result = cmd.execute(true, |_| {}).unwrap();
        assert_eq!(result.files_copied, 0);

        // Verify no state was saved
        let store = PackageStore::load(sd_dir.path().to_path_buf()).unwrap();
        assert!(store.packages().is_empty());
    }
}
