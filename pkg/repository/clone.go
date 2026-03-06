package repository

import (
	"fmt"
	"os"
	"path/filepath"

	"github.com/go-git/go-git/v5"
	"github.com/go-git/go-git/v5/plumbing"
	"github.com/jurgelenas/edgetx-cli/pkg/manifest"
)

// CloneResult holds the outcome of cloning and checking out a repository.
type CloneResult struct {
	Manifest *manifest.Manifest
	Dir      string // directory containing the cloned repo
	Resolved ResolvedVersion
}

// CacheDir returns the platform-appropriate cache directory for edgetx-cli.
func CacheDir() (string, error) {
	base, err := os.UserCacheDir()
	if err != nil {
		return "", fmt.Errorf("determining cache directory: %w", err)
	}
	return filepath.Join(base, "edgetx-cli", "repos"), nil
}

// CloneAndCheckout clones a repository and checks out the specified version.
// It uses a persistent cache under the user's cache directory. If the resolved
// commit already exists in cache, the clone is skipped.
func CloneAndCheckout(ref PackageRef) (*CloneResult, error) {
	cacheBase, err := CacheDir()
	if err != nil {
		return nil, err
	}

	// Clone to a temp dir first, then move to cache once we know the commit.
	tmpDir, err := os.MkdirTemp("", "edgetx-clone-*")
	if err != nil {
		return nil, fmt.Errorf("creating temp dir: %w", err)
	}

	cloneOpts := &git.CloneOptions{
		URL:   ref.CloneURL(),
		Depth: 1,
	}

	// If a specific version is requested and looks like a tag/branch, try
	// single-branch clone for efficiency.
	if ref.Version != "" {
		cloneOpts.ReferenceName = plumbing.NewTagReferenceName(ref.Version)
		cloneOpts.SingleBranch = true
	}

	repo, err := git.PlainClone(tmpDir, false, cloneOpts)
	if err != nil && ref.Version != "" {
		// Tag clone failed, try branch.
		os.RemoveAll(tmpDir)
		tmpDir, _ = os.MkdirTemp("", "edgetx-clone-*")
		cloneOpts.ReferenceName = plumbing.NewBranchReferenceName(ref.Version)
		repo, err = git.PlainClone(tmpDir, false, cloneOpts)
	}
	if err != nil && ref.Version != "" {
		// Branch failed too, do a full clone for commit resolution.
		os.RemoveAll(tmpDir)
		tmpDir, _ = os.MkdirTemp("", "edgetx-clone-*")
		cloneOpts.ReferenceName = ""
		cloneOpts.SingleBranch = false
		cloneOpts.Depth = 0
		repo, err = git.PlainClone(tmpDir, false, cloneOpts)
	}
	if err != nil {
		os.RemoveAll(tmpDir)
		return nil, fmt.Errorf("cloning %s: %w", ref.CloneURL(), err)
	}

	resolved, err := ResolveVersion(repo, ref.Version)
	if err != nil {
		os.RemoveAll(tmpDir)
		return nil, err
	}

	// Check cache.
	cacheDir := filepath.Join(cacheBase, ref.Canonical(), resolved.Hash.String())
	if _, err := os.Stat(cacheDir); err == nil {
		// Cache hit — use cached version.
		os.RemoveAll(tmpDir)
		return loadFromDir(cacheDir, resolved)
	}

	// Checkout the resolved commit.
	wt, err := repo.Worktree()
	if err != nil {
		os.RemoveAll(tmpDir)
		return nil, fmt.Errorf("getting worktree: %w", err)
	}

	if err := wt.Checkout(&git.CheckoutOptions{Hash: resolved.Hash, Force: true}); err != nil {
		os.RemoveAll(tmpDir)
		return nil, fmt.Errorf("checking out %s: %w", resolved.Hash, err)
	}

	// Move to cache.
	if err := os.MkdirAll(filepath.Dir(cacheDir), 0o755); err != nil {
		os.RemoveAll(tmpDir)
		return nil, err
	}
	if err := os.Rename(tmpDir, cacheDir); err != nil {
		// Rename may fail across filesystems; fall back to using tmpDir directly.
		cacheDir = tmpDir
	}

	return loadFromDir(cacheDir, resolved)
}

func loadFromDir(dir string, resolved ResolvedVersion) (*CloneResult, error) {
	m, err := manifest.Load(dir)
	if err != nil {
		return nil, fmt.Errorf("repository does not contain a valid edgetx.toml: %w", err)
	}

	return &CloneResult{
		Manifest: m,
		Dir:      dir,
		Resolved: resolved,
	}, nil
}

// Cleanup removes a clone directory. Safe to call on cached dirs (no-op if the
// directory doesn't exist).
func Cleanup(dir string) {
	os.RemoveAll(dir)
}
