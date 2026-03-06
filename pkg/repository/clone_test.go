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

// createBareTestRepo creates a local bare repo with an edgetx.toml manifest
// for use as a clone source.
func createBareTestRepo(t *testing.T, manifestContent string) string {
	t.Helper()

	// Create a normal repo first, then we'll use it as a "remote".
	dir := t.TempDir()
	repo, err := git.PlainInit(dir, false)
	assert.NoError(t, err)

	// Write manifest.
	assert.NoError(t, os.WriteFile(filepath.Join(dir, "edgetx.toml"), []byte(manifestContent), 0o644))

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
	manifest := `[package]
name = "test-tool"
description = "A test tool"

[[tools]]
name = "MyTool"
path = "SCRIPTS/TOOLS/MyTool"
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
	result, err := loadFromDir(repoDir, rv)
	assert.NoError(t, err)
	assert.Equal(t, "test-tool", result.Manifest.Package.Name)
	assert.Equal(t, "tag", result.Resolved.Channel)
	assert.Equal(t, "v1.0.0", result.Resolved.Version)

	_ = ref // suppress unused
}

func TestCloneAndCheckout_NoManifest(t *testing.T) {
	dir := t.TempDir()
	// No edgetx.toml — should error.

	rv := ResolvedVersion{Channel: "branch", Version: "main"}
	_, err := loadFromDir(dir, rv)
	assert.Error(t, err)
	assert.Contains(t, err.Error(), "edgetx.toml")
}

func TestCloneAndCheckout_InvalidManifest(t *testing.T) {
	dir := t.TempDir()
	os.WriteFile(filepath.Join(dir, "edgetx.toml"), []byte("{{invalid"), 0o644)

	rv := ResolvedVersion{Channel: "branch", Version: "main"}
	_, err := loadFromDir(dir, rv)
	assert.Error(t, err)
}

func TestCloneAndCheckout_WithSourceDir(t *testing.T) {
	dir := t.TempDir()
	os.MkdirAll(filepath.Join(dir, "src/SCRIPTS/TOOLS/MyTool"), 0o755)
	os.WriteFile(filepath.Join(dir, "src/SCRIPTS/TOOLS/MyTool/main.lua"), []byte("-- tool"), 0o644)

	manifest := `[package]
name = "test-tool"
description = "Test"
source_dir = "src"

[[tools]]
name = "MyTool"
path = "SCRIPTS/TOOLS/MyTool"
`
	os.WriteFile(filepath.Join(dir, "edgetx.toml"), []byte(manifest), 0o644)

	rv := ResolvedVersion{Channel: "branch", Version: "main"}
	result, err := loadFromDir(dir, rv)
	assert.NoError(t, err)
	assert.Equal(t, filepath.Join(dir, "src"), result.Manifest.SourceRoot(dir))
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
