use crate::error::RegistryError;
use crate::manifest;
use crate::registry::{PackageRef, version::ResolvedVersion};
use std::path::{Path, PathBuf};

/// CloneResult holds the outcome of resolving a package.
#[derive(Debug)]
pub struct CloneResult {
    pub manifest: manifest::Manifest,
    pub dir: PathBuf,
    pub manifest_dir: PathBuf,
    pub resolved: ResolvedVersion,
}

/// Returns the platform-appropriate cache directory for edgetx-cli repos.
pub fn cache_dir() -> Result<PathBuf, RegistryError> {
    let base = directories::BaseDirs::new()
        .ok_or_else(|| RegistryError::Other("cannot determine cache directory".into()))?;
    Ok(base.cache_dir().join("edgetx-cli").join("repos"))
}

/// Resolve a package reference: fetch if remote, then extract and load manifest.
/// Uses a persistent cache under the user's cache directory.
pub fn resolve_package(pkg_ref: &PackageRef) -> Result<CloneResult, RegistryError> {
    resolve_package_with_cache(pkg_ref, None)
}

/// Like `resolve_package` but with an optional cache base override (for testing).
pub fn resolve_package_with_cache(
    pkg_ref: &PackageRef,
    cache_base_override: Option<&Path>,
) -> Result<CloneResult, RegistryError> {
    if let PackageRef::Local { path, sub_path } = pkg_ref {
        return load_from_local(path, sub_path);
    }

    let cache_base = match cache_base_override {
        Some(p) => p.to_path_buf(),
        None => cache_dir()?,
    };

    let url = pkg_ref.clone_url();

    // Fetch bare repo into a temp dir
    let tmp_dir = tempfile::tempdir()
        .map_err(|e| RegistryError::Other(format!("creating temp dir: {e}")))?;

    log::debug!("fetching {} to {:?}", url, tmp_dir.path());

    let repo = fetch_repository(&url, tmp_dir.path())?;

    // Resolve version
    let resolved = resolve_version_from_repo(&repo, pkg_ref.version())?;

    // Check cache
    let cache_path = cache_base
        .join(pkg_ref.canonical())
        .join(&resolved.hash);

    if cache_path.is_dir() {
        log::debug!("cache hit: {}", cache_path.display());
        return load_from_dir(&cache_path, pkg_ref.sub_path(), resolved);
    }

    // Extract the tree into the cache directory
    let _ = std::fs::create_dir_all(&cache_path);
    extract_tree_to_dir(&repo, &resolved.hash, &cache_path)?;

    load_from_dir(&cache_path, pkg_ref.sub_path(), resolved)
}

/// Bare-clone (fetch) a repository into `dest`.
fn fetch_repository(url: &str, dest: &Path) -> Result<gix::Repository, RegistryError> {
    use gix::progress::Discard;

    let mut prepare = gix::prepare_clone_bare(url, dest).map_err(|e| RegistryError::Clone {
        url: url.to_string(),
        reason: e.to_string(),
    })?;

    let (repo, _outcome) = prepare
        .fetch_only(Discard, &gix::interrupt::IS_INTERRUPTED)
        .map_err(|e| RegistryError::Clone {
            url: url.to_string(),
            reason: e.to_string(),
        })?;

    Ok(repo)
}

/// Extract all files from the commit's tree into `dest`.
fn extract_tree_to_dir(
    repo: &gix::Repository,
    hash: &str,
    dest: &Path,
) -> Result<(), RegistryError> {
    let id = repo
        .rev_parse_single(hash)
        .map_err(|e| RegistryError::Other(format!("resolving {hash}: {e}")))?;

    let commit = id
        .object()
        .map_err(|e| RegistryError::Other(format!("reading object {hash}: {e}")))?
        .peel_to_kind(gix::object::Kind::Commit)
        .map_err(|e| RegistryError::Other(format!("peeling to commit: {e}")))?;

    let tree = commit
        .into_commit()
        .tree()
        .map_err(|e| RegistryError::Other(format!("reading tree: {e}")))?;

    // Recursively walk the tree and write files
    extract_tree_recursive(repo, &tree, dest, &PathBuf::new())?;

    Ok(())
}

fn extract_tree_recursive(
    repo: &gix::Repository,
    tree: &gix::Tree<'_>,
    dest: &Path,
    prefix: &Path,
) -> Result<(), RegistryError> {
    for entry in tree.iter() {
        let entry = entry.map_err(|e| RegistryError::Other(format!("tree entry: {e}")))?;
        let name = entry.filename().to_string();
        let entry_path = prefix.join(&name);

        let object = entry
            .object()
            .map_err(|e| RegistryError::Other(format!("reading {}: {e}", entry_path.display())))?;

        match object.kind {
            gix::object::Kind::Blob => {
                let file_path = dest.join(&entry_path);
                if let Some(parent) = file_path.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| {
                        RegistryError::Other(format!(
                            "creating dir {}: {e}",
                            parent.display()
                        ))
                    })?;
                }
                std::fs::write(&file_path, &*object.data).map_err(|e| {
                    RegistryError::Other(format!("writing {}: {e}", file_path.display()))
                })?;
            }
            gix::object::Kind::Tree => {
                let subtree = object
                    .peel_to_tree()
                    .map_err(|e| RegistryError::Other(format!("peeling subtree: {e}")))?;
                extract_tree_recursive(repo, &subtree, dest, &entry_path)?;
            }
            _ => {} // skip tags, etc.
        }
    }
    Ok(())
}

fn resolve_version_from_repo(
    repo: &gix::Repository,
    version: &str,
) -> Result<ResolvedVersion, RegistryError> {
    // Collect tags
    let tags: Vec<String> = repo
        .references()
        .ok()
        .and_then(|refs| {
            refs.tags().ok().map(|iter| {
                iter.filter_map(|r| r.ok().map(|r| r.name().shorten().to_string()))
                    .collect()
            })
        })
        .unwrap_or_default();

    // Collect branches
    let branches: Vec<String> = repo
        .references()
        .ok()
        .and_then(|refs| {
            refs.remote_branches().ok().map(|iter| {
                iter.filter_map(|r| {
                    r.ok().map(|r| {
                        let name = r.name().shorten().to_string();
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
) -> Result<String, RegistryError> {
    let spec = match channel {
        "tag" => format!("refs/tags/{name}"),
        "branch" => format!("refs/remotes/origin/{name}"),
        _ => name.to_string(),
    };

    let id = repo
        .rev_parse_single(spec.as_str())
        .map_err(|e| RegistryError::Other(format!("resolving {spec}: {e}")))?;

    // Peel to commit
    let commit = id
        .object()
        .map_err(|e| RegistryError::Other(format!("peeling {spec}: {e}")))?
        .peel_to_kind(gix::object::Kind::Commit)
        .map_err(|e| RegistryError::Other(format!("peeling to commit: {e}")))?;

    Ok(commit.id().to_string())
}

fn load_from_local(dir: &Path, sub_path: &str) -> Result<CloneResult, RegistryError> {
    let resolved = ResolvedVersion {
        channel: "local".into(),
        version: String::new(),
        hash: String::new(),
    };
    load_from_dir(dir, sub_path, resolved)
}

pub(crate) fn load_from_dir(
    dir: &Path,
    sub_path: &str,
    resolved: ResolvedVersion,
) -> Result<CloneResult, RegistryError> {
    let (m, manifest_dir) = manifest::load_with_sub_path(dir, sub_path)
        .map_err(|e| RegistryError::NoManifest(e.to_string()))?;

    Ok(CloneResult {
        manifest: m,
        dir: dir.to_path_buf(),
        manifest_dir,
        resolved,
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

    fn create_test_repo(manifest: &str, files: &[(&str, &str)]) -> TempDir {
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        run_git(root, &["init", "-b", "main"]);
        run_git(root, &["config", "user.email", "test@test.com"]);
        run_git(root, &["config", "user.name", "Test"]);

        std::fs::write(root.join("edgetx.yml"), manifest).unwrap();

        for (path, content) in files {
            let full = root.join(path);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(&full, content).unwrap();
        }

        run_git(root, &["add", "-A"]);
        run_git(root, &["commit", "-m", "initial"]);

        dir
    }

    #[test]
    fn test_cache_dir_returns_path() {
        let path = cache_dir().unwrap();
        assert!(path.ends_with("edgetx-cli/repos"));
    }

    #[test]
    fn test_load_from_dir_with_manifest() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("edgetx.yml"),
            "package:\n  name: test-pkg\n",
        )
        .unwrap();

        let resolved = ResolvedVersion {
            channel: "tag".into(),
            version: "v1.0".into(),
            hash: "abc".into(),
        };
        let result = load_from_dir(tmp.path(), "", resolved).unwrap();
        assert_eq!(result.manifest.package.name, "test-pkg");
    }

    #[test]
    fn test_load_from_dir_missing_manifest() {
        let tmp = tempfile::tempdir().unwrap();
        let resolved = ResolvedVersion {
            channel: "tag".into(),
            version: "v1.0".into(),
            hash: "abc".into(),
        };
        let result = load_from_dir(tmp.path(), "", resolved);
        assert!(result.is_err());
        match result.unwrap_err() {
            RegistryError::NoManifest(_) => {}
            other => panic!("expected NoManifest, got: {other:?}"),
        }
    }

    #[test]
    fn test_load_from_local() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("edgetx.yml"),
            "package:\n  name: local-pkg\n",
        )
        .unwrap();

        let result = load_from_local(tmp.path(), "").unwrap();
        assert_eq!(result.resolved.channel, "local");
        assert_eq!(result.manifest.package.name, "local-pkg");
    }

    #[test]
    fn test_load_from_dir_with_yml_sub_path() {
        let tmp = tempfile::tempdir().unwrap();
        let sub = tmp.path().join("variants");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(
            sub.join("edgetx.c480x272.yml"),
            "package:\n  name: variant-pkg\n",
        )
        .unwrap();

        let resolved = ResolvedVersion {
            channel: "tag".into(),
            version: "v1.0".into(),
            hash: "abc".into(),
        };
        let result =
            load_from_dir(tmp.path(), "variants/edgetx.c480x272.yml", resolved).unwrap();
        assert_eq!(result.manifest.package.name, "variant-pkg");
        assert_eq!(result.manifest_dir, sub);
    }

    #[test]
    fn test_load_from_dir_with_dir_sub_path() {
        let tmp = tempfile::tempdir().unwrap();
        let sub = tmp.path().join("subpkg");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("edgetx.yml"), "package:\n  name: sub-pkg\n").unwrap();

        let resolved = ResolvedVersion {
            channel: "tag".into(),
            version: "v1.0".into(),
            hash: "abc".into(),
        };
        let result = load_from_dir(tmp.path(), "subpkg", resolved).unwrap();
        assert_eq!(result.manifest.package.name, "sub-pkg");
        assert_eq!(result.manifest_dir, sub);
    }

    #[test]
    fn test_fetch_and_extract_produces_files() {
        let repo = create_test_repo(
            "package:\n  name: test-pkg\ntools:\n  - name: Tool\n    path: SCRIPTS/TOOLS/Tool\n",
            &[("SCRIPTS/TOOLS/Tool/main.lua", "-- hello")],
        );

        let tmp = TempDir::new().unwrap();
        let url = format!("file://{}", repo.path().display());
        let fetched = fetch_repository(&url, tmp.path()).unwrap();

        let dest = TempDir::new().unwrap();
        let head = fetched.head_commit().unwrap().id().to_string();
        extract_tree_to_dir(&fetched, &head, dest.path()).unwrap();

        assert!(dest.path().join("edgetx.yml").exists());
        assert!(dest.path().join("SCRIPTS/TOOLS/Tool/main.lua").exists());
        let content =
            std::fs::read_to_string(dest.path().join("SCRIPTS/TOOLS/Tool/main.lua")).unwrap();
        assert_eq!(content, "-- hello");
    }

    #[test]
    fn test_resolve_specific_tag() {
        let repo = create_test_repo(
            "package:\n  name: tagged\ntools:\n  - name: T\n    path: SCRIPTS/TOOLS/T\n",
            &[("SCRIPTS/TOOLS/T/main.lua", "-- v1")],
        );
        run_git(repo.path(), &["tag", "v1.0.0"]);

        // Make a second commit and tag
        std::fs::write(repo.path().join("SCRIPTS/TOOLS/T/main.lua"), "-- v2").unwrap();
        run_git(repo.path(), &["add", "-A"]);
        run_git(repo.path(), &["commit", "-m", "v2"]);
        run_git(repo.path(), &["tag", "v2.0.0"]);

        let cache = TempDir::new().unwrap();
        let pkg_ref: PackageRef = format!("file://{}@v1.0.0", repo.path().display())
            .parse()
            .unwrap();
        let result = resolve_package_with_cache(&pkg_ref, Some(cache.path())).unwrap();

        assert_eq!(result.resolved.channel, "tag");
        assert_eq!(result.resolved.version, "v1.0.0");

        let content =
            std::fs::read_to_string(result.dir.join("SCRIPTS/TOOLS/T/main.lua")).unwrap();
        assert_eq!(content, "-- v1");
    }

    #[test]
    fn test_resolve_specific_branch() {
        let repo = create_test_repo(
            "package:\n  name: branched\ntools:\n  - name: T\n    path: SCRIPTS/TOOLS/T\n",
            &[("SCRIPTS/TOOLS/T/main.lua", "-- main")],
        );

        run_git(repo.path(), &["checkout", "-b", "feature"]);
        std::fs::write(repo.path().join("SCRIPTS/TOOLS/T/main.lua"), "-- feature").unwrap();
        run_git(repo.path(), &["add", "-A"]);
        run_git(repo.path(), &["commit", "-m", "feature work"]);
        run_git(repo.path(), &["checkout", "main"]);

        let cache = TempDir::new().unwrap();
        let pkg_ref: PackageRef = format!("file://{}@feature", repo.path().display())
            .parse()
            .unwrap();
        let result = resolve_package_with_cache(&pkg_ref, Some(cache.path())).unwrap();

        assert_eq!(result.resolved.channel, "branch");
        assert_eq!(result.resolved.version, "feature");

        let content =
            std::fs::read_to_string(result.dir.join("SCRIPTS/TOOLS/T/main.lua")).unwrap();
        assert_eq!(content, "-- feature");
    }
}
