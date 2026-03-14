package packages

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/jurgelenas/edgetx-cli/internal/repository"
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

func TestRemove_SubPathSource(t *testing.T) {
	sdCard := t.TempDir()

	// Set up state with a subpath source (as produced by `pkg install --path`).
	state := &State{
		Packages: []InstalledPackage{
			{
				Source:  "owner/repo::edgetx.c480x272.yml",
				Name:    "yaapu-color",
				Channel: "branch",
				Version: "edgetx-package",
				Paths:   []string{"WIDGETS/yaapu"},
			},
		},
	}
	assert.NoError(t, state.Save(sdCard))

	// Create the tracked file so removeTrackedFiles has something to clean.
	widgetDir := filepath.Join(sdCard, "WIDGETS/yaapu")
	os.MkdirAll(widgetDir, 0o755)
	os.WriteFile(filepath.Join(widgetDir, "main.lua"), []byte("--"), 0o644)
	assert.NoError(t, SaveFileList(sdCard, "yaapu-color", []string{"WIDGETS/yaapu/main.lua"}))

	// Query with full canonical source including ::subpath should work.
	result, err := Remove(RemoveOptions{
		SDRoot: sdCard,
		Query:  "owner/repo::edgetx.c480x272.yml",
	})
	assert.NoError(t, err)
	assert.Equal(t, "yaapu-color", result.Package.Name)

	reloaded, _ := LoadState(sdCard)
	assert.Empty(t, reloaded.Packages)
}

func TestRemove_SubPathSourceWithVersion(t *testing.T) {
	sdCard := t.TempDir()

	state := &State{
		Packages: []InstalledPackage{
			{
				Source:  "owner/repo::edgetx.c480x272.yml",
				Name:    "yaapu-color",
				Channel: "branch",
				Version: "edgetx-package",
				Paths:   []string{"WIDGETS/yaapu"},
			},
		},
	}
	assert.NoError(t, state.Save(sdCard))

	// Query with version suffix - splitQueryVersion must preserve the ::subpath.
	result, err := Remove(RemoveOptions{
		SDRoot: sdCard,
		Query:  "owner/repo::edgetx.c480x272.yml@edgetx-package",
	})
	assert.NoError(t, err)
	assert.Equal(t, "yaapu-color", result.Package.Name)
}

func TestRemove_SubPathSourceWithoutSubPath_Fails(t *testing.T) {
	sdCard := t.TempDir()

	state := &State{
		Packages: []InstalledPackage{
			{
				Source:  "owner/repo::edgetx.c480x272.yml",
				Name:    "yaapu-color",
				Channel: "branch",
				Version: "edgetx-package",
				Paths:   []string{"WIDGETS/yaapu"},
			},
		},
	}
	assert.NoError(t, state.Save(sdCard))

	// Query without ::subpath should NOT match the stored source.
	_, err := Remove(RemoveOptions{
		SDRoot: sdCard,
		Query:  "owner/repo",
	})
	assert.Error(t, err)
	assert.Contains(t, err.Error(), "not found")
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
