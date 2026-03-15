use crate::error::RepositoryError;
use crate::manifest;
use crate::repository::{PackageRef, version::ResolvedVersion};
use std::path::{Path, PathBuf};

/// CloneResult holds the outcome of cloning and checking out a repository.
#[derive(Debug)]
pub struct CloneResult {
    pub manifest: manifest::Manifest,
    pub dir: PathBuf,
    pub manifest_dir: PathBuf,
    pub resolved: ResolvedVersion,
}

/// Returns the platform-appropriate cache directory for edgetx-cli repos.
pub fn cache_dir() -> Result<PathBuf, RepositoryError> {
    let base = directories::BaseDirs::new()
        .ok_or_else(|| RepositoryError::Other("cannot determine cache directory".into()))?;
    Ok(base.cache_dir().join("edgetx-cli").join("repos"))
}

/// Clone a repository and check out the specified version.
/// Uses a persistent cache under the user's cache directory.
pub fn clone_and_checkout(pkg_ref: &PackageRef) -> Result<CloneResult, RepositoryError> {
    if pkg_ref.is_local {
        return load_from_local(&pkg_ref.local_path, &pkg_ref.sub_path);
    }

    let cache_base = cache_dir()?;
    let url = pkg_ref.clone_url();

    // Use gix to clone
    let tmp_dir = tempfile::tempdir()
        .map_err(|e| RepositoryError::Other(format!("creating temp dir: {e}")))?;

    log::debug!("cloning {} to {:?}", url, tmp_dir.path());

    // Try clone with gix
    let repo = clone_with_gix(&url, tmp_dir.path(), &pkg_ref.version)?;

    // Resolve version
    let resolved = resolve_version_from_repo(&repo, &pkg_ref.version)?;

    // Check cache
    let cache_path = cache_base
        .join(pkg_ref.canonical())
        .join(&resolved.hash);

    if cache_path.is_dir() {
        // Cache hit
        log::debug!("cache hit: {}", cache_path.display());
        return load_from_dir(&cache_path, &pkg_ref.sub_path, resolved);
    }

    // Checkout the resolved commit
    checkout_commit(&repo, &resolved.hash)?;

    // Move to cache
    let source_path = tmp_dir.path().to_path_buf();
    if let Err(_) = std::fs::create_dir_all(cache_path.parent().unwrap()) {
        // Fall through, use tmp dir
    }

    let final_path = if std::fs::rename(&source_path, &cache_path).is_ok() {
        // Prevent tempdir cleanup from removing our cached data
        let _ = tmp_dir.into_path();
        cache_path
    } else {
        let kept = tmp_dir.into_path();
        kept
    };

    load_from_dir(&final_path, &pkg_ref.sub_path, resolved)
}

fn clone_with_gix(
    url: &str,
    dest: &Path,
    version: &str,
) -> Result<gix::Repository, RepositoryError> {
    use gix::progress::Discard;

    let mut prepare = gix::prepare_clone_bare(url, dest)
        .map_err(|e| RepositoryError::Clone {
            url: url.to_string(),
            reason: e.to_string(),
        })?;

    let (mut checkout, _outcome) = prepare
        .fetch_only(Discard, &gix::interrupt::IS_INTERRUPTED)
        .map_err(|e| RepositoryError::Clone {
            url: url.to_string(),
            reason: e.to_string(),
        })?;

    Ok(checkout)
}

fn resolve_version_from_repo(
    repo: &gix::Repository,
    version: &str,
) -> Result<ResolvedVersion, RepositoryError> {
    // Collect tags
    let tags: Vec<String> = repo
        .references()
        .ok()
        .and_then(|refs| {
            refs.tags()
                .ok()
                .map(|iter| {
                    iter.filter_map(|r| {
                        r.ok().map(|r| {
                            r.name().shorten().to_string()
                        })
                    })
                    .collect()
                })
        })
        .unwrap_or_default();

    // Collect branches
    let branches: Vec<String> = repo
        .references()
        .ok()
        .and_then(|refs| {
            refs.remote_branches()
                .ok()
                .map(|iter| {
                    iter.filter_map(|r| {
                        r.ok().map(|r| {
                            let name = r.name().shorten().to_string();
                            // Strip "origin/" prefix
                            name.strip_prefix("origin/")
                                .unwrap_or(&name)
                                .to_string()
                        })
                    })
                    .collect()
                })
        })
        .unwrap_or_default();

    // Get HEAD
    let head = repo
        .head_commit()
        .map(|c| c.id().to_string())
        .unwrap_or_default();

    let default_branch = repo
        .head_ref()
        .ok()
        .flatten()
        .map(|r| r.name().shorten().to_string())
        .unwrap_or_else(|| "main".to_string());

    let mut resolved =
        super::version::resolve_version(&tags, &branches, &default_branch, &head, version)?;

    // If hash is empty, resolve it from the ref
    if resolved.hash.is_empty() {
        resolved.hash = resolve_ref_to_hash(repo, &resolved.version, &resolved.channel)?;
    }

    Ok(resolved)
}

fn resolve_ref_to_hash(
    repo: &gix::Repository,
    name: &str,
    channel: &str,
) -> Result<String, RepositoryError> {
    let spec = match channel {
        "tag" => format!("refs/tags/{name}"),
        "branch" => format!("refs/remotes/origin/{name}"),
        _ => name.to_string(),
    };

    let id = repo
        .rev_parse_single(spec.as_str())
        .map_err(|e| RepositoryError::Other(format!("resolving {spec}: {e}")))?;

    // Peel to commit
    let commit = id
        .object()
        .map_err(|e| RepositoryError::Other(format!("peeling {spec}: {e}")))?
        .peel_to_kind(gix::object::Kind::Commit)
        .map_err(|e| RepositoryError::Other(format!("peeling to commit: {e}")))?;

    Ok(commit.id().to_string())
}

fn checkout_commit(repo: &gix::Repository, hash: &str) -> Result<(), RepositoryError> {
    // For bare repos, we just need to verify the commit exists
    let _commit = repo
        .rev_parse_single(hash)
        .map_err(|e| RepositoryError::Other(format!("checking out {hash}: {e}")))?;
    Ok(())
}

fn load_from_local(
    dir: &Path,
    sub_path: &str,
) -> Result<CloneResult, RepositoryError> {
    let resolved = ResolvedVersion {
        channel: "local".into(),
        version: String::new(),
        hash: String::new(),
    };
    load_from_dir(dir, sub_path, resolved)
}

fn load_from_dir(
    dir: &Path,
    sub_path: &str,
    resolved: ResolvedVersion,
) -> Result<CloneResult, RepositoryError> {
    let (m, manifest_dir) = if sub_path.is_empty() {
        let m = manifest::load(dir)
            .map_err(|e| RepositoryError::NoManifest(e.to_string()))?;
        (m, dir.to_path_buf())
    } else if sub_path.ends_with(".yml") || sub_path.ends_with(".yaml") {
        let path = dir.join(sub_path);
        let m = manifest::load_file(&path)
            .map_err(|e| RepositoryError::NoManifest(e.to_string()))?;
        let mdir = path.parent().unwrap_or(dir).to_path_buf();
        (m, mdir)
    } else {
        let m = manifest::load(&dir.join(sub_path))
            .map_err(|e| RepositoryError::NoManifest(e.to_string()))?;
        (m, dir.join(sub_path))
    };

    Ok(CloneResult {
        manifest: m,
        dir: dir.to_path_buf(),
        manifest_dir,
        resolved,
    })
}
