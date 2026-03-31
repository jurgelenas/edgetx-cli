use std::path::PathBuf;

use crate::manifest::{self, Manifest};
use crate::source::version::Channel;
use crate::source::{PackageRef, resolve};

use super::PackageError;
use super::file_list::PackageFileList;
use super::install::remove_tracked_files;
use super::store::{InstalledPackage, PackageStore};
use super::transfer::{copy_content_items, count_files};

pub type BeforeCopyFn = Box<dyn Fn(&str, usize)>;
pub type OnFileFn = Box<dyn Fn(&str)>;

/// UpdateOptions configures an update operation.
pub struct UpdateOptions {
    pub sd_root: PathBuf,
    pub query: String,
    pub all: bool,
    pub dev: Option<bool>,
    pub dry_run: bool,
    pub before_copy: Option<BeforeCopyFn>,
    pub on_file: Option<OnFileFn>,
}

/// UpdateResult holds the outcome of updating a single package.
#[derive(Debug)]
pub struct UpdateResult {
    pub package: InstalledPackage,
    pub files_copied: usize,
    pub up_to_date: bool,
}

/// UpdateCommand holds the resolved manifest and metadata, ready for execution.
pub struct UpdateCommand {
    pub package: InstalledPackage,
    pub old_package: InstalledPackage,
    pub original_source: String,
    manifest: Manifest,
    manifest_dir: PathBuf,
    include_dev: bool,
    up_to_date: bool,
}

impl UpdateCommand {
    /// Resolve the new version, check conflicts, return an UpdateCommand ready for execution.
    pub fn resolve(
        pkg: &InstalledPackage,
        original_source: &str,
        store: &PackageStore,
        version_override: &str,
        include_dev: bool,
    ) -> Result<UpdateCommand, PackageError> {
        // Pinned commits can't be updated without explicit version
        if pkg.channel.is_pinned() && version_override.is_empty() {
            return Ok(UpdateCommand {
                package: pkg.clone(),
                old_package: pkg.clone(),
                original_source: original_source.to_string(),
                manifest: Manifest::default(),
                manifest_dir: PathBuf::new(),
                include_dev,
                up_to_date: true,
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
                    original_source: original_source.to_string(),
                    manifest: result.manifest,
                    manifest_dir: result.manifest_dir,
                    include_dev,
                    up_to_date: true,
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
        store.check_conflicts(&new_paths, original_source)?;

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
            original_source: original_source.to_string(),
            manifest: m,
            manifest_dir,
            include_dev,
            up_to_date: false,
        })
    }

    /// Returns the number of files that will be copied.
    pub fn total_files(&self) -> usize {
        count_files(&self.manifest_dir, &self.manifest, self.include_dev)
    }

    /// Execute copies the files and updates the state.
    pub fn execute(
        self,
        store: &mut PackageStore,
        dry_run: bool,
        mut on_file: impl FnMut(&str),
    ) -> Result<UpdateResult, PackageError> {
        if self.up_to_date {
            return Ok(UpdateResult {
                package: self.package,
                files_copied: 0,
                up_to_date: true,
            });
        }

        let new_paths = self.manifest.all_paths(self.include_dev);
        let sd_root = store.sd_root().to_path_buf();

        if !dry_run {
            remove_tracked_files(store, &self.old_package.name);
            store.remove(&self.original_source);

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
        })
    }
}

/// Update one or all installed packages.
pub fn update(opts: UpdateOptions) -> Result<Vec<UpdateResult>, PackageError> {
    if opts.query.is_empty() && !opts.all {
        return Err(PackageError::NotFound(
            "specify a package name or use --all".into(),
        ));
    }

    let mut store = PackageStore::load(opts.sd_root)?;

    let targets: Vec<InstalledPackage>;
    let original_sources: Vec<String>;
    let mut version_override = String::new();

    if opts.all {
        targets = store.packages().to_vec();
        original_sources = targets.iter().map(|t| t.source.clone()).collect();
    } else {
        let pkg_ref: PackageRef = opts.query.parse()?;
        let query = pkg_ref.canonical();
        version_override = pkg_ref.version().to_string();

        match store.find(&query) {
            Ok(pkg) => {
                original_sources = vec![pkg.source.clone()];
                targets = vec![pkg.clone()];
            }
            Err(_) => {
                return Err(PackageError::NotFound(format!(
                    "package {:?} not found",
                    opts.query
                )));
            }
        }
    }

    let mut results = Vec::new();
    for (i, pkg) in targets.iter().enumerate() {
        let include_dev = opts.dev.unwrap_or(pkg.dev);
        let cmd = UpdateCommand::resolve(
            pkg,
            &original_sources[i],
            &store,
            &version_override,
            include_dev,
        )?;

        if let Some(cb) = &opts.before_copy {
            cb(&cmd.package.name, cmd.total_files());
        }

        let result = cmd.execute(&mut store, opts.dry_run, |f| {
            if let Some(cb) = &opts.on_file {
                cb(f);
            }
        })?;
        results.push(result);
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packages::path::PackagePath;
    use tempfile::TempDir;

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

        let results = update(UpdateOptions {
            sd_root: sd_dir.path().to_path_buf(),
            query: format!("local::{}", pkg_dir.path().display()),
            all: false,
            dev: None,
            dry_run: false,
            before_copy: None,
            on_file: None,
        })
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

        let results = update(UpdateOptions {
            sd_root: sd_dir.path().to_path_buf(),
            query: "Org/Repo".into(),
            all: false,
            dev: None,
            dry_run: false,
            before_copy: None,
            on_file: None,
        })
        .unwrap();

        assert_eq!(results.len(), 1);
        assert!(results[0].up_to_date);
    }

    #[test]
    fn test_update_not_found() {
        let sd_dir = TempDir::new().unwrap();
        std::fs::create_dir_all(sd_dir.path().join("RADIO")).unwrap();

        let store = PackageStore::load(sd_dir.path().to_path_buf()).unwrap();
        store.save().unwrap();

        let result = update(UpdateOptions {
            sd_root: sd_dir.path().to_path_buf(),
            query: "NonExistent/Repo".into(),
            all: false,
            dev: None,
            dry_run: false,
            before_copy: None,
            on_file: None,
        });
        assert!(result.is_err());
    }

    #[test]
    fn test_update_requires_query_or_all() {
        let sd_dir = TempDir::new().unwrap();
        std::fs::create_dir_all(sd_dir.path().join("RADIO")).unwrap();

        let result = update(UpdateOptions {
            sd_root: sd_dir.path().to_path_buf(),
            query: String::new(),
            all: false,
            dev: None,
            dry_run: false,
            before_copy: None,
            on_file: None,
        });
        assert!(result.is_err());
        assert!(
            result.unwrap_err().to_string().contains("--all"),
            "error should mention --all"
        );
    }
}
