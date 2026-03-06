package packages

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/jurgelenas/edgetx-cli/pkg/repository"
	"github.com/stretchr/testify/assert"
)

func TestUpdate_LocalReinstall(t *testing.T) {
	project := createLocalProject(t, false)
	sdCard := t.TempDir()

	ref, _ := repository.ParsePackageRef(project)
	_, err := Install(InstallOptions{SDRoot: sdCard, Ref: ref})
	assert.NoError(t, err)

	// Modify source.
	os.WriteFile(filepath.Join(project, "SCRIPTS/TOOLS/MyTool/main.lua"), []byte("-- updated"), 0o644)

	results, err := Update(UpdateOptions{SDRoot: sdCard, Query: "local::" + project})
	assert.NoError(t, err)
	assert.Len(t, results, 1)
	assert.False(t, results[0].UpToDate)

	// Verify updated content.
	content, _ := os.ReadFile(filepath.Join(sdCard, "SCRIPTS/TOOLS/MyTool/main.lua"))
	assert.Equal(t, "-- updated", string(content))
}

func TestUpdate_NotInstalled(t *testing.T) {
	sdCard := t.TempDir()

	_, err := Update(UpdateOptions{SDRoot: sdCard, Query: "nonexistent"})
	assert.Error(t, err)
}

func TestUpdate_DryRun(t *testing.T) {
	project := createLocalProject(t, false)
	sdCard := t.TempDir()

	ref, _ := repository.ParsePackageRef(project)
	_, err := Install(InstallOptions{SDRoot: sdCard, Ref: ref})
	assert.NoError(t, err)

	// Modify source.
	os.WriteFile(filepath.Join(project, "SCRIPTS/TOOLS/MyTool/main.lua"), []byte("-- updated"), 0o644)

	results, err := Update(UpdateOptions{SDRoot: sdCard, Query: "local::" + project, DryRun: true})
	assert.NoError(t, err)
	assert.Len(t, results, 1)

	// Original file should still be there.
	content, _ := os.ReadFile(filepath.Join(sdCard, "SCRIPTS/TOOLS/MyTool/main.lua"))
	assert.Equal(t, "-- tool", string(content))
}

func TestUpdate_AllNoArgs(t *testing.T) {
	sdCard := t.TempDir()

	_, err := Update(UpdateOptions{SDRoot: sdCard})
	assert.Error(t, err)
	assert.Contains(t, err.Error(), "--all")
}

func TestUpdate_All(t *testing.T) {
	project1 := createLocalProject(t, false)
	sdCard := t.TempDir()

	ref1, _ := repository.ParsePackageRef(project1)
	_, err := Install(InstallOptions{SDRoot: sdCard, Ref: ref1})
	assert.NoError(t, err)

	// Modify source.
	os.WriteFile(filepath.Join(project1, "SCRIPTS/TOOLS/MyTool/main.lua"), []byte("-- v2"), 0o644)

	results, err := Update(UpdateOptions{SDRoot: sdCard, All: true})
	assert.NoError(t, err)
	assert.Len(t, results, 1)
}

func TestUpdate_PreservesDevFlag(t *testing.T) {
	project := createLocalProjectWithDev(t)
	sdCard := t.TempDir()

	// Install with dev.
	ref, _ := repository.ParsePackageRef(project)
	_, err := Install(InstallOptions{SDRoot: sdCard, Ref: ref, Dev: true})
	assert.NoError(t, err)

	// Modify source.
	os.WriteFile(filepath.Join(project, "SCRIPTS/TOOLS/MyTool/main.lua"), []byte("-- updated"), 0o644)

	// Update without explicitly setting dev - should preserve the stored dev=true.
	results, err := Update(UpdateOptions{SDRoot: sdCard, Query: "local::" + project})
	assert.NoError(t, err)
	assert.Len(t, results, 1)
	assert.True(t, results[0].Package.Dev)

	// Dev files should still be present.
	assert.FileExists(t, filepath.Join(sdCard, "SCRIPTS/TOOLS/DebugTool/main.lua"))
	assert.FileExists(t, filepath.Join(sdCard, "SCRIPTS/TestUtils/main.lua"))
}

func TestUpdate_DevSetOverridesStored(t *testing.T) {
	project := createLocalProjectWithDev(t)
	sdCard := t.TempDir()

	// Install with dev.
	ref, _ := repository.ParsePackageRef(project)
	_, err := Install(InstallOptions{SDRoot: sdCard, Ref: ref, Dev: true})
	assert.NoError(t, err)
	assert.FileExists(t, filepath.Join(sdCard, "SCRIPTS/TOOLS/DebugTool/main.lua"))

	// Update with DevSet=true, Dev=false - should drop dev deps.
	results, err := Update(UpdateOptions{SDRoot: sdCard, Query: "local::" + project, DevSet: true, Dev: false})
	assert.NoError(t, err)
	assert.Len(t, results, 1)
	assert.False(t, results[0].Package.Dev)

	// Dev files should be gone (old files removed, new install without dev).
	assert.NoFileExists(t, filepath.Join(sdCard, "SCRIPTS/TOOLS/DebugTool/main.lua"))
	assert.NoFileExists(t, filepath.Join(sdCard, "SCRIPTS/TestUtils/main.lua"))
	// Non-dev files should still be present.
	assert.FileExists(t, filepath.Join(sdCard, "SCRIPTS/TOOLS/MyTool/main.lua"))
}

func TestUpdate_WithoutDevExcludesDevItems(t *testing.T) {
	project := createLocalProjectWithDev(t)
	sdCard := t.TempDir()

	// Install without dev.
	ref, _ := repository.ParsePackageRef(project)
	_, err := Install(InstallOptions{SDRoot: sdCard, Ref: ref})
	assert.NoError(t, err)

	// Update without dev - should stay without dev.
	results, err := Update(UpdateOptions{SDRoot: sdCard, Query: "local::" + project})
	assert.NoError(t, err)
	assert.Len(t, results, 1)
	assert.False(t, results[0].Package.Dev)
	assert.NoFileExists(t, filepath.Join(sdCard, "SCRIPTS/TOOLS/DebugTool/main.lua"))
}

func TestUpdate_CommitPinned(t *testing.T) {
	sdCard := t.TempDir()

	// Manually set up a commit-pinned package.
	state := &State{
		Packages: []InstalledPackage{
			{
				Source:  "Org/Repo",
				Name:    "pinned",
				Channel: "commit",
				Commit:  "abc123",
				Paths:   []string{"SCRIPTS/TOOLS/Pinned"},
			},
		},
	}
	state.Save(sdCard)

	results, err := Update(UpdateOptions{SDRoot: sdCard, Query: "Org/Repo"})
	assert.NoError(t, err)
	assert.Len(t, results, 1)
	assert.True(t, results[0].UpToDate, "commit-pinned package should be up to date")
}
