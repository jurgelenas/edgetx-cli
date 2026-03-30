use std::path::{Path, PathBuf};

use crate::manifest::{self, ContentItem, Manifest};
use crate::packages::path::PackagePath;
use crate::radio;
use crate::source::version::Channel;
use crate::source::{PackageRef, resolve};

use super::PackageError;
use super::conflict::check_conflicts;
use super::state::{self, InstalledPackage, State};

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
    state: State,
}

impl InstallCommand {
    /// Resolve the package ref, load the manifest, check for conflicts.
    pub fn resolve(opts: InstallOptions) -> Result<InstallCommand, PackageError> {
        let mut state = state::load_state(&opts.sd_root)?;

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
            if let Some(info) = radio::radioinfo::load_radio_info(&opts.sd_root)? {
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
        let existing_source = if state.find_by_source(&canonical).is_some() {
            Some(canonical.clone())
        } else {
            let by_name = state.find_by_name(&m.package.name);
            if by_name.len() == 1 {
                Some(by_name[0].source.clone())
            } else {
                None
            }
        };

        if let Some(src) = existing_source {
            // Find the name before removing
            if let Some(existing) = state.find_by_source(&src) {
                remove_tracked_files(&opts.sd_root, &existing.name);
            }
            state.remove(&src);
        }

        let paths = m.all_paths(opts.dev);
        check_conflicts(&state, &paths, "")?;

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
            state,
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
        dry_run: bool,
        mut on_file: impl FnMut(&str),
    ) -> Result<InstallResult, PackageError> {
        let mut total_copied = 0;
        let mut copied_files = Vec::new();
        let mut state = self.state;

        if !dry_run {
            for item in self.manifest.content_items(self.include_dev) {
                let source_root = self
                    .manifest
                    .resolve_content_path(&self.manifest_dir, &item.path)
                    .map_err(|e| PackageError::ContentResolve {
                        path: item.path.clone(),
                        source: e,
                    })?;

                let exclude = build_exclude(self.manifest.package.binary, &item);
                let n = radio::copy::copy_paths(
                    &source_root,
                    sd_root,
                    &[item.path.as_str()],
                    &radio::copy::CopyOptions {
                        dry_run: false,
                        exclude: &exclude,
                    },
                    &mut |dest: &Path| {
                        if let Ok(rel) = dest.strip_prefix(sd_root) {
                            copied_files.push(PackagePath::new(rel.to_string_lossy()));
                        }
                        on_file(&dest.display().to_string());
                    },
                )?;
                total_copied += n;
            }

            state.add(self.package.clone());
            state.save(sd_root)?;

            // Track content directories for cleanup on removal
            for item in self.manifest.content_items(self.include_dev) {
                copied_files.push(PackagePath::new(format!("{}/", item.path)));
            }

            state::save_file_list(sd_root, &self.package.name, &copied_files)?;
        }

        Ok(InstallResult {
            package: self.package,
            files_copied: total_copied,
        })
    }
}

/// Build exclude patterns for a content item.
pub(crate) fn build_exclude(binary: bool, item: &ContentItem) -> Vec<String> {
    if binary {
        item.exclude.clone()
    } else {
        let mut excludes = radio::copy::DEFAULT_EXCLUDE
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>();
        excludes.extend(item.exclude.iter().cloned());
        excludes
    }
}

/// Count the total number of files that would be copied.
pub fn count_install_files(manifest_dir: &Path, m: &Manifest, include_dev: bool) -> usize {
    let mut total = 0;
    for item in m.content_items(include_dev) {
        if let Ok(source_root) = m.resolve_content_path(manifest_dir, &item.path) {
            let exclude = build_exclude(m.package.binary, &item);
            total += radio::copy::count_files(&source_root, &[item.path.as_str()], &exclude);
        }
    }
    total
}

/// Remove files installed by a package using the tracked file list.
pub(crate) fn remove_tracked_files(sd_root: &Path, name: &str) {
    let entries = state::load_file_list(sd_root, name);

    // Delete file entries + .luac companions
    for f in entries.iter().filter(|e| !e.as_str().ends_with('/')) {
        let _ = std::fs::remove_file(sd_root.join(f.as_str()));
        if f.as_str().ends_with(".lua") {
            let _ = std::fs::remove_file(sd_root.join(format!("{f}c")));
        }
    }

    // Remove tracked directories (deepest first handled by remove_empty_tree)
    for d in entries.iter().filter(|e| e.as_str().ends_with('/')) {
        remove_empty_tree(sd_root, d.as_str().trim_end_matches('/'));
    }

    state::remove_file_list(sd_root, name);
}

/// Remove empty subdirectories within a tracked directory, bottom-up.
/// Removes the directory itself if it ends up empty.
/// Never walks above the given directory.
pub(crate) fn remove_empty_tree(sd_root: &Path, rel_dir: &str) {
    let root = sd_root.join(rel_dir);
    if !root.is_dir() {
        return;
    }

    // Collect all subdirectories, then sort deepest first
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

        let cmd = InstallCommand::resolve(InstallOptions {
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

        let result = cmd.execute(sd_dir.path(), false, |_| {}).unwrap();
        assert_eq!(result.files_copied, 1);

        // Verify state
        let state = state::load_state(sd_dir.path()).unwrap();
        assert_eq!(state.packages.len(), 1);
        assert_eq!(state.packages[0].name, "test-pkg");

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

        let cmd = InstallCommand::resolve(InstallOptions {
            sd_root: sd_dir.path().to_path_buf(),
            pkg_ref: PackageRef::Local {
                path: pkg_dir.path().to_path_buf(),
                sub_path: String::new(),
            },
            dev: false,
        })
        .unwrap();

        cmd.execute(sd_dir.path(), false, |_| {}).unwrap();

        let entries = state::load_file_list(sd_dir.path(), "test-pkg");
        assert!(entries.iter().any(|e| e == "SCRIPTS/TOOLS/MyTool/"));
        assert!(entries.iter().any(|e| e == "SCRIPTS/TOOLS/MyTool/main.lua"));
    }

    #[test]
    fn test_remove_empty_tree() {
        let dir = TempDir::new().unwrap();
        let sd = dir.path();

        // Create nested structure
        std::fs::create_dir_all(sd.join("SCRIPTS/TOOLS/MyTool/sub")).unwrap();

        // All empty — should be fully removed
        remove_empty_tree(sd, "SCRIPTS/TOOLS/MyTool");
        assert!(!sd.join("SCRIPTS/TOOLS/MyTool").exists());
        // Parent should still exist
        assert!(sd.join("SCRIPTS/TOOLS").exists());
    }

    #[test]
    fn test_remove_empty_tree_deeply_nested() {
        let dir = TempDir::new().unwrap();
        let sd = dir.path();

        std::fs::create_dir_all(sd.join("SCRIPTS/TOOLS/MyTool/lib/utils/deep")).unwrap();

        remove_empty_tree(sd, "SCRIPTS/TOOLS/MyTool");
        assert!(!sd.join("SCRIPTS/TOOLS/MyTool").exists());
        assert!(sd.join("SCRIPTS/TOOLS").exists());
    }

    #[test]
    fn test_remove_empty_tree_keeps_nonempty() {
        let dir = TempDir::new().unwrap();
        let sd = dir.path();

        std::fs::create_dir_all(sd.join("SCRIPTS/TOOLS/MyTool/sub")).unwrap();
        std::fs::write(sd.join("SCRIPTS/TOOLS/MyTool/keep.txt"), "data").unwrap();

        remove_empty_tree(sd, "SCRIPTS/TOOLS/MyTool");
        // MyTool kept because it has a file
        assert!(sd.join("SCRIPTS/TOOLS/MyTool").exists());
        // Empty sub should be removed
        assert!(!sd.join("SCRIPTS/TOOLS/MyTool/sub").exists());
    }

    #[test]
    fn test_remove_tracked_files_with_luac() {
        let dir = TempDir::new().unwrap();
        let sd = dir.path();

        std::fs::create_dir_all(sd.join("RADIO/packages")).unwrap();
        std::fs::create_dir_all(sd.join("SCRIPTS/TOOLS/MyTool")).unwrap();
        std::fs::write(sd.join("SCRIPTS/TOOLS/MyTool/main.lua"), "-- lua").unwrap();
        std::fs::write(sd.join("SCRIPTS/TOOLS/MyTool/main.luac"), "bytecode").unwrap();

        state::save_file_list(
            sd,
            "test-pkg",
            &[
                "SCRIPTS/TOOLS/MyTool/main.lua".into(),
                "SCRIPTS/TOOLS/MyTool/".into(),
            ],
        )
        .unwrap();

        remove_tracked_files(sd, "test-pkg");

        assert!(!sd.join("SCRIPTS/TOOLS/MyTool/main.lua").exists());
        assert!(!sd.join("SCRIPTS/TOOLS/MyTool/main.luac").exists());
        assert!(!sd.join("SCRIPTS/TOOLS/MyTool").exists());
        assert!(sd.join("SCRIPTS/TOOLS").exists());
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

        let cmd = InstallCommand::resolve(InstallOptions {
            sd_root: sd_dir.path().to_path_buf(),
            pkg_ref: PackageRef::Local {
                path: pkg_dir.path().to_path_buf(),
                sub_path: String::new(),
            },
            dev: false,
        })
        .unwrap();

        let result = cmd.execute(sd_dir.path(), true, |_| {}).unwrap();
        assert_eq!(result.files_copied, 0);

        // Verify no state was saved
        let state = state::load_state(sd_dir.path()).unwrap();
        assert!(state.packages.is_empty());
    }
}
