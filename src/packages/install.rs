use std::path::PathBuf;

use crate::manifest::{self, Manifest, RadioCapabilities};
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
    /// Detected radio hardware capabilities. None if unknown.
    pub radio: Option<RadioCapabilities>,
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
    existing_id: Option<String>,
    store: PackageStore,
}

impl InstallCommand {
    /// Create a new install command by resolving the package ref, loading the manifest, and checking for conflicts.
    pub fn new(opts: InstallOptions) -> Result<InstallCommand, PackageError> {
        let store = PackageStore::load(opts.sd_root)?;

        let is_local = opts.pkg_ref.is_local();
        let fetch_canonical = opts.pkg_ref.canonical();
        let explicit_variant = opts.pkg_ref.variant().to_string();
        let local_path_of_ref = match &opts.pkg_ref {
            PackageRef::Local { path, .. } => Some(path.clone()),
            _ => None,
        };

        let (mut m, mut manifest_dir, channel, version, commit) = match &opts.pkg_ref {
            PackageRef::Local { path, variant } => {
                let (m, mdir) = manifest::load_with_sub_path(path, variant)
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

        // Variant selection: auto or manual via --path / ::variant
        let mut selected_variant: Option<String> = None;
        if m.has_variants() {
            let variant_filename = if !explicit_variant.is_empty() {
                // Manual override: look up variant by filename
                match m
                    .package
                    .variants
                    .iter()
                    .find(|v| v.path == explicit_variant)
                {
                    Some(v) => v.path.clone(),
                    None => {
                        return Err(PackageError::Source(
                            crate::source::SourceError::InvalidRef {
                                raw: explicit_variant.clone(),
                                reason: format!(
                                    "variant {:?} not found in manifest variants list",
                                    explicit_variant
                                ),
                            },
                        ));
                    }
                }
            } else if let Some(ref radio) = opts.radio {
                match m.select_variant(radio) {
                    Some(variant) => variant.path.clone(),
                    None => return Err(PackageError::NoMatchingVariant),
                }
            } else {
                return Err(PackageError::UnknownRadio);
            };

            log::info!("selected variant: {variant_filename}");
            let (vm, vdir) = manifest::load_with_sub_path(&manifest_dir, &variant_filename)
                .map_err(|e| PackageError::Source(e.into()))?;
            m = vm;
            manifest_dir = vdir;
            selected_variant = Some(variant_filename);
        }

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

        let id = m.package.id.clone();

        let origin = if !is_local && id != fetch_canonical {
            log::warn!("manifest declares id={id:?} but fetched from {fetch_canonical:?}");
            Some(fetch_canonical.clone())
        } else {
            None
        };

        let existing_id = if store.find_by_id(&id).is_some() {
            Some(id.clone())
        } else {
            None
        };

        let paths = m.all_paths(opts.dev);
        store.check_conflicts(&paths, existing_id.as_deref())?;

        Ok(InstallCommand {
            manifest: m.clone(),
            manifest_dir,
            package: InstalledPackage {
                id,
                name: m.package.name.clone(),
                channel,
                version,
                commit,
                origin,
                variant: selected_variant,
                local_path: local_path_of_ref,
                paths,
                dev: opts.dev,
            },
            include_dev: opts.dev,
            existing_id,
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
            if let Some(id) = &self.existing_id {
                store.remove(id);
            }

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
                if sd_root.join(item.path.as_str()).is_dir() {
                    copied_files.push(PackagePath::new(format!("{}/", item.path)));
                }
            }

            PackageFileList::new(self.package.id.clone(), copied_files)
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
  id: example.com/test/test-pkg
  description: "Test"
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
                variant: String::new(),
            },
            dev: false,
            radio: None,
        })
        .unwrap();

        assert_eq!(cmd.package.id, "example.com/test/test-pkg");
        assert_eq!(cmd.package.channel, Channel::Local);

        let result = cmd.execute(false, |_| {}).unwrap();
        assert_eq!(result.files_copied, 1);

        // Verify state
        let store = PackageStore::load(sd_dir.path().to_path_buf()).unwrap();
        assert_eq!(store.packages().len(), 1);
        assert_eq!(store.packages()[0].id, "example.com/test/test-pkg");

        // Verify file was copied
        assert!(sd_dir.path().join("SCRIPTS/TOOLS/MyTool/main.lua").exists());
    }

    #[test]
    fn test_install_saves_directory_entries() {
        let (pkg_dir, sd_dir) = setup_local_package(
            r#"
package:
  id: example.com/test/test-pkg
  description: "Test"
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
                variant: String::new(),
            },
            dev: false,
            radio: None,
        })
        .unwrap();

        cmd.execute(false, |_| {}).unwrap();

        let store = PackageStore::load(sd_dir.path().to_path_buf()).unwrap();
        let file_list = PackageFileList::load(&store.file_list_dir, "example.com/test/test-pkg");
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
  id: example.com/test/test-pkg
  description: "Test"
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
                variant: String::new(),
            },
            dev: false,
            radio: None,
        })
        .unwrap();

        let result = cmd.execute(true, |_| {}).unwrap();
        assert_eq!(result.files_copied, 0);

        // Verify no state was saved
        let store = PackageStore::load(sd_dir.path().to_path_buf()).unwrap();
        assert!(store.packages().is_empty());
    }

    #[test]
    fn test_reinstall_same_source() {
        let (pkg_dir, sd_dir) = setup_local_package(
            r#"
package:
  id: example.com/test/test-pkg
  description: "Test"
tools:
  - name: MyTool
    path: SCRIPTS/TOOLS/MyTool
"#,
            &["SCRIPTS/TOOLS/MyTool/main.lua"],
        );

        // First install
        let cmd = InstallCommand::new(InstallOptions {
            sd_root: sd_dir.path().to_path_buf(),
            pkg_ref: PackageRef::Local {
                path: pkg_dir.path().to_path_buf(),
                variant: String::new(),
            },
            dev: false,
            radio: None,
        })
        .unwrap();
        cmd.execute(false, |_| {}).unwrap();

        // Reinstall same source should succeed
        let cmd = InstallCommand::new(InstallOptions {
            sd_root: sd_dir.path().to_path_buf(),
            pkg_ref: PackageRef::Local {
                path: pkg_dir.path().to_path_buf(),
                variant: String::new(),
            },
            dev: false,
            radio: None,
        })
        .unwrap();
        let result = cmd.execute(false, |_| {}).unwrap();
        assert_eq!(result.files_copied, 1);

        let store = PackageStore::load(sd_dir.path().to_path_buf()).unwrap();
        assert_eq!(store.packages().len(), 1);
    }

    #[test]
    fn test_reinstall_does_not_mutate_in_new() {
        let (pkg_dir, sd_dir) = setup_local_package(
            r#"
package:
  id: example.com/test/test-pkg
  description: "Test"
tools:
  - name: MyTool
    path: SCRIPTS/TOOLS/MyTool
"#,
            &["SCRIPTS/TOOLS/MyTool/main.lua"],
        );

        // First install
        let cmd = InstallCommand::new(InstallOptions {
            sd_root: sd_dir.path().to_path_buf(),
            pkg_ref: PackageRef::Local {
                path: pkg_dir.path().to_path_buf(),
                variant: String::new(),
            },
            dev: false,
            radio: None,
        })
        .unwrap();
        cmd.execute(false, |_| {}).unwrap();
        assert!(sd_dir.path().join("SCRIPTS/TOOLS/MyTool/main.lua").exists());

        // Call new() but don't execute — files should still be on disk
        let _cmd = InstallCommand::new(InstallOptions {
            sd_root: sd_dir.path().to_path_buf(),
            pkg_ref: PackageRef::Local {
                path: pkg_dir.path().to_path_buf(),
                variant: String::new(),
            },
            dev: false,
            radio: None,
        })
        .unwrap();

        // Files must still exist since we only called new(), not execute()
        assert!(sd_dir.path().join("SCRIPTS/TOOLS/MyTool/main.lua").exists());
    }

    #[test]
    fn test_different_id_overlapping_paths_conflicts() {
        // Two packages with different ids but the same install path should conflict.
        let (pkg_dir_a, sd_dir) = setup_local_package(
            r#"
package:
  id: example.com/test/pkg-a
  description: "Test"
tools:
  - name: MyTool
    path: SCRIPTS/TOOLS/MyTool
"#,
            &["SCRIPTS/TOOLS/MyTool/main.lua"],
        );

        let cmd = InstallCommand::new(InstallOptions {
            sd_root: sd_dir.path().to_path_buf(),
            pkg_ref: PackageRef::Local {
                path: pkg_dir_a.path().to_path_buf(),
                variant: String::new(),
            },
            dev: false,
            radio: None,
        })
        .unwrap();
        cmd.execute(false, |_| {}).unwrap();

        // Package B: different id, overlapping path → should conflict
        let (pkg_dir_b, _) = setup_local_package(
            r#"
package:
  id: example.com/test/pkg-b
  description: "Test"
tools:
  - name: MyTool
    path: SCRIPTS/TOOLS/MyTool
"#,
            &["SCRIPTS/TOOLS/MyTool/main.lua"],
        );

        let result = InstallCommand::new(InstallOptions {
            sd_root: sd_dir.path().to_path_buf(),
            pkg_ref: PackageRef::Local {
                path: pkg_dir_b.path().to_path_buf(),
                variant: String::new(),
            },
            dev: false,
            radio: None,
        });
        assert!(result.is_err());
    }

    #[test]
    fn test_same_id_reinstall_replaces() {
        // Installing the same id from a different source location replaces the entry.
        let (pkg_dir_a, sd_dir) = setup_local_package(
            r#"
package:
  id: example.com/test/same-name
  description: "Test"
tools:
  - name: MyTool
    path: SCRIPTS/TOOLS/MyTool
"#,
            &["SCRIPTS/TOOLS/MyTool/main.lua"],
        );

        let cmd = InstallCommand::new(InstallOptions {
            sd_root: sd_dir.path().to_path_buf(),
            pkg_ref: PackageRef::Local {
                path: pkg_dir_a.path().to_path_buf(),
                variant: String::new(),
            },
            dev: false,
            radio: None,
        })
        .unwrap();
        cmd.execute(false, |_| {}).unwrap();

        // Same id from different dir — should succeed (replaces)
        let (pkg_dir_b, _) = setup_local_package(
            r#"
package:
  id: example.com/test/same-name
  description: "Test"
tools:
  - name: MyTool
    path: SCRIPTS/TOOLS/MyTool
"#,
            &["SCRIPTS/TOOLS/MyTool/main.lua"],
        );

        let result = InstallCommand::new(InstallOptions {
            sd_root: sd_dir.path().to_path_buf(),
            pkg_ref: PackageRef::Local {
                path: pkg_dir_b.path().to_path_buf(),
                variant: String::new(),
            },
            dev: false,
            radio: None,
        });
        assert!(result.is_ok());
    }

    #[test]
    fn test_same_name_different_source_no_overlap_conflicts() {
        let (pkg_dir_a, sd_dir) = setup_local_package(
            r#"
package:
  id: example.com/test/same-name
  description: "Test"
tools:
  - name: ToolA
    path: SCRIPTS/TOOLS/ToolA
"#,
            &["SCRIPTS/TOOLS/ToolA/main.lua"],
        );

        // Install package A
        let cmd = InstallCommand::new(InstallOptions {
            sd_root: sd_dir.path().to_path_buf(),
            pkg_ref: PackageRef::Local {
                path: pkg_dir_a.path().to_path_buf(),
                variant: String::new(),
            },
            dev: false,
            radio: None,
        })
        .unwrap();
        cmd.execute(false, |_| {}).unwrap();

        // Package B: same name, different source, different path — no conflict
        let (pkg_dir_b, _) = setup_local_package(
            r#"
package:
  id: example.com/test/same-name
  description: "Test"
tools:
  - name: ToolB
    path: SCRIPTS/TOOLS/ToolB
"#,
            &["SCRIPTS/TOOLS/ToolB/main.lua"],
        );

        // Same id (example.com/test/same-name) now means install B replaces A.
        // That's allowed even though paths differ, because id is the identity.
        let result = InstallCommand::new(InstallOptions {
            sd_root: sd_dir.path().to_path_buf(),
            pkg_ref: PackageRef::Local {
                path: pkg_dir_b.path().to_path_buf(),
                variant: String::new(),
            },
            dev: false,
            radio: None,
        });
        assert!(result.is_ok());
    }

    #[test]
    fn test_bare_file_no_directory_entry() {
        let (pkg_dir, sd_dir) = setup_local_package(
            r#"
package:
  id: example.com/test/bare-tool
  description: "Test"
tools:
  - name: MyTool
    path: SCRIPTS/TOOLS/tool.lua
"#,
            &["SCRIPTS/TOOLS/tool.lua"],
        );

        let cmd = InstallCommand::new(InstallOptions {
            sd_root: sd_dir.path().to_path_buf(),
            pkg_ref: PackageRef::Local {
                path: pkg_dir.path().to_path_buf(),
                variant: String::new(),
            },
            dev: false,
            radio: None,
        })
        .unwrap();
        cmd.execute(false, |_| {}).unwrap();

        let store = PackageStore::load(sd_dir.path().to_path_buf()).unwrap();
        let file_list = PackageFileList::load(&store.file_list_dir, "example.com/test/bare-tool");

        // Should have the file but NOT a bogus "SCRIPTS/TOOLS/tool.lua/" directory entry
        assert!(
            file_list
                .files()
                .iter()
                .any(|e| e == "SCRIPTS/TOOLS/tool.lua")
        );
        assert!(
            !file_list
                .files()
                .iter()
                .any(|e| e == "SCRIPTS/TOOLS/tool.lua/")
        );
    }

    /// Create a local package with a router manifest and variant manifests.
    /// Returns (pkg_dir, sd_dir).
    fn setup_variant_package() -> (TempDir, TempDir) {
        let pkg_dir = TempDir::new().unwrap();

        // Router manifest - no content, just variants
        std::fs::write(
            pkg_dir.path().join("edgetx.yml"),
            r#"
package:
  id: example.com/test/multi-variant
  description: "Test"
  variants:
    - path: edgetx.bw128x64.yml
      capabilities:
        display:
          type: bw
          resolution: 128x64
    - path: edgetx.bw.yml
      capabilities:
        display:
          type: bw
    - path: edgetx.color.yml
      capabilities:
        display:
          type: colorlcd
"#,
        )
        .unwrap();

        // BW 128x64 variant
        std::fs::write(
            pkg_dir.path().join("edgetx.bw128x64.yml"),
            r#"
package:
  id: example.com/test/multi-variant
  description: "Test"
tools:
  - name: BWTool128
    path: SCRIPTS/TOOLS/BWTool128
"#,
        )
        .unwrap();
        let p = pkg_dir.path().join("SCRIPTS/TOOLS/BWTool128/main.lua");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, "-- bw 128x64").unwrap();

        // Generic BW variant
        std::fs::write(
            pkg_dir.path().join("edgetx.bw.yml"),
            r#"
package:
  id: example.com/test/multi-variant
  description: "Test"
tools:
  - name: BWToolGeneric
    path: SCRIPTS/TOOLS/BWToolGeneric
"#,
        )
        .unwrap();
        let p = pkg_dir.path().join("SCRIPTS/TOOLS/BWToolGeneric/main.lua");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, "-- bw generic").unwrap();

        // Color variant
        std::fs::write(
            pkg_dir.path().join("edgetx.color.yml"),
            r#"
package:
  id: example.com/test/multi-variant
  description: "Test"
widgets:
  - name: ColorWidget
    path: WIDGETS/ColorWidget
"#,
        )
        .unwrap();
        let p = pkg_dir.path().join("WIDGETS/ColorWidget/main.lua");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, "-- color widget").unwrap();

        let sd_dir = TempDir::new().unwrap();
        std::fs::create_dir_all(sd_dir.path().join("RADIO")).unwrap();

        (pkg_dir, sd_dir)
    }

    use crate::manifest::{DisplayCapabilities, RadioCapabilities};

    #[test]
    fn test_install_variant_selects_color() {
        let (pkg_dir, sd_dir) = setup_variant_package();

        let cmd = InstallCommand::new(InstallOptions {
            sd_root: sd_dir.path().to_path_buf(),
            pkg_ref: PackageRef::Local {
                path: pkg_dir.path().to_path_buf(),
                variant: String::new(),
            },
            dev: false,
            radio: Some(RadioCapabilities {
                display: DisplayCapabilities {
                    width: 480,
                    height: 272,
                    color: true,
                    touch: None,
                },
            }),
        })
        .unwrap();

        let result = cmd.execute(false, |_| {}).unwrap();
        assert!(result.files_copied > 0);

        // Color variant's widget should be installed
        assert!(sd_dir.path().join("WIDGETS/ColorWidget/main.lua").exists());
        // BW variant's tool should NOT be installed
        assert!(
            !sd_dir
                .path()
                .join("SCRIPTS/TOOLS/BWTool128/main.lua")
                .exists()
        );
        assert!(
            !sd_dir
                .path()
                .join("SCRIPTS/TOOLS/BWToolGeneric/main.lua")
                .exists()
        );
    }

    #[test]
    fn test_install_variant_selects_bw() {
        let (pkg_dir, sd_dir) = setup_variant_package();

        let cmd = InstallCommand::new(InstallOptions {
            sd_root: sd_dir.path().to_path_buf(),
            pkg_ref: PackageRef::Local {
                path: pkg_dir.path().to_path_buf(),
                variant: String::new(),
            },
            dev: false,
            radio: Some(RadioCapabilities {
                display: DisplayCapabilities {
                    width: 212,
                    height: 64,
                    color: false,
                    touch: None,
                },
            }),
        })
        .unwrap();

        let result = cmd.execute(false, |_| {}).unwrap();
        assert!(result.files_copied > 0);

        // Generic BW variant should be selected (212x64 doesn't match 128x64 specific)
        assert!(
            sd_dir
                .path()
                .join("SCRIPTS/TOOLS/BWToolGeneric/main.lua")
                .exists()
        );
        assert!(!sd_dir.path().join("WIDGETS/ColorWidget/main.lua").exists());
        assert!(
            !sd_dir
                .path()
                .join("SCRIPTS/TOOLS/BWTool128/main.lua")
                .exists()
        );
    }

    #[test]
    fn test_install_variant_prefers_specific_resolution() {
        let (pkg_dir, sd_dir) = setup_variant_package();

        let cmd = InstallCommand::new(InstallOptions {
            sd_root: sd_dir.path().to_path_buf(),
            pkg_ref: PackageRef::Local {
                path: pkg_dir.path().to_path_buf(),
                variant: String::new(),
            },
            dev: false,
            radio: Some(RadioCapabilities {
                display: DisplayCapabilities {
                    width: 128,
                    height: 64,
                    color: false,
                    touch: None,
                },
            }),
        })
        .unwrap();

        let result = cmd.execute(false, |_| {}).unwrap();
        assert!(result.files_copied > 0);

        // Specific 128x64 variant should win over generic BW
        assert!(
            sd_dir
                .path()
                .join("SCRIPTS/TOOLS/BWTool128/main.lua")
                .exists()
        );
        assert!(
            !sd_dir
                .path()
                .join("SCRIPTS/TOOLS/BWToolGeneric/main.lua")
                .exists()
        );
        assert!(!sd_dir.path().join("WIDGETS/ColorWidget/main.lua").exists());
    }

    #[test]
    fn test_install_variant_unknown_radio_fails() {
        let (pkg_dir, sd_dir) = setup_variant_package();

        // Unknown radio (e.g. --dir to a plain directory, board not in catalog)
        let result = InstallCommand::new(InstallOptions {
            sd_root: sd_dir.path().to_path_buf(),
            pkg_ref: PackageRef::Local {
                path: pkg_dir.path().to_path_buf(),
                variant: String::new(),
            },
            dev: false,
            radio: None,
        });

        let err = result.err().expect("should fail");
        assert!(err.to_string().contains("could not detect"));
    }

    #[test]
    fn test_install_variant_no_match_errors() {
        let pkg_dir = TempDir::new().unwrap();

        // Only color variant available
        std::fs::write(
            pkg_dir.path().join("edgetx.yml"),
            r#"
package:
  id: example.com/test/color-only
  description: "Test"
  variants:
    - path: edgetx.color.yml
      capabilities:
        display:
          type: colorlcd
"#,
        )
        .unwrap();
        std::fs::write(
            pkg_dir.path().join("edgetx.color.yml"),
            r#"
package:
  id: example.com/test/color-only
  description: "Test"
widgets:
  - name: ColorWidget
    path: WIDGETS/ColorWidget
"#,
        )
        .unwrap();
        let p = pkg_dir.path().join("WIDGETS/ColorWidget/main.lua");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, "-- color").unwrap();

        let sd_dir = TempDir::new().unwrap();
        std::fs::create_dir_all(sd_dir.path().join("RADIO")).unwrap();

        // BW radio - no matching variant, should fail
        let result = InstallCommand::new(InstallOptions {
            sd_root: sd_dir.path().to_path_buf(),
            pkg_ref: PackageRef::Local {
                path: pkg_dir.path().to_path_buf(),
                variant: String::new(),
            },
            dev: false,
            radio: Some(RadioCapabilities {
                display: DisplayCapabilities {
                    width: 128,
                    height: 64,
                    color: false,
                    touch: None,
                },
            }),
        });

        let err = result.err().expect("should fail");
        assert!(err.to_string().contains("no matching variant"));
    }
}
