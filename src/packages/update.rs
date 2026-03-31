use std::path::PathBuf;

use crate::manifest::{self, Manifest};
use crate::source::version::Channel;
use crate::source::{PackageRef, resolve};

use super::PackageError;
use super::file_list::PackageFileList;
use super::store::{InstalledPackage, PackageStore};
use super::transfer::{copy_content_items, count_files};

/// UpdateOptions configures the construction of an UpdateCommand.
pub struct UpdateOptions<'a> {
    pub pkg: &'a InstalledPackage,
    pub version_override: &'a str,
    pub include_dev: bool,
}

/// UpdateResult holds the outcome of updating a single package, including the store for reuse.
pub struct UpdateResult {
    pub package: InstalledPackage,
    pub files_copied: usize,
    pub up_to_date: bool,
    pub store: PackageStore,
}

/// UpdateCommand holds the resolved manifest and metadata, ready for execution.
pub struct UpdateCommand {
    pub package: InstalledPackage,
    pub old_package: InstalledPackage,
    manifest: Manifest,
    manifest_dir: PathBuf,
    include_dev: bool,
    up_to_date: bool,
    store: PackageStore,
}

impl UpdateCommand {
    /// Create a new update command by resolving the new version and checking conflicts.
    pub fn new(opts: UpdateOptions, store: PackageStore) -> Result<UpdateCommand, PackageError> {
        let pkg = opts.pkg;
        let version_override = opts.version_override;
        let include_dev = opts.include_dev;

        // Pinned commits can't be updated without explicit version
        if pkg.channel.is_pinned() && version_override.is_empty() {
            return Ok(UpdateCommand {
                package: pkg.clone(),
                old_package: pkg.clone(),
                manifest: Manifest::default(),
                manifest_dir: PathBuf::new(),
                include_dev,
                up_to_date: true,
                store,
            });
        }

        let (m, manifest_dir, new_channel, new_version, new_commit) = if pkg.channel.is_local() {
            // Re-copy from local path
            let pkg_ref: PackageRef = pkg.source.parse()?;

            let (local_path, sub_path) = match &pkg_ref {
                PackageRef::Local { path, sub_path } => (path.clone(), sub_path.clone()),
                _ => {
                    return Err(PackageError::NotFound(format!(
                        "expected local package for channel=local, got {:?}",
                        pkg.source
                    )));
                }
            };

            let (m, mdir) = manifest::load_with_sub_path(&local_path, &sub_path)
                .map_err(|e| PackageError::Source(e.into()))?;
            (m, mdir, Channel::Local, String::new(), String::new())
        } else {
            let mut pkg_ref: PackageRef = pkg.source.parse()?;

            if !version_override.is_empty() {
                pkg_ref.set_version(version_override.to_string());
            } else if pkg.channel == Channel::Branch {
                pkg_ref.set_version(pkg.version.clone());
            }
            // tag channel with no override: leave version empty to get latest

            let result = resolve::resolve_package(&pkg_ref)?;

            // Check if already up to date
            if result.resolved.hash == pkg.commit {
                return Ok(UpdateCommand {
                    package: pkg.clone(),
                    old_package: pkg.clone(),
                    manifest: result.manifest,
                    manifest_dir: result.manifest_dir,
                    include_dev,
                    up_to_date: true,
                    store,
                });
            }

            (
                result.manifest,
                result.manifest_dir,
                result.resolved.channel,
                result.resolved.version,
                result.resolved.hash,
            )
        };

        let new_paths = m.all_paths(include_dev);

        // Check conflicts, skip both current and original source
        store.check_conflicts(&new_paths, &pkg.source)?;

        Ok(UpdateCommand {
            package: InstalledPackage {
                source: pkg.source.clone(),
                name: m.package.name.clone(),
                channel: new_channel,
                version: new_version,
                commit: new_commit,
                paths: new_paths,
                dev: include_dev,
            },
            old_package: pkg.clone(),
            manifest: m,
            manifest_dir,
            include_dev,
            up_to_date: false,
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
    ) -> Result<UpdateResult, PackageError> {
        if self.up_to_date {
            return Ok(UpdateResult {
                package: self.package,
                files_copied: 0,
                up_to_date: true,
                store: self.store,
            });
        }

        let new_paths = self.manifest.all_paths(self.include_dev);
        let mut store = self.store;
        let sd_root = store.sd_root().to_path_buf();

        if !dry_run {
            store.remove(&self.old_package.source);

            let (total_copied, copied_files) = copy_content_items(
                &self.manifest,
                &self.manifest_dir,
                &sd_root,
                self.include_dev,
                &mut on_file,
            )?;

            let updated = InstalledPackage {
                source: self.old_package.source.clone(),
                name: self.manifest.package.name.clone(),
                channel: self.package.channel,
                version: self.package.version.clone(),
                commit: self.package.commit.clone(),
                paths: new_paths,
                dev: self.include_dev,
            };
            store.add(updated.clone());
            store.save()?;
            PackageFileList::new(updated.name.clone(), copied_files).save(&store.file_list_dir)?;

            return Ok(UpdateResult {
                package: updated,
                files_copied: total_copied,
                up_to_date: false,
                store,
            });
        }

        Ok(UpdateResult {
            package: InstalledPackage {
                source: self.old_package.source.clone(),
                name: self.manifest.package.name,
                channel: self.package.channel,
                version: self.package.version,
                commit: self.package.commit,
                paths: new_paths,
                dev: self.include_dev,
            },
            files_copied: 0,
            up_to_date: false,
            store,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packages::path::PackagePath;
    use tempfile::TempDir;

    #[derive(Debug)]
    struct TestUpdateResult {
        package: InstalledPackage,
        files_copied: usize,
        up_to_date: bool,
    }

    /// Test helper: update one or all packages in a single call.
    fn update(
        sd_root: PathBuf,
        query: &str,
        all: bool,
        dev: Option<bool>,
        dry_run: bool,
    ) -> Result<Vec<TestUpdateResult>, PackageError> {
        let mut store = PackageStore::load(sd_root)?;

        let (canonical, version_override) = if !query.is_empty() {
            let pkg_ref: PackageRef = query.parse()?;
            (pkg_ref.canonical(), pkg_ref.version().to_string())
        } else {
            (String::new(), String::new())
        };

        let targets = store.update_targets(&canonical, all)?;

        let mut results = Vec::new();
        for target in &targets {
            let include_dev = dev.unwrap_or(target.dev);
            let cmd = UpdateCommand::new(
                UpdateOptions {
                    pkg: target,
                    version_override: &version_override,
                    include_dev,
                },
                store,
            )?;

            let result = cmd.execute(dry_run, |_| {})?;
            store = result.store;
            results.push(TestUpdateResult {
                package: result.package,
                files_copied: result.files_copied,
                up_to_date: result.up_to_date,
            });
        }

        Ok(results)
    }

    fn setup_local_installed(
        manifest: &str,
        files: &[(&str, &str)],
    ) -> (TempDir, TempDir, InstalledPackage) {
        let pkg_dir = TempDir::new().unwrap();
        std::fs::write(pkg_dir.path().join("edgetx.yml"), manifest).unwrap();
        for (path, content) in files {
            let full = pkg_dir.path().join(path);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(&full, content).unwrap();
        }

        let sd_dir = TempDir::new().unwrap();
        std::fs::create_dir_all(sd_dir.path().join("RADIO")).unwrap();

        let source = format!("local::{}", pkg_dir.path().display());
        let pkg = InstalledPackage {
            source: source.clone(),
            name: "test-pkg".into(),
            channel: Channel::Local,
            version: String::new(),
            commit: String::new(),
            paths: vec!["SCRIPTS/TOOLS/MyTool".into()],
            dev: false,
        };

        // Install initial files
        for (path, content) in files {
            let full = sd_dir.path().join(path);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(&full, content).unwrap();
        }

        // Save state
        let mut store = PackageStore::load(sd_dir.path().to_path_buf()).unwrap();
        store.add(pkg.clone());
        store.save().unwrap();

        PackageFileList::new(
            "test-pkg".into(),
            files.iter().map(|(p, _)| PackagePath::from(*p)).collect(),
        )
        .save(&store.file_list_dir)
        .unwrap();

        (pkg_dir, sd_dir, pkg)
    }

    #[test]
    fn test_update_local_package() {
        let (pkg_dir, sd_dir, _pkg) = setup_local_installed(
            "package:\n  name: test-pkg\ntools:\n  - name: MyTool\n    path: SCRIPTS/TOOLS/MyTool\n",
            &[("SCRIPTS/TOOLS/MyTool/main.lua", "-- original")],
        );

        // Modify source
        std::fs::write(
            pkg_dir.path().join("SCRIPTS/TOOLS/MyTool/main.lua"),
            "-- updated",
        )
        .unwrap();

        let results = update(
            sd_dir.path().to_path_buf(),
            &format!("local::{}", pkg_dir.path().display()),
            false,
            None,
            false,
        )
        .unwrap();

        assert_eq!(results.len(), 1);
        assert!(!results[0].up_to_date);
        assert!(results[0].files_copied > 0);

        let content =
            std::fs::read_to_string(sd_dir.path().join("SCRIPTS/TOOLS/MyTool/main.lua")).unwrap();
        assert_eq!(content, "-- updated");
    }

    #[test]
    fn test_update_pinned_commit_skipped() {
        let sd_dir = TempDir::new().unwrap();
        std::fs::create_dir_all(sd_dir.path().join("RADIO")).unwrap();

        let mut store = PackageStore::load(sd_dir.path().to_path_buf()).unwrap();
        store.add(InstalledPackage {
            source: "Org/Repo".into(),
            name: "pinned-pkg".into(),
            channel: Channel::Commit,
            version: "abc123".into(),
            commit: "abc123".into(),
            paths: vec![],
            dev: false,
        });
        store.save().unwrap();

        let results = update(sd_dir.path().to_path_buf(), "Org/Repo", false, None, false).unwrap();

        assert_eq!(results.len(), 1);
        assert!(results[0].up_to_date);
    }

    #[test]
    fn test_update_not_found() {
        let sd_dir = TempDir::new().unwrap();
        std::fs::create_dir_all(sd_dir.path().join("RADIO")).unwrap();

        let store = PackageStore::load(sd_dir.path().to_path_buf()).unwrap();
        store.save().unwrap();

        let result = update(
            sd_dir.path().to_path_buf(),
            "NonExistent/Repo",
            false,
            None,
            false,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_update_requires_query_or_all() {
        let sd_dir = TempDir::new().unwrap();
        std::fs::create_dir_all(sd_dir.path().join("RADIO")).unwrap();

        let result = update(sd_dir.path().to_path_buf(), "", false, None, false);
        assert!(result.is_err());
        assert!(
            result.unwrap_err().to_string().contains("--all"),
            "error should mention --all"
        );
    }
}
