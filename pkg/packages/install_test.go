package packages

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/jurgelenas/edgetx-cli/pkg/repository"
	"github.com/stretchr/testify/assert"
)

// createLocalProject creates a local project directory with edgetx.yml and content.
func createLocalProject(t *testing.T, binary bool) string {
	t.Helper()
	dir := t.TempDir()

	binaryField := ""
	if binary {
		binaryField = "\n  binary: true"
	}

	manifest := `package:
  name: test-tool
  description: A test tool` + binaryField + `

tools:
  - name: MyTool
    path: SCRIPTS/TOOLS/MyTool
`
	os.WriteFile(filepath.Join(dir, "edgetx.yml"), []byte(manifest), 0o644)
	os.MkdirAll(filepath.Join(dir, "SCRIPTS/TOOLS/MyTool"), 0o755)
	os.WriteFile(filepath.Join(dir, "SCRIPTS/TOOLS/MyTool/main.lua"), []byte("-- tool"), 0o644)
	os.WriteFile(filepath.Join(dir, "SCRIPTS/TOOLS/MyTool/main.luac"), []byte("compiled"), 0o644)

	return dir
}

func TestInstall_LocalDir(t *testing.T) {
	project := createLocalProject(t, false)
	sdCard := t.TempDir()

	ref, _ := repository.ParsePackageRef(project)

	result, err := Install(InstallOptions{
		SDRoot: sdCard,
		Ref:    ref,
	})

	assert.NoError(t, err)
	assert.Equal(t, "test-tool", result.Package.Name)
	assert.Equal(t, "local", result.Package.Channel)
	assert.Equal(t, 1, result.FilesCopied, ".luac should be excluded")
	assert.FileExists(t, filepath.Join(sdCard, "SCRIPTS/TOOLS/MyTool/main.lua"))
	assert.NoFileExists(t, filepath.Join(sdCard, "SCRIPTS/TOOLS/MyTool/main.luac"))
}

func TestInstall_StateUpdated(t *testing.T) {
	project := createLocalProject(t, false)
	sdCard := t.TempDir()

	ref, _ := repository.ParsePackageRef(project)

	_, err := Install(InstallOptions{SDRoot: sdCard, Ref: ref})
	assert.NoError(t, err)

	state, err := LoadState(sdCard)
	assert.NoError(t, err)
	assert.Len(t, state.Packages, 1)
	assert.Equal(t, "local::"+project, state.Packages[0].Source)
	assert.Equal(t, "test-tool", state.Packages[0].Name)
	assert.Equal(t, "local", state.Packages[0].Channel)
	assert.Equal(t, []string{"SCRIPTS/TOOLS/MyTool"}, state.Packages[0].Paths)
}

func TestInstall_PathConflict(t *testing.T) {
	project := createLocalProject(t, false)
	sdCard := t.TempDir()

	// Pre-populate state with conflicting package.
	state := &State{
		Packages: []InstalledPackage{
			{Source: "Other/Pkg", Name: "other", Paths: []string{"SCRIPTS/TOOLS/MyTool"}},
		},
	}
	state.Save(sdCard)

	ref, _ := repository.ParsePackageRef(project)

	_, err := Install(InstallOptions{SDRoot: sdCard, Ref: ref})
	assert.Error(t, err)
	assert.Contains(t, err.Error(), "conflict")
}

func TestInstall_LocalReinstall(t *testing.T) {
	project := createLocalProject(t, false)
	sdCard := t.TempDir()

	ref, _ := repository.ParsePackageRef(project)

	_, err := Install(InstallOptions{SDRoot: sdCard, Ref: ref})
	assert.NoError(t, err)

	// Update source file.
	os.WriteFile(filepath.Join(project, "SCRIPTS/TOOLS/MyTool/main.lua"), []byte("-- updated"), 0o644)

	// Local reinstall should succeed.
	result, err := Install(InstallOptions{SDRoot: sdCard, Ref: ref})
	assert.NoError(t, err)
	assert.Equal(t, 1, result.FilesCopied)

	// Verify updated content.
	content, _ := os.ReadFile(filepath.Join(sdCard, "SCRIPTS/TOOLS/MyTool/main.lua"))
	assert.Equal(t, "-- updated", string(content))

	// State should still have exactly 1 package.
	state, _ := LoadState(sdCard)
	assert.Len(t, state.Packages, 1)
}


func TestInstall_DryRun(t *testing.T) {
	project := createLocalProject(t, false)
	sdCard := t.TempDir()

	ref, _ := repository.ParsePackageRef(project)

	result, err := Install(InstallOptions{SDRoot: sdCard, Ref: ref, DryRun: true})
	assert.NoError(t, err)
	assert.Equal(t, 0, result.FilesCopied)

	// No files should be on disk.
	assert.NoFileExists(t, filepath.Join(sdCard, "SCRIPTS/TOOLS/MyTool/main.lua"))

	// State should not be updated.
	state, _ := LoadState(sdCard)
	assert.Empty(t, state.Packages)
}

func TestInstall_BinaryPackage(t *testing.T) {
	project := createLocalProject(t, true)
	sdCard := t.TempDir()

	ref, _ := repository.ParsePackageRef(project)

	result, err := Install(InstallOptions{SDRoot: sdCard, Ref: ref})
	assert.NoError(t, err)
	assert.Equal(t, 2, result.FilesCopied, ".luac should be included for binary packages")
	assert.FileExists(t, filepath.Join(sdCard, "SCRIPTS/TOOLS/MyTool/main.lua"))
	assert.FileExists(t, filepath.Join(sdCard, "SCRIPTS/TOOLS/MyTool/main.luac"))
}

func TestInstall_SourcePackage(t *testing.T) {
	project := createLocalProject(t, false)
	sdCard := t.TempDir()

	ref, _ := repository.ParsePackageRef(project)

	result, err := Install(InstallOptions{SDRoot: sdCard, Ref: ref})
	assert.NoError(t, err)
	assert.Equal(t, 1, result.FilesCopied, ".luac should be excluded for source packages")
	assert.NoFileExists(t, filepath.Join(sdCard, "SCRIPTS/TOOLS/MyTool/main.luac"))
}

func createProjectWithMinVersion(t *testing.T, minVersion string) string {
	t.Helper()
	dir := t.TempDir()

	manifest := `package:
  name: test-tool
  description: A test tool
  min_edgetx_version: "` + minVersion + `"

tools:
  - name: MyTool
    path: SCRIPTS/TOOLS/MyTool
`
	os.WriteFile(filepath.Join(dir, "edgetx.yml"), []byte(manifest), 0o644)
	os.MkdirAll(filepath.Join(dir, "SCRIPTS/TOOLS/MyTool"), 0o755)
	os.WriteFile(filepath.Join(dir, "SCRIPTS/TOOLS/MyTool/main.lua"), []byte("-- tool"), 0o644)

	return dir
}

func createRadioYML(t *testing.T, sdCard string, semver string) {
	t.Helper()
	os.MkdirAll(filepath.Join(sdCard, "RADIO"), 0o755)
	os.WriteFile(filepath.Join(sdCard, "RADIO/radio.yml"), []byte("semver: "+semver+"\n"), 0o644)
}

func TestInstall_MinVersionMet(t *testing.T) {
	project := createProjectWithMinVersion(t, "2.12.0")
	sdCard := t.TempDir()
	createRadioYML(t, sdCard, "2.13.0")

	ref, _ := repository.ParsePackageRef(project)

	result, err := Install(InstallOptions{SDRoot: sdCard, Ref: ref})
	assert.NoError(t, err)
	assert.Equal(t, "test-tool", result.Package.Name)
}

func TestInstall_MinVersionNotMet(t *testing.T) {
	project := createProjectWithMinVersion(t, "2.14.0")
	sdCard := t.TempDir()
	createRadioYML(t, sdCard, "2.12.0")

	ref, _ := repository.ParsePackageRef(project)

	_, err := Install(InstallOptions{SDRoot: sdCard, Ref: ref})
	assert.Error(t, err)
	assert.Contains(t, err.Error(), "does not meet minimum")
}

func TestInstall_MinVersionNoRadioYML(t *testing.T) {
	project := createProjectWithMinVersion(t, "2.12.0")
	sdCard := t.TempDir()

	ref, _ := repository.ParsePackageRef(project)

	result, err := Install(InstallOptions{SDRoot: sdCard, Ref: ref})
	assert.NoError(t, err)
	assert.Equal(t, "test-tool", result.Package.Name)
}

func TestInstall_WithExclude(t *testing.T) {
	dir := t.TempDir()

	manifest := `package:
  name: test-tool
  description: test

tools:
  - name: MyTool
    path: SCRIPTS/TOOLS/MyTool
    exclude:
      - config.txt
`
	os.WriteFile(filepath.Join(dir, "edgetx.yml"), []byte(manifest), 0o644)
	os.MkdirAll(filepath.Join(dir, "SCRIPTS/TOOLS/MyTool"), 0o755)
	os.WriteFile(filepath.Join(dir, "SCRIPTS/TOOLS/MyTool/main.lua"), []byte("-- tool"), 0o644)
	os.WriteFile(filepath.Join(dir, "SCRIPTS/TOOLS/MyTool/config.txt"), []byte("config"), 0o644)

	sdCard := t.TempDir()
	ref, _ := repository.ParsePackageRef(dir)

	result, err := Install(InstallOptions{SDRoot: sdCard, Ref: ref})
	assert.NoError(t, err)
	assert.Equal(t, 1, result.FilesCopied)
	assert.FileExists(t, filepath.Join(sdCard, "SCRIPTS/TOOLS/MyTool/main.lua"))
	assert.NoFileExists(t, filepath.Join(sdCard, "SCRIPTS/TOOLS/MyTool/config.txt"))
}
