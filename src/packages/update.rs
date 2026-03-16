use anyhow::Result;
use std::path::{Path, PathBuf};

use crate::manifest;
use crate::radio;
use crate::repository::{self, clone, source::Source};

use super::conflict::check_conflicts;
use super::install::{build_exclude, clean_empty_parents, count_install_files, remove_tracked_files};
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

    let mut targets: Vec<InstalledPackage>;
    let mut original_sources: Vec<String>;
    let mut version_override = String::new();

    if opts.all {
        targets = state.packages.clone();
        original_sources = targets.iter().map(|t| t.source.clone()).collect();
    } else {
        let src = opts.query.parse::<Source>().unwrap();
        let query = src.canonical();
        version_override = src.version.clone();

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
        let src = pkg.source.parse::<Source>().unwrap();
        let local_path = &src.base;
        let sub_path = &src.sub_path;

        let (m, mdir) = if sub_path.is_empty() {
            let m = manifest::load(Path::new(local_path))?;
            (m, PathBuf::from(local_path))
        } else if sub_path.ends_with(".yml") || sub_path.ends_with(".yaml") {
            let path = Path::new(local_path).join(sub_path);
            let m = manifest::load_file(&path)?;
            (m, path.parent().unwrap_or(Path::new(local_path)).to_path_buf())
        } else {
            let m = manifest::load(&Path::new(local_path).join(sub_path))?;
            (m, Path::new(local_path).join(sub_path))
        };
        (m, mdir, "local".to_string(), String::new(), String::new())
    } else {
        let src = pkg.source.parse::<Source>().unwrap();
        let mut pkg_ref = repository::parse_package_ref(&src.base)
            .map_err(|e| anyhow::anyhow!("parsing source {:?}: {e}", pkg.source))?;
        pkg_ref.sub_path = src.sub_path;

        if !version_override.is_empty() {
            pkg_ref.version = version_override.to_string();
        } else if pkg.channel == "branch" {
            pkg_ref.version = pkg.version.clone();
        }
        // tag channel with no override: leave version empty to get latest

        let result = clone::clone_and_checkout(&pkg_ref)
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
