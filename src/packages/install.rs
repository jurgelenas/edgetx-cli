use anyhow::Result;
use std::path::{Path, PathBuf};

use crate::manifest::{self, ContentItem, Manifest};
use crate::radio;
use crate::source::{PackageRef, resolve};

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
    pub fn resolve(opts: InstallOptions) -> Result<InstallCommand> {
        let mut state = state::load_state(&opts.sd_root)
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        let canonical = opts.pkg_ref.canonical();

        let (m, manifest_dir, channel, version, commit) = match &opts.pkg_ref {
            PackageRef::Local { path, sub_path } => {
                let (m, mdir) = manifest::load_with_sub_path(path, sub_path)?;
                (m, mdir, "local".to_string(), String::new(), String::new())
            }
            PackageRef::Remote { .. } => {
                let result = resolve::resolve_package(&opts.pkg_ref)
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
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
        check_conflicts(&state, &paths, "")
            .map_err(|e| anyhow::anyhow!("{e}"))?;

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
    ) -> Result<InstallResult> {
        let mut total_copied = 0;
        let mut copied_files = Vec::new();
        let mut state = self.state;

        if !dry_run {
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
                    &[&item.path],
                    &radio::copy::CopyOptions {
                        dry_run: false,
                        exclude: &exclude,
                        on_file: Some(&|dest: &Path| {
                            if let Ok(rel) = dest.strip_prefix(sd_root) {
                                copied_ref.borrow_mut().push(rel.to_string_lossy().to_string());
                            }
                            (on_file_ref.borrow_mut())(&dest.display().to_string());
                        }),
                    },
                )?;
                total_copied += n;
            }

            state.add(self.package.clone());
            state
                .save(sd_root)
                .map_err(|e| anyhow::anyhow!("saving state: {e}"))?;

            state::save_file_list(sd_root, &self.package.name, &copied_files)
                .map_err(|e| anyhow::anyhow!("saving file list: {e}"))?;
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
            total += radio::copy::count_files(&source_root, &[&item.path], &exclude);
        }
    }
    total
}


/// Remove files installed by a package using the tracked file list.
pub(crate) fn remove_tracked_files(sd_root: &Path, name: &str) {
    let files = state::load_file_list(sd_root, name);
    for f in &files {
        let _ = std::fs::remove_file(sd_root.join(f));
    }
    for f in &files {
        clean_empty_parents(sd_root, f);
    }
    state::remove_file_list(sd_root, name);
}

/// Remove empty parent directories up to the SD root.
pub(crate) fn clean_empty_parents(sd_root: &Path, rel_path: &str) {
    let parts: Vec<&str> = rel_path.split('/').collect();
    for i in (1..parts.len()).rev() {
        let parent = sd_root.join(parts[..i].join("/"));
        match std::fs::read_dir(&parent) {
            Ok(mut entries) => {
                if entries.next().is_some() {
                    break;
                }
                let _ = std::fs::remove_dir(&parent);
            }
            Err(_) => break,
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
        assert_eq!(cmd.package.channel, "local");

        let result = cmd
            .execute(sd_dir.path(), false, |_| {})
            .unwrap();
        assert_eq!(result.files_copied, 1);

        // Verify state
        let state = state::load_state(sd_dir.path()).unwrap();
        assert_eq!(state.packages.len(), 1);
        assert_eq!(state.packages[0].name, "test-pkg");

        // Verify file was copied
        assert!(sd_dir
            .path()
            .join("SCRIPTS/TOOLS/MyTool/main.lua")
            .exists());
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

        let result = cmd
            .execute(sd_dir.path(), true, |_| {})
            .unwrap();
        assert_eq!(result.files_copied, 0);

        // Verify no state was saved
        let state = state::load_state(sd_dir.path()).unwrap();
        assert!(state.packages.is_empty());
    }
}
