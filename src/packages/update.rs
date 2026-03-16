use anyhow::Result;
use std::path::{Path, PathBuf};

use crate::manifest;
use crate::radio;
use crate::registry::{PackageRef, resolve};

use super::conflict::check_conflicts;
use super::install::{build_exclude, count_install_files, remove_tracked_files};
use super::state::{self, InstalledPackage, State};

/// UpdateOptions configures an update operation.
pub struct UpdateOptions {
    pub sd_root: PathBuf,
    pub query: String,
    pub all: bool,
    pub dev: bool,
    pub dev_set: bool,
    pub dry_run: bool,
    pub before_copy: Option<Box<dyn Fn(&str, usize)>>,
    pub on_file: Option<Box<dyn Fn(&str)>>,
}

/// UpdateResult holds the outcome of updating a single package.
#[derive(Debug)]
pub struct UpdateResult {
    pub package: InstalledPackage,
    pub files_copied: usize,
    pub up_to_date: bool,
}

/// Update one or all installed packages.
pub fn update(opts: UpdateOptions) -> Result<Vec<UpdateResult>> {
    if opts.query.is_empty() && !opts.all {
        anyhow::bail!("specify a package name or use --all");
    }

    let mut state = state::load_state(&opts.sd_root)
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    let targets: Vec<InstalledPackage>;
    let original_sources: Vec<String>;
    let mut version_override = String::new();

    if opts.all {
        targets = state.packages.clone();
        original_sources = targets.iter().map(|t| t.source.clone()).collect();
    } else {
        let pkg_ref: PackageRef = opts.query.parse()
            .map_err(|e| anyhow::anyhow!("{e}"))?;
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

        let result = update_single(
            &opts.sd_root,
            pkg,
            &original_sources[i],
            &mut state,
            &version_override,
            include_dev,
            opts.dry_run,
            &opts.on_file,
            &opts.before_copy,
        )?;
        results.push(result);
    }

    Ok(results)
}

fn update_single(
    sd_root: &Path,
    pkg: &InstalledPackage,
    original_source: &str,
    state: &mut State,
    version_override: &str,
    include_dev: bool,
    dry_run: bool,
    on_file: &Option<Box<dyn Fn(&str)>>,
    before_copy: &Option<Box<dyn Fn(&str, usize)>>,
) -> Result<UpdateResult> {
    // Pinned commits can't be updated without explicit version
    if pkg.channel == "commit" && version_override.is_empty() {
        return Ok(UpdateResult {
            package: pkg.clone(),
            files_copied: 0,
            up_to_date: true,
        });
    }

    let (m, manifest_dir, new_channel, new_version, new_commit) = if pkg.channel == "local" {
        // Re-copy from local path
        let pkg_ref: PackageRef = pkg.source.parse()
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        let (local_path, sub_path) = match &pkg_ref {
            PackageRef::Local { path, sub_path } => (path.clone(), sub_path.clone()),
            _ => anyhow::bail!("expected local package for channel=local"),
        };

        let (m, mdir) = manifest::load_with_sub_path(&local_path, &sub_path)?;
        (m, mdir, "local".to_string(), String::new(), String::new())
    } else {
        let mut pkg_ref: PackageRef = pkg.source.parse()
            .map_err(|e| anyhow::anyhow!("parsing source {:?}: {e}", pkg.source))?;

        if !version_override.is_empty() {
            pkg_ref.set_version(version_override.to_string());
        } else if pkg.channel == "branch" {
            pkg_ref.set_version(pkg.version.clone());
        }
        // tag channel with no override: leave version empty to get latest

        let result = resolve::resolve_package(&pkg_ref)
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        // Check if already up to date
        if result.resolved.hash == pkg.commit {
            return Ok(UpdateResult {
                package: pkg.clone(),
                files_copied: 0,
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
    check_conflicts(state, &new_paths, original_source)
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    let mut total_copied = 0;

    if !dry_run {
        remove_tracked_files(sd_root, &pkg.name);
        state.remove(original_source);

        if let Some(cb) = before_copy {
            cb(&m.package.name, count_install_files(&manifest_dir, &m, include_dev));
        }

        let mut copied_files = Vec::new();
        for item in m.content_items(include_dev) {
            let source_root = m.resolve_content_path(&manifest_dir, &item.path)
                .map_err(|e| anyhow::anyhow!("resolving {}: {e}", item.path))?;
            let exclude = build_exclude(m.package.binary, &item);
            let copied_ref = std::cell::RefCell::new(&mut copied_files);
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
                        if let Some(cb) = on_file {
                            cb(&dest.display().to_string());
                        }
                    }),
                },
            )?;
            total_copied += n;
        }

        let updated = InstalledPackage {
            source: pkg.source.clone(),
            name: m.package.name.clone(),
            channel: new_channel.clone(),
            version: new_version.clone(),
            commit: new_commit.clone(),
            paths: new_paths,
            dev: include_dev,
        };
        state.add(updated.clone());
        state.save(sd_root).map_err(|e| anyhow::anyhow!("saving state: {e}"))?;
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
            source: pkg.source.clone(),
            name: m.package.name,
            channel: new_channel,
            version: new_version,
            commit: new_commit,
            paths: new_paths,
            dev: include_dev,
        },
        files_copied: 0,
        up_to_date: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use tempfile::TempDir;

    fn run_git(dir: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
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
            channel: "local".into(),
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
            &files.iter().map(|(p, _)| p.to_string()).collect::<Vec<_>>(),
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

        let content = std::fs::read_to_string(
            sd_dir.path().join("SCRIPTS/TOOLS/MyTool/main.lua"),
        )
        .unwrap();
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
                channel: "commit".into(),
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
