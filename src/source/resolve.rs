use crate::error::SourceError;
use crate::manifest;
use crate::source::{PackageRef, version::ResolvedVersion};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A submodule entry found during tree extraction.
struct SubmoduleEntry {
    path: String,
    hash: String,
}

/// CloneResult holds the outcome of resolving a package.
#[derive(Debug)]
pub struct CloneResult {
    pub manifest: manifest::Manifest,
    #[allow(dead_code)]
    pub dir: PathBuf,
    pub manifest_dir: PathBuf,
    pub resolved: ResolvedVersion,
}

/// Returns the platform-appropriate cache directory for edgetx-cli repos.
pub fn cache_dir() -> Result<PathBuf, SourceError> {
    let base = directories::BaseDirs::new()
        .ok_or_else(|| SourceError::Other("cannot determine cache directory".into()))?;
    Ok(base.cache_dir().join("edgetx-cli").join("repos"))
}

/// Resolve a package reference: fetch if remote, then extract and load manifest.
/// Uses a persistent cache under the user's cache directory.
pub fn resolve_package(pkg_ref: &PackageRef) -> Result<CloneResult, SourceError> {
    resolve_package_with_cache(pkg_ref, None)
}

/// Like `resolve_package` but with an optional cache base override (for testing).
pub fn resolve_package_with_cache(
    pkg_ref: &PackageRef,
    cache_base_override: Option<&Path>,
) -> Result<CloneResult, SourceError> {
    if let PackageRef::Local { path, sub_path } = pkg_ref {
        return load_from_local(path, sub_path);
    }

    let cache_base = match cache_base_override {
        Some(p) => p.to_path_buf(),
        None => cache_dir()?,
    };

    let url = pkg_ref.clone_url();

    // Fetch bare repo into a temp dir
    let tmp_dir =
        tempfile::tempdir().map_err(|e| SourceError::Other(format!("creating temp dir: {e}")))?;

    log::debug!("fetching {} to {:?}", url, tmp_dir.path());

    let repo = fetch_repository(&url, tmp_dir.path())?;

    // Resolve version
    let resolved = resolve_version_from_repo(&repo, pkg_ref.version())?;

    // Check cache — a valid cache has a .complete marker written after extraction.
    // This guards against corrupt partial caches left by older versions or interrupted runs.
    let cache_path = cache_base.join(pkg_ref.canonical()).join(&resolved.hash);
    let complete_marker = cache_path.join(".complete");

    if complete_marker.is_file() {
        log::debug!("cache hit: {}", cache_path.display());
        return load_from_dir(&cache_path, pkg_ref.sub_path(), resolved);
    }

    // Remove any stale incomplete cache
    if cache_path.is_dir() {
        log::debug!("removing incomplete cache: {}", cache_path.display());
        let _ = std::fs::remove_dir_all(&cache_path);
    }

    // Extract into a temp dir first, then rename to cache path atomically.
    // This prevents partial cache directories from persisting on failure.
    let cache_parent = cache_path.parent().unwrap_or(&cache_path);
    let _ = std::fs::create_dir_all(cache_parent);
    let tmp_extract = tempfile::tempdir_in(cache_parent)
        .map_err(|e| SourceError::Other(format!("creating temp dir for extraction: {e}")))?;

    extract_tree_to_dir(&repo, &resolved.hash, tmp_extract.path())?;

    // Write completion marker
    std::fs::write(tmp_extract.path().join(".complete"), "")
        .map_err(|e| SourceError::Other(format!("writing cache marker: {e}")))?;

    // Atomically move the completed extraction into the cache slot
    std::fs::rename(tmp_extract.path(), &cache_path)
        .map_err(|e| SourceError::Other(format!("moving extraction to cache: {e}")))?;
    // Prevent tempdir drop from removing the renamed directory
    std::mem::forget(tmp_extract);

    load_from_dir(&cache_path, pkg_ref.sub_path(), resolved)
}

/// Bare-clone (fetch) a repository into `dest`.
fn fetch_repository(url: &str, dest: &Path) -> Result<gix::Repository, SourceError> {
    use gix::progress::Discard;

    let mut prepare = gix::prepare_clone_bare(url, dest).map_err(|e| SourceError::Clone {
        url: url.to_string(),
        reason: e.to_string(),
    })?;

    let (repo, _outcome) = prepare
        .fetch_only(Discard, &gix::interrupt::IS_INTERRUPTED)
        .map_err(|e| SourceError::Clone {
            url: url.to_string(),
            reason: e.to_string(),
        })?;

    Ok(repo)
}

/// Extract all files from the commit's tree into `dest`, including submodule contents.
fn extract_tree_to_dir(repo: &gix::Repository, hash: &str, dest: &Path) -> Result<(), SourceError> {
    let id = repo
        .rev_parse_single(hash)
        .map_err(|e| SourceError::Other(format!("resolving {hash}: {e}")))?;

    let commit = id
        .object()
        .map_err(|e| SourceError::Other(format!("reading object {hash}: {e}")))?
        .peel_to_kind(gix::object::Kind::Commit)
        .map_err(|e| SourceError::Other(format!("peeling to commit: {e}")))?;

    let tree = commit
        .into_commit()
        .tree()
        .map_err(|e| SourceError::Other(format!("reading tree: {e}")))?;

    // Recursively walk the tree, collecting submodule entries
    let mut submodules = Vec::new();
    extract_tree_recursive(repo, &tree, dest, &PathBuf::new(), &mut submodules)?;

    // Fetch and extract submodule contents
    fetch_submodules(&submodules, dest)?;

    Ok(())
}

#[allow(clippy::only_used_in_recursion)]
fn extract_tree_recursive(
    repo: &gix::Repository,
    tree: &gix::Tree<'_>,
    dest: &Path,
    prefix: &Path,
    submodules: &mut Vec<SubmoduleEntry>,
) -> Result<(), SourceError> {
    for entry in tree.iter() {
        let entry = entry.map_err(|e| SourceError::Other(format!("tree entry: {e}")))?;

        // Collect submodule entries for separate fetching — their commit objects
        // aren't in this bare clone so we can't call entry.object() on them.
        if entry.mode().is_commit() {
            let name = entry.filename().to_string();
            submodules.push(SubmoduleEntry {
                path: prefix.join(&name).to_string_lossy().into_owned(),
                hash: entry.object_id().to_string(),
            });
            continue;
        }

        let name = entry.filename().to_string();
        let entry_path = prefix.join(&name);

        let object = entry
            .object()
            .map_err(|e| SourceError::Other(format!("reading {}: {e}", entry_path.display())))?;

        match object.kind {
            gix::object::Kind::Blob => {
                let file_path = dest.join(&entry_path);
                if let Some(parent) = file_path.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| {
                        SourceError::Other(format!("creating dir {}: {e}", parent.display()))
                    })?;
                }
                std::fs::write(&file_path, &*object.data).map_err(|e| {
                    SourceError::Other(format!("writing {}: {e}", file_path.display()))
                })?;
            }
            gix::object::Kind::Tree => {
                let subtree = object
                    .peel_to_tree()
                    .map_err(|e| SourceError::Other(format!("peeling subtree: {e}")))?;
                extract_tree_recursive(repo, &subtree, dest, &entry_path, submodules)?;
            }
            _ => {}
        }
    }
    Ok(())
}

/// Fetch and extract submodule contents into the destination directory.
fn fetch_submodules(submodules: &[SubmoduleEntry], dest: &Path) -> Result<(), SourceError> {
    if submodules.is_empty() {
        return Ok(());
    }

    let url_map = parse_gitmodules(&dest.join(".gitmodules"))?;

    for sm in submodules {
        let url = match url_map.get(&sm.path) {
            Some(u) => u,
            None => {
                log::warn!("submodule {:?} not found in .gitmodules, skipping", sm.path);
                continue;
            }
        };

        log::debug!("fetching submodule {} from {}", sm.path, url);

        let tmp_dir = tempfile::tempdir()
            .map_err(|e| SourceError::Other(format!("creating temp dir for submodule: {e}")))?;

        let repo = fetch_repository(url, tmp_dir.path())?;

        let sm_dest = dest.join(&sm.path);
        let _ = std::fs::create_dir_all(&sm_dest);
        extract_tree_to_dir(&repo, &sm.hash, &sm_dest)?;
    }

    Ok(())
}

/// Parse a .gitmodules file into a map of submodule path → URL.
fn parse_gitmodules(path: &Path) -> Result<HashMap<String, String>, SourceError> {
    let content = std::fs::read_to_string(path).map_err(|e| {
        SourceError::Other(format!("reading .gitmodules at {}: {e}", path.display()))
    })?;

    let mut map = HashMap::new();
    let mut current_path = None;
    let mut current_url = None;

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with('[') {
            // Flush previous section
            if let (Some(p), Some(u)) = (current_path.take(), current_url.take()) {
                map.insert(p, u);
            }
        } else if let Some(val) = trimmed
            .strip_prefix("path")
            .and_then(|v| v.trim_start().strip_prefix('='))
        {
            current_path = Some(val.trim().to_string());
        } else if let Some(val) = trimmed
            .strip_prefix("url")
            .and_then(|v| v.trim_start().strip_prefix('='))
        {
            current_url = Some(val.trim().to_string());
        }
    }

    // Flush last section
    if let (Some(p), Some(u)) = (current_path, current_url) {
        map.insert(p, u);
    }

    Ok(map)
}

fn resolve_version_from_repo(
    repo: &gix::Repository,
    version: &str,
) -> Result<ResolvedVersion, SourceError> {
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
                        name.strip_prefix("origin/").unwrap_or(&name).to_string()
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
) -> Result<String, SourceError> {
    let spec = match channel {
        "tag" => format!("refs/tags/{name}"),
        "branch" => format!("refs/remotes/origin/{name}"),
        _ => name.to_string(),
    };

    let id = repo
        .rev_parse_single(spec.as_str())
        .map_err(|e| SourceError::Other(format!("resolving {spec}: {e}")))?;

    // Peel to commit
    let commit = id
        .object()
        .map_err(|e| SourceError::Other(format!("peeling {spec}: {e}")))?
        .peel_to_kind(gix::object::Kind::Commit)
        .map_err(|e| SourceError::Other(format!("peeling to commit: {e}")))?;

    Ok(commit.id().to_string())
}

fn load_from_local(dir: &Path, sub_path: &str) -> Result<CloneResult, SourceError> {
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
) -> Result<CloneResult, SourceError> {
    let (m, manifest_dir) = manifest::load_with_sub_path(dir, sub_path)
        .map_err(|e| SourceError::NoManifest(e.to_string()))?;

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
            SourceError::NoManifest(_) => {}
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
        let result = load_from_dir(tmp.path(), "variants/edgetx.c480x272.yml", resolved).unwrap();
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

        let content = std::fs::read_to_string(result.dir.join("SCRIPTS/TOOLS/T/main.lua")).unwrap();
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

        let content = std::fs::read_to_string(result.dir.join("SCRIPTS/TOOLS/T/main.lua")).unwrap();
        assert_eq!(content, "-- feature");
    }

    #[test]
    fn test_extract_with_submodule() {
        // Create a submodule repo with a file
        let sub_repo = TempDir::new().unwrap();
        run_git(sub_repo.path(), &["init", "-b", "main"]);
        run_git(sub_repo.path(), &["config", "user.email", "test@test.com"]);
        run_git(sub_repo.path(), &["config", "user.name", "Test"]);
        std::fs::write(sub_repo.path().join("lib.lua"), "-- submodule lib").unwrap();
        run_git(sub_repo.path(), &["add", "-A"]);
        run_git(sub_repo.path(), &["commit", "-m", "sub initial"]);

        let sub_head = {
            let out = Command::new("git")
                .args(["rev-parse", "HEAD"])
                .current_dir(sub_repo.path())
                .output()
                .unwrap();
            String::from_utf8(out.stdout).unwrap().trim().to_string()
        };

        // Create the main repo with a submodule and files that sort after it
        let main_repo = TempDir::new().unwrap();
        run_git(main_repo.path(), &["init", "-b", "main"]);
        run_git(main_repo.path(), &["config", "user.email", "test@test.com"]);
        run_git(main_repo.path(), &["config", "user.name", "Test"]);

        // Add files that sort alphabetically before and after "deps" (the submodule path)
        std::fs::write(
            main_repo.path().join("edgetx.yml"),
            "package:\n  name: with-submodule\n",
        )
        .unwrap();
        let src_dir = main_repo.path().join("src");
        std::fs::create_dir_all(&src_dir).unwrap();
        std::fs::write(src_dir.join("main.lua"), "-- main script").unwrap();

        // Write .gitmodules manually to avoid file:// transport issues
        let sub_url = format!("file://{}", sub_repo.path().display());
        std::fs::write(
            main_repo.path().join(".gitmodules"),
            format!("[submodule \"deps\"]\n\tpath = deps\n\turl = {}\n", sub_url),
        )
        .unwrap();

        // Add the submodule gitlink entry to the index
        run_git(
            main_repo.path(),
            &["add", ".gitmodules", "edgetx.yml", "src"],
        );
        run_git(
            main_repo.path(),
            &[
                "update-index",
                "--add",
                "--cacheinfo",
                &format!("160000,{},deps", sub_head),
            ],
        );
        run_git(main_repo.path(), &["commit", "-m", "add submodule"]);

        // Fetch and extract
        let tmp = TempDir::new().unwrap();
        let url = format!("file://{}", main_repo.path().display());
        let fetched = fetch_repository(&url, tmp.path()).unwrap();

        let dest = TempDir::new().unwrap();
        let head = fetched.head_commit().unwrap().id().to_string();
        extract_tree_to_dir(&fetched, &head, dest.path()).unwrap();

        // Verify all main repo files are present (including those after submodule alphabetically)
        assert!(
            dest.path().join("edgetx.yml").exists(),
            "edgetx.yml should exist (sorts after submodule 'deps')"
        );
        assert!(
            dest.path().join("src/main.lua").exists(),
            "src/main.lua should exist (sorts after submodule 'deps')"
        );
        assert!(
            dest.path().join(".gitmodules").exists(),
            ".gitmodules should exist"
        );

        // Verify submodule content was fetched and extracted
        assert!(
            dest.path().join("deps/lib.lua").exists(),
            "submodule content deps/lib.lua should be extracted"
        );
        let sub_content = std::fs::read_to_string(dest.path().join("deps/lib.lua")).unwrap();
        assert_eq!(sub_content, "-- submodule lib");
    }

    #[test]
    fn test_parse_gitmodules() {
        let tmp = tempfile::tempdir().unwrap();
        let gitmodules = tmp.path().join(".gitmodules");
        std::fs::write(
            &gitmodules,
            r#"[submodule "mylib"]
	path = libs/mylib
	url = https://github.com/example/mylib.git
[submodule "stdlib"]
	path = deps/stdlib
	url = https://github.com/example/stdlib.git
	branch = main
"#,
        )
        .unwrap();

        let map = parse_gitmodules(&gitmodules).unwrap();
        assert_eq!(map.len(), 2);
        assert_eq!(
            map.get("libs/mylib").unwrap(),
            "https://github.com/example/mylib.git"
        );
        assert_eq!(
            map.get("deps/stdlib").unwrap(),
            "https://github.com/example/stdlib.git"
        );
    }
}
