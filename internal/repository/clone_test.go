package repository

import (
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/go-git/go-git/v5"
	"github.com/go-git/go-git/v5/plumbing/object"
	"github.com/stretchr/testify/assert"
)

// createBareTestRepo creates a local bare repo with an edgetx.yml manifest
// for use as a clone source.
func createBareTestRepo(t *testing.T, manifestContent string) string {
	t.Helper()

	// Create a normal repo first, then we'll use it as a "remote".
	dir := t.TempDir()
	repo, err := git.PlainInit(dir, false)
	assert.NoError(t, err)

	// Write manifest.
	assert.NoError(t, os.WriteFile(filepath.Join(dir, "edgetx.yml"), []byte(manifestContent), 0o644))

	// Create content directories.
	os.MkdirAll(filepath.Join(dir, "SCRIPTS/TOOLS/MyTool"), 0o755)
	os.WriteFile(filepath.Join(dir, "SCRIPTS/TOOLS/MyTool/main.lua"), []byte("-- tool"), 0o644)

	wt, err := repo.Worktree()
	assert.NoError(t, err)

	wt.Add(".")
	_, err = wt.Commit("initial", &git.CommitOptions{
		Author: &object.Signature{
			Name:  "Test",
			Email: "test@test.com",
			When:  time.Now(),
		},
	})
	assert.NoError(t, err)

	return dir
}

func TestCloneAndCheckout_Tag(t *testing.T) {
	manifest := `package:
  name: test-tool
  description: A test tool

tools:
  - name: MyTool
    path: SCRIPTS/TOOLS/MyTool
`
	repoDir := createBareTestRepo(t, manifest)
	repo, _ := git.PlainOpen(repoDir)
	head, _ := repo.Head()
	repo.CreateTag("v1.0.0", head.Hash(), nil)

	ref := PackageRef{
		IsLocal:   true,
		LocalPath: repoDir,
	}

	// For local testing, we test loadFromDir directly since CloneAndCheckout
	// needs a real remote URL.
	rv := ResolvedVersion{Channel: "tag", Version: "v1.0.0", Hash: head.Hash()}
	result, err := loadFromDir(repoDir, "", rv)
	assert.NoError(t, err)
	assert.Equal(t, "test-tool", result.Manifest.Package.Name)
	assert.Equal(t, "tag", result.Resolved.Channel)
	assert.Equal(t, "v1.0.0", result.Resolved.Version)

	_ = ref // suppress unused
}

func TestCloneAndCheckout_NoManifest(t *testing.T) {
	dir := t.TempDir()
	// No edgetx.yml — should error.

	rv := ResolvedVersion{Channel: "branch", Version: "main"}
	_, err := loadFromDir(dir, "", rv)
	assert.Error(t, err)
	assert.Contains(t, err.Error(), "edgetx.yml")
}

func TestCloneAndCheckout_InvalidManifest(t *testing.T) {
	dir := t.TempDir()
	os.WriteFile(filepath.Join(dir, "edgetx.yml"), []byte(":\n  :\n    - [invalid"), 0o644)

	rv := ResolvedVersion{Channel: "branch", Version: "main"}
	_, err := loadFromDir(dir, "", rv)
	assert.Error(t, err)
}

func TestCloneAndCheckout_WithSourceDir(t *testing.T) {
	dir := t.TempDir()
	os.MkdirAll(filepath.Join(dir, "src/SCRIPTS/TOOLS/MyTool"), 0o755)
	os.WriteFile(filepath.Join(dir, "src/SCRIPTS/TOOLS/MyTool/main.lua"), []byte("-- tool"), 0o644)

	manifest := `package:
  name: test-tool
  description: Test
  source_dir: src

tools:
  - name: MyTool
    path: SCRIPTS/TOOLS/MyTool
`
	os.WriteFile(filepath.Join(dir, "edgetx.yml"), []byte(manifest), 0o644)

	rv := ResolvedVersion{Channel: "branch", Version: "main"}
	result, err := loadFromDir(dir, "", rv)
	assert.NoError(t, err)
	assert.Equal(t, []string{filepath.Join(dir, "src")}, result.Manifest.SourceRoots(dir))
}

func TestLoadFromDir_WithFileSubPath(t *testing.T) {
	dir := t.TempDir()
	os.MkdirAll(filepath.Join(dir, "SCRIPTS/TOOLS/MyTool"), 0o755)
	os.WriteFile(filepath.Join(dir, "SCRIPTS/TOOLS/MyTool/main.lua"), []byte("-- tool"), 0o644)

	manifest := `package:
  name: custom-variant
  description: Test

tools:
  - name: MyTool
    path: SCRIPTS/TOOLS/MyTool
`
	os.WriteFile(filepath.Join(dir, "edgetx.custom.yml"), []byte(manifest), 0o644)

	rv := ResolvedVersion{Channel: "tag", Version: "v1.0.0"}
	result, err := loadFromDir(dir, "edgetx.custom.yml", rv)
	assert.NoError(t, err)
	assert.Equal(t, "custom-variant", result.Manifest.Package.Name)
	assert.Equal(t, dir, result.ManifestDir)
}

func TestLoadFromDir_WithDirSubPath(t *testing.T) {
	dir := t.TempDir()
	os.MkdirAll(filepath.Join(dir, "sub/SCRIPTS/TOOLS/MyTool"), 0o755)
	os.WriteFile(filepath.Join(dir, "sub/SCRIPTS/TOOLS/MyTool/main.lua"), []byte("-- tool"), 0o644)

	manifest := `package:
  name: sub-pkg
  description: Test

tools:
  - name: MyTool
    path: SCRIPTS/TOOLS/MyTool
`
	os.WriteFile(filepath.Join(dir, "sub/edgetx.yml"), []byte(manifest), 0o644)

	rv := ResolvedVersion{Channel: "branch", Version: "main"}
	result, err := loadFromDir(dir, "sub", rv)
	assert.NoError(t, err)
	assert.Equal(t, "sub-pkg", result.Manifest.Package.Name)
	assert.Equal(t, filepath.Join(dir, "sub"), result.ManifestDir)
}

func TestResolveLatestRemoteTag_FindsLatestSemver(t *testing.T) {
	manifest := `package:
  name: test-tool
  description: A test tool

tools:
  - name: MyTool
    path: SCRIPTS/TOOLS/MyTool
`
	repoDir := createBareTestRepo(t, manifest)
	repo, _ := git.PlainOpen(repoDir)
	head, _ := repo.Head()

	repo.CreateTag("v1.0.0", head.Hash(), nil)
	repo.CreateTag("v2.0.0", head.Hash(), nil)
	repo.CreateTag("v1.5.0", head.Hash(), nil)
	repo.CreateTag("nightly", head.Hash(), nil)

	tag, err := resolveLatestRemoteTag(repoDir)
	assert.NoError(t, err)
	assert.Equal(t, "v2.0.0", tag)
}

func TestResolveLatestRemoteTag_NoSemverTags(t *testing.T) {
	manifest := `package:
  name: test-tool
  description: A test tool

tools:
  - name: MyTool
    path: SCRIPTS/TOOLS/MyTool
`
	repoDir := createBareTestRepo(t, manifest)
	repo, _ := git.PlainOpen(repoDir)
	head, _ := repo.Head()

	repo.CreateTag("nightly", head.Hash(), nil)
	repo.CreateTag("latest", head.Hash(), nil)

	tag, err := resolveLatestRemoteTag(repoDir)
	assert.NoError(t, err)
	assert.Equal(t, "", tag)
}

func TestResolveLatestRemoteTag_NoTags(t *testing.T) {
	manifest := `package:
  name: test-tool
  description: A test tool

tools:
  - name: MyTool
    path: SCRIPTS/TOOLS/MyTool
`
	repoDir := createBareTestRepo(t, manifest)

	tag, err := resolveLatestRemoteTag(repoDir)
	assert.NoError(t, err)
	assert.Equal(t, "", tag)
}

func TestCloneAndCheckout_NoVersion_UsesLatestTag(t *testing.T) {
	manifest := `package:
  name: test-tool
  description: A test tool

tools:
  - name: MyTool
    path: SCRIPTS/TOOLS/MyTool
`
	repoDir := createBareTestRepo(t, manifest)
	repo, _ := git.PlainOpen(repoDir)
	head, _ := repo.Head()

	repo.CreateTag("v1.0.0", head.Hash(), nil)
	repo.CreateTag("v2.0.0", head.Hash(), nil)

	ref := PackageRef{
		Owner:   "test",
		Repo:    "test",
		Version: "",
	}
	// Override CloneURL by using a local path as the remote URL.
	// We need to call CloneAndCheckout with a ref that points to our local repo.
	// Since CloneURL() builds an https URL, we test resolveLatestRemoteTag + ResolveVersion
	// together via the local repo directly.
	latestTag, err := resolveLatestRemoteTag(repoDir)
	assert.NoError(t, err)
	assert.Equal(t, "v2.0.0", latestTag)

	// Verify the resolved version from the repo matches.
	rv, err := ResolveVersion(repo, "")
	assert.NoError(t, err)
	assert.Equal(t, "tag", rv.Channel)
	assert.Equal(t, "v2.0.0", rv.Version)

	_ = ref
}

func TestCleanup(t *testing.T) {
	dir := t.TempDir()
	subDir := filepath.Join(dir, "test-clone")
	os.MkdirAll(subDir, 0o755)
	os.WriteFile(filepath.Join(subDir, "file.txt"), []byte("test"), 0o644)

	Cleanup(subDir)
	_, err := os.Stat(subDir)
	assert.True(t, os.IsNotExist(err))
}
