use std::path::PathBuf;

use crate::source::resolve;
use crate::source::version::Channel;

use super::PackageError;
use super::store::PackageStore;

/// OutdatedOptions configures an outdated check.
pub struct OutdatedOptions {
    pub sd_root: PathBuf,
    /// Root directory for the persistent fetch cache. Production passes
    /// [`resolve::cache_dir`]; tests pass a `TempDir`.
    pub cache_dir: PathBuf,
}

/// Describes one package that has an update available.
pub struct OutdatedPackage {
    pub id: String,
    pub current_version: String,
    pub latest_version: String,
    pub channel: Channel,
}

/// Check installed packages for available updates.
pub fn check_outdated(opts: OutdatedOptions) -> Result<Vec<OutdatedPackage>, PackageError> {
    let store = PackageStore::load(opts.sd_root)?;
    let mut outdated = Vec::new();

    for pkg in store.packages() {
        let pkg_ref = match pkg.to_remote_ref() {
            Ok(Some(r)) => r,
            Ok(None) => continue,
            Err(e) => {
                log::warn!("failed to build ref for {}: {e}", pkg.id);
                continue;
            }
        };

        let result = match resolve::resolve_package(&pkg_ref, &opts.cache_dir) {
            Ok(r) => r,
            Err(e) => {
                log::warn!("failed to check {}: {e}", pkg.id);
                continue;
            }
        };

        if result.resolved.hash != pkg.commit {
            outdated.push(OutdatedPackage {
                id: pkg.id.clone(),
                current_version: pkg.version.clone(),
                latest_version: result.resolved.version,
                channel: pkg.channel,
            });
        }
    }

    Ok(outdated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packages::store::InstalledPackage;
    use std::path::Path;
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

    fn head_commit(dir: &Path) -> String {
        let out = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(dir)
            .output()
            .unwrap();
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    }

    const MANIFEST: &str = "package:\n  id: github.com/Test/Repo\n  description: \"Test\"\n";

    fn init_repo_on_branch(branch: &str) -> TempDir {
        let dir = TempDir::new().unwrap();
        run_git(dir.path(), &["init", "-b", branch]);
        run_git(dir.path(), &["config", "user.email", "t@t.com"]);
        run_git(dir.path(), &["config", "user.name", "T"]);
        dir
    }

    fn commit_manifest(repo: &Path, msg: &str) {
        std::fs::write(repo.join("edgetx.yml"), MANIFEST).unwrap();
        run_git(repo, &["add", "-A"]);
        run_git(repo, &["commit", "-m", msg]);
    }

    fn make_sd(repos_before_sd: bool) -> TempDir {
        let _ = repos_before_sd;
        let sd = TempDir::new().unwrap();
        std::fs::create_dir_all(sd.path().join("RADIO")).unwrap();
        sd
    }

    /// Stamp an `InstalledPackage` directly into the store, skipping the
    /// full install flow. We're testing the resolve path, not file copy.
    fn install_pkg(
        sd_root: &Path,
        id: &str,
        channel: Channel,
        version: &str,
        commit: &str,
        origin: Option<String>,
    ) {
        let mut store = PackageStore::load(sd_root.to_path_buf()).unwrap();
        store.add(InstalledPackage {
            id: id.into(),
            name: String::new(),
            channel,
            version: version.into(),
            commit: commit.into(),
            origin,
            variant: None,
            local_path: None,
            paths: vec![],
            dev: false,
        });
        store.save().unwrap();
    }

    fn file_url(repo: &Path) -> String {
        format!("file://{}", repo.display())
    }

    fn check(sd: &Path, cache: &Path) -> Vec<OutdatedPackage> {
        check_outdated(OutdatedOptions {
            sd_root: sd.to_path_buf(),
            cache_dir: cache.to_path_buf(),
        })
        .unwrap()
    }

    #[test]
    fn test_branch_up_to_date_returns_empty() {
        let repo = init_repo_on_branch("main");
        run_git(repo.path(), &["checkout", "-b", "feature"]);
        commit_manifest(repo.path(), "feature initial");
        let commit = head_commit(repo.path());

        let sd = make_sd(false);
        let cache = TempDir::new().unwrap();
        install_pkg(
            sd.path(),
            &file_url(repo.path()),
            Channel::Branch,
            "feature",
            &commit,
            None,
        );

        assert!(check(sd.path(), cache.path()).is_empty());
    }

    #[test]
    fn test_branch_with_new_commits_reports_outdated() {
        let repo = init_repo_on_branch("main");
        run_git(repo.path(), &["checkout", "-b", "feature"]);
        commit_manifest(repo.path(), "feature v1");
        let installed_commit = head_commit(repo.path());

        // Second commit on the branch, after install
        std::fs::write(repo.path().join("extra.lua"), "-- new").unwrap();
        run_git(repo.path(), &["add", "-A"]);
        run_git(repo.path(), &["commit", "-m", "feature v2"]);
        let new_commit = head_commit(repo.path());
        assert_ne!(installed_commit, new_commit);

        let sd = make_sd(false);
        let cache = TempDir::new().unwrap();
        install_pkg(
            sd.path(),
            &file_url(repo.path()),
            Channel::Branch,
            "feature",
            &installed_commit,
            None,
        );

        let result = check(sd.path(), cache.path());
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].latest_version, "feature");
        assert_eq!(result[0].current_version, "feature");
        assert_eq!(result[0].channel, Channel::Branch);
    }

    /// Regression for the real-world bug: user installed from a branch, but
    /// the repo's "latest" semver tag points to an old commit that doesn't
    /// have the EdgeTX manifest yet. Pre-fix, `outdated` dropped the branch
    /// name and resolved to that tag, which failed manifest load and was
    /// silently swallowed as "all up to date". Post-fix, the branch name is
    /// preserved and resolution hits the branch HEAD.
    #[test]
    fn test_branch_with_stale_semver_tag_regression() {
        let repo = init_repo_on_branch("main");
        // main has NO manifest — just some unrelated file.
        std::fs::write(repo.path().join("README"), "old project").unwrap();
        run_git(repo.path(), &["add", "-A"]);
        run_git(repo.path(), &["commit", "-m", "legacy"]);
        run_git(repo.path(), &["tag", "v0.1.0"]);

        run_git(repo.path(), &["checkout", "-b", "feature"]);
        commit_manifest(repo.path(), "add edgetx packaging");
        let branch_head = head_commit(repo.path());

        let sd = make_sd(false);
        let cache = TempDir::new().unwrap();
        install_pkg(
            sd.path(),
            &file_url(repo.path()),
            Channel::Branch,
            "feature",
            &branch_head,
            None,
        );

        // Post-fix: branch HEAD matches installed commit → empty (not outdated)
        // and crucially: no warning about missing manifest at the v0.1.0 commit.
        assert!(check(sd.path(), cache.path()).is_empty());
    }

    #[test]
    fn test_semver_tag_checks_for_newer_semver_tag() {
        let repo = init_repo_on_branch("main");
        commit_manifest(repo.path(), "v1");
        run_git(repo.path(), &["tag", "v1.0.0"]);
        let v1_commit = head_commit(repo.path());

        std::fs::write(repo.path().join("extra.lua"), "-- v2").unwrap();
        run_git(repo.path(), &["add", "-A"]);
        run_git(repo.path(), &["commit", "-m", "v2"]);
        run_git(repo.path(), &["tag", "v2.0.0"]);

        let sd = make_sd(false);
        let cache = TempDir::new().unwrap();
        install_pkg(
            sd.path(),
            &file_url(repo.path()),
            Channel::Tag,
            "v1.0.0",
            &v1_commit,
            None,
        );

        let result = check(sd.path(), cache.path());
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].current_version, "v1.0.0");
        assert_eq!(result[0].latest_version, "v2.0.0");
    }

    #[test]
    fn test_non_semver_tag_up_to_date_when_unchanged() {
        let repo = init_repo_on_branch("main");
        commit_manifest(repo.path(), "stable release");
        run_git(repo.path(), &["tag", "stable"]);
        let commit = head_commit(repo.path());

        let sd = make_sd(false);
        let cache = TempDir::new().unwrap();
        install_pkg(
            sd.path(),
            &file_url(repo.path()),
            Channel::Tag,
            "stable",
            &commit,
            None,
        );

        assert!(check(sd.path(), cache.path()).is_empty());
    }

    #[test]
    fn test_non_semver_tag_tracks_same_tag_ref() {
        let repo = init_repo_on_branch("main");
        commit_manifest(repo.path(), "stable v1");
        run_git(repo.path(), &["tag", "stable"]);
        let installed_commit = head_commit(repo.path());

        // Move the `stable` tag to a new commit
        std::fs::write(repo.path().join("extra.lua"), "-- moved").unwrap();
        run_git(repo.path(), &["add", "-A"]);
        run_git(repo.path(), &["commit", "-m", "stable v2"]);
        run_git(repo.path(), &["tag", "-f", "stable"]);
        let new_commit = head_commit(repo.path());
        assert_ne!(installed_commit, new_commit);

        let sd = make_sd(false);
        let cache = TempDir::new().unwrap();
        install_pkg(
            sd.path(),
            &file_url(repo.path()),
            Channel::Tag,
            "stable",
            &installed_commit,
            None,
        );

        let result = check(sd.path(), cache.path());
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].current_version, "stable");
        assert_eq!(
            result[0].latest_version, "stable",
            "non-semver tag should re-resolve to same tag name"
        );
    }

    #[test]
    fn test_pinned_commit_is_skipped() {
        let repo = init_repo_on_branch("main");
        commit_manifest(repo.path(), "v1");
        let pinned = head_commit(repo.path());

        // Move main forward — a pinned commit install should still be "up to
        // date" because pins are explicit.
        std::fs::write(repo.path().join("extra.lua"), "-- later").unwrap();
        run_git(repo.path(), &["add", "-A"]);
        run_git(repo.path(), &["commit", "-m", "later"]);

        let sd = make_sd(false);
        let cache = TempDir::new().unwrap();
        install_pkg(
            sd.path(),
            &file_url(repo.path()),
            Channel::Commit,
            &pinned,
            &pinned,
            None,
        );

        assert!(check(sd.path(), cache.path()).is_empty());
    }

    #[test]
    fn test_fork_origin_fetches_from_origin() {
        let upstream = init_repo_on_branch("main");
        run_git(upstream.path(), &["checkout", "-b", "feature"]);
        commit_manifest(upstream.path(), "upstream feature");
        let upstream_commit = head_commit(upstream.path());

        // Fork starts with the same commit, then diverges.
        let fork = init_repo_on_branch("main");
        run_git(fork.path(), &["checkout", "-b", "feature"]);
        commit_manifest(fork.path(), "fork feature");
        std::fs::write(fork.path().join("extra.lua"), "-- fork-only").unwrap();
        run_git(fork.path(), &["add", "-A"]);
        run_git(fork.path(), &["commit", "-m", "fork work"]);
        let fork_commit = head_commit(fork.path());
        assert_ne!(upstream_commit, fork_commit);

        let sd = make_sd(false);
        let cache = TempDir::new().unwrap();
        // User installed from the fork, but the package id reflects the
        // upstream canonical. Outdated should resolve via `origin` and
        // compare against the fork's HEAD.
        install_pkg(
            sd.path(),
            &file_url(upstream.path()),
            Channel::Branch,
            "feature",
            &fork_commit,
            Some(file_url(fork.path())),
        );

        // Pre-fix (and post-fix): comparing against fork HEAD == installed
        // commit → empty. If outdated ignored `origin` and hit upstream, the
        // commits would differ and it would falsely report outdated.
        assert!(check(sd.path(), cache.path()).is_empty());
    }

    #[test]
    fn test_deleted_upstream_branch_is_skipped() {
        let repo = init_repo_on_branch("main");
        commit_manifest(repo.path(), "main manifest");
        run_git(repo.path(), &["checkout", "-b", "feature"]);
        std::fs::write(repo.path().join("extra.lua"), "-- branch").unwrap();
        run_git(repo.path(), &["add", "-A"]);
        run_git(repo.path(), &["commit", "-m", "feature"]);
        let branch_commit = head_commit(repo.path());

        let sd = make_sd(false);
        let cache = TempDir::new().unwrap();
        install_pkg(
            sd.path(),
            &file_url(repo.path()),
            Channel::Branch,
            "feature",
            &branch_commit,
            None,
        );

        // Delete the branch from the remote after install.
        run_git(repo.path(), &["checkout", "main"]);
        run_git(repo.path(), &["branch", "-D", "feature"]);

        // Expected: warn-and-continue, not fatal. The outdated list is empty
        // because the one package could not be resolved.
        assert!(check(sd.path(), cache.path()).is_empty());
    }
}
