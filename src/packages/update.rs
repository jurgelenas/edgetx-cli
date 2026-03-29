use anyhow::Result;
use std::path::{Path, PathBuf};

use crate::manifest::{self, Manifest};
use crate::packages::path::PackagePath;
use crate::radio;
use crate::source::version::Channel;
use crate::source::{PackageRef, resolve};

use super::conflict::check_conflicts;
use super::install::{build_exclude, count_install_files, remove_tracked_files};
use super::state::{self, InstalledPackage, State};

pub type BeforeCopyFn = Box<dyn Fn(&str, usize)>;
pub type OnFileFn = Box<dyn Fn(&str)>;

/// UpdateOptions configures an update operation.
pub struct UpdateOptions {
    pub sd_root: PathBuf,
    pub query: String,
    pub all: bool,
    pub dev: bool,
    pub dev_set: bool,
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
        state: &State,
        version_override: &str,
        include_dev: bool,
    ) -> Result<UpdateCommand> {
        // Pinned commits can't be updated without explicit version
        if pkg.channel == Channel::Commit && version_override.is_empty() {
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

        let (m, manifest_dir, new_channel, new_version, new_commit) = if pkg.channel
            == Channel::Local
        {
            // Re-copy from local path
            let pkg_ref: PackageRef = pkg.source.parse().map_err(|e| anyhow::anyhow!("{e}"))?;

            let (local_path, sub_path) = match &pkg_ref {
                PackageRef::Local { path, sub_path } => (path.clone(), sub_path.clone()),
                _ => anyhow::bail!("expected local package for channel=local"),
            };

            let (m, mdir) = manifest::load_with_sub_path(&local_path, &sub_path)?;
            (m, mdir, Channel::Local, String::new(), String::new())
        } else {
            let mut pkg_ref: PackageRef = pkg
                .source
                .parse()
                .map_err(|e| anyhow::anyhow!("parsing source {:?}: {e}", pkg.source))?;

            if !version_override.is_empty() {
                pkg_ref.set_version(version_override.to_string());
            } else if pkg.channel == Channel::Branch {
                pkg_ref.set_version(pkg.version.clone());
            }
            // tag channel with no override: leave version empty to get latest

            let result = resolve::resolve_package(&pkg_ref).map_err(|e| anyhow::anyhow!("{e}"))?;

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
        check_conflicts(state, &new_paths, original_source).map_err(|e| anyhow::anyhow!("{e}"))?;

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
        count_install_files(&self.manifest_dir, &self.manifest, self.include_dev)
    }

    /// Execute copies the files and updates the state.
    pub fn execute(
        self,
        sd_root: &Path,
        state: &mut State,
        dry_run: bool,
        mut on_file: impl FnMut(&str),
    ) -> Result<UpdateResult> {
        if self.up_to_date {
            return Ok(UpdateResult {
                package: self.package,
                files_copied: 0,
                up_to_date: true,
            });
        }

        let new_paths = self.manifest.all_paths(self.include_dev);

        if !dry_run {
            remove_tracked_files(sd_root, &self.old_package.name);
            state.remove(&self.original_source);

            let mut total_copied = 0;
            let mut copied_files: Vec<PackagePath> = Vec::new();
            for item in self.manifest.content_items(self.include_dev) {
                let source_root = self
                    .manifest
                    .resolve_content_path(&self.manifest_dir, &item.path)
                    .map_err(|e| anyhow::anyhow!("resolving {}: {e}", item.path))?;
                let exclude = build_exclude(self.manifest.package.binary, &item);
                let copied_ref = std::cell::RefCell::new(&mut copied_files);
                let on_file_ref = std::cell::RefCell::new(&mut on_file);
                let n = radio::copy::copy_paths(
                    &source_root,
                    sd_root,
                    &[item.path.as_str()],
                    &radio::copy::CopyOptions {
                        dry_run: false,
                        exclude: &exclude,
                        on_file: Some(&|dest: &Path| {
                            if let Ok(rel) = dest.strip_prefix(sd_root) {
                                copied_ref
                                    .borrow_mut()
                                    .push(PackagePath::new(rel.to_string_lossy()));
                            }
                            (on_file_ref.borrow_mut())(&dest.display().to_string());
                        }),
                    },
                )?;
                total_copied += n;
            }

            let updated = InstalledPackage {
                source: self.old_package.source.clone(),
                name: self.manifest.package.name.clone(),
                channel: self.package.channel,
                version: self.package.version.clone(),
                commit: self.package.commit.clone(),
                paths: new_paths,
                dev: self.include_dev,
            };
            state.add(updated.clone());
            state
                .save(sd_root)
                .map_err(|e| anyhow::anyhow!("saving state: {e}"))?;
            state::save_file_list(sd_root, &updated.name, &copied_files)
                .map_err(|e| anyhow::anyhow!("saving file list: {e}"))?;

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
pub fn update(opts: UpdateOptions) -> Result<Vec<UpdateResult>> {
    if opts.query.is_empty() && !opts.all {
        anyhow::bail!("specify a package name or use --all");
    }

    let mut state = state::load_state(&opts.sd_root).map_err(|e| anyhow::anyhow!("{e}"))?;

    let targets: Vec<InstalledPackage>;
    let original_sources: Vec<String>;
    let mut version_override = String::new();

    if opts.all {
        targets = state.packages.clone();
        original_sources = targets.iter().map(|t| t.source.clone()).collect();
    } else {
        let pkg_ref: PackageRef = opts.query.parse().map_err(|e| anyhow::anyhow!("{e}"))?;
        let query = pkg_ref.canonical();
        version_override = pkg_ref.version().to_string();

        match state.find(&query) {
            Ok(pkg) => {
                original_sources = vec![pkg.source.clone()];
                targets = vec![pkg.clone()];
            }
            Err(_) => {
                // Try parsing as remote ref to discover manifest name
                anyhow::bail!("package {:?} not found", opts.query);
            }
        }
    }

    let mut results = Vec::new();
    for (i, pkg) in targets.iter().enumerate() {
        let include_dev = if opts.dev_set { opts.dev } else { pkg.dev };
        let cmd = UpdateCommand::resolve(
            pkg,
            &original_sources[i],
            &state,
            &version_override,
            include_dev,
        )?;

        if let Some(cb) = &opts.before_copy {
            cb(&cmd.package.name, cmd.total_files());
        }

        let result = cmd.execute(&opts.sd_root, &mut state, opts.dry_run, |f| {
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
        let state = State {
            packages: vec![pkg.clone()],
        };
        state.save(sd_dir.path()).unwrap();
        state::save_file_list(
            sd_dir.path(),
            "test-pkg",
            &files
                .iter()
                .map(|(p, _)| PackagePath::from(*p))
                .collect::<Vec<_>>(),
        )
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
            dev: false,
            dev_set: false,
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

        let state = State {
            packages: vec![InstalledPackage {
                source: "Org/Repo".into(),
                name: "pinned-pkg".into(),
                channel: Channel::Commit,
                version: "abc123".into(),
                commit: "abc123".into(),
                paths: vec![],
                dev: false,
            }],
        };
        state.save(sd_dir.path()).unwrap();

        let results = update(UpdateOptions {
            sd_root: sd_dir.path().to_path_buf(),
            query: "Org/Repo".into(),
            all: false,
            dev: false,
            dev_set: false,
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

        let state = State { packages: vec![] };
        state.save(sd_dir.path()).unwrap();

        let result = update(UpdateOptions {
            sd_root: sd_dir.path().to_path_buf(),
            query: "NonExistent/Repo".into(),
            all: false,
            dev: false,
            dev_set: false,
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
            dev: false,
            dev_set: false,
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
