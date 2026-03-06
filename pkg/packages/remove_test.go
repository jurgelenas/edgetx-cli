package packages

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/jurgelenas/edgetx-cli/pkg/repository"
	"github.com/stretchr/testify/assert"
)

func TestRemove_Success(t *testing.T) {
	project := createLocalProject(t, false)
	sdCard := t.TempDir()

	ref, _ := repository.ParsePackageRef(project)
	_, err := Install(InstallOptions{SDRoot: sdCard, Ref: ref})
	assert.NoError(t, err)

	result, err := Remove(RemoveOptions{SDRoot: sdCard, Query: "local::" + project})
	assert.NoError(t, err)
	assert.Equal(t, "test-tool", result.Package.Name)

	// Files should be deleted.
	assert.NoFileExists(t, filepath.Join(sdCard, "SCRIPTS/TOOLS/MyTool/main.lua"))

	// State should be updated.
	state, _ := LoadState(sdCard)
	assert.Empty(t, state.Packages)
}

func TestRemove_NotInstalled(t *testing.T) {
	sdCard := t.TempDir()

	_, err := Remove(RemoveOptions{SDRoot: sdCard, Query: "nonexistent"})
	assert.Error(t, err)
	assert.Contains(t, err.Error(), "not found")
}

func TestRemove_DryRun(t *testing.T) {
	project := createLocalProject(t, false)
	sdCard := t.TempDir()

	ref, _ := repository.ParsePackageRef(project)
	_, err := Install(InstallOptions{SDRoot: sdCard, Ref: ref})
	assert.NoError(t, err)

	_, err = Remove(RemoveOptions{SDRoot: sdCard, Query: "local::" + project, DryRun: true})
	assert.NoError(t, err)

	// Files should NOT be deleted.
	assert.FileExists(t, filepath.Join(sdCard, "SCRIPTS/TOOLS/MyTool/main.lua"))

	// State should NOT be updated.
	state, _ := LoadState(sdCard)
	assert.Len(t, state.Packages, 1)
}

func TestRemove_CleansEmptyDirs(t *testing.T) {
	project := createLocalProject(t, false)
	sdCard := t.TempDir()

	ref, _ := repository.ParsePackageRef(project)
	_, err := Install(InstallOptions{SDRoot: sdCard, Ref: ref})
	assert.NoError(t, err)

	_, err = Remove(RemoveOptions{SDRoot: sdCard, Query: "local::" + project})
	assert.NoError(t, err)

	// Empty parent dirs should be cleaned up.
	_, err = os.Stat(filepath.Join(sdCard, "SCRIPTS/TOOLS"))
	assert.True(t, os.IsNotExist(err), "SCRIPTS/TOOLS should be removed as empty dir")
}

func TestRemove_PreservesOtherPackageFiles(t *testing.T) {
	project := createLocalProject(t, false)
	sdCard := t.TempDir()

	ref, _ := repository.ParsePackageRef(project)
	_, err := Install(InstallOptions{SDRoot: sdCard, Ref: ref})
	assert.NoError(t, err)

	// Create another package's file in a different location.
	otherPath := filepath.Join(sdCard, "WIDGETS/Other/main.lua")
	os.MkdirAll(filepath.Dir(otherPath), 0o755)
	os.WriteFile(otherPath, []byte("-- other"), 0o644)

	state, _ := LoadState(sdCard)
	state.Add(InstalledPackage{
		Source: "Other/Pkg",
		Name:   "other",
		Paths:  []string{"WIDGETS/Other"},
	})
	state.Save(sdCard)

	_, err = Remove(RemoveOptions{SDRoot: sdCard, Query: "local::" + project})
	assert.NoError(t, err)

	// Other package's files should still exist.
	assert.FileExists(t, otherPath)
}

func TestRemove_PreservesUserDataFiles(t *testing.T) {
	project := createLocalProject(t, false)
	sdCard := t.TempDir()

	ref, _ := repository.ParsePackageRef(project)
	_, err := Install(InstallOptions{SDRoot: sdCard, Ref: ref})
	assert.NoError(t, err)

	// Simulate a user-created data file in the same directory as installed files.
	presetsPath := filepath.Join(sdCard, "SCRIPTS/TOOLS/MyTool/presets.txt")
	os.WriteFile(presetsPath, []byte("user data"), 0o644)

	_, err = Remove(RemoveOptions{SDRoot: sdCard, Query: "local::" + project})
	assert.NoError(t, err)

	// Package file should be gone.
	assert.NoFileExists(t, filepath.Join(sdCard, "SCRIPTS/TOOLS/MyTool/main.lua"))

	// User data file should be preserved.
	assert.FileExists(t, presetsPath)
}
