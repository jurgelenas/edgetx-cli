package manifest

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/stretchr/testify/assert"
)

const validTOML = `[package]
name = "test-pack"
version = "2.0.0"
description = "A test package"

[[libraries]]
name = "SharedLib"
path = "SCRIPTS/SharedLib"

[[scripts]]
name = "MyScript"
path = "SCRIPTS/MyScript"

[[tools]]
name = "MyTool"
path = "SCRIPTS/TOOLS/MyTool"

[[widgets]]
name = "MyWidget"
path = "WIDGETS/MyWidget"
depends = ["SharedLib"]
exclude = ["presets.txt"]
`

func TestLoad_ValidManifest(t *testing.T) {
	dir := t.TempDir()
	if !assert.NoError(t, os.WriteFile(filepath.Join(dir, FileName), []byte(validTOML), 0o644)) {
		return
	}

	m, err := Load(dir)
	if !assert.NoError(t, err) {
		return
	}

	assert.Equal(t, "test-pack", m.Package.Name)
	assert.Equal(t, "2.0.0", m.Package.Version)
	assert.Equal(t, "A test package", m.Package.Description)

	assert.Len(t, m.Libraries, 1)
	assert.Equal(t, "SharedLib", m.Libraries[0].Name)

	assert.Len(t, m.Scripts, 1)
	assert.Len(t, m.Tools, 1)
	assert.Len(t, m.Widgets, 1)
	assert.Equal(t, []string{"SharedLib"}, m.Widgets[0].Depends)
	assert.Equal(t, []string{"presets.txt"}, m.Widgets[0].Exclude)
}

func TestLoad_MissingFile(t *testing.T) {
	dir := t.TempDir()

	_, err := Load(dir)
	assert.Error(t, err)
	assert.Contains(t, err.Error(), "reading manifest")
}

func TestLoad_MalformedTOML(t *testing.T) {
	dir := t.TempDir()
	if !assert.NoError(t, os.WriteFile(filepath.Join(dir, FileName), []byte("{{invalid"), 0o644)) {
		return
	}

	_, err := Load(dir)
	assert.Error(t, err)
	assert.Contains(t, err.Error(), "parsing manifest")
}

func TestValidate_UnresolvedDep(t *testing.T) {
	m := &Manifest{
		Widgets: []ContentItem{
			{Name: "BadWidget", Path: "WIDGETS/Bad", Depends: []string{"NonExistent"}},
		},
	}

	err := m.Validate()
	assert.Error(t, err)
	assert.Contains(t, err.Error(), "NonExistent")
	assert.Contains(t, err.Error(), "BadWidget")
}

func TestValidate_AllDepsResolved(t *testing.T) {
	m := &Manifest{
		Libraries: []ContentItem{
			{Name: "Lib1", Path: "SCRIPTS/Lib1"},
		},
		Widgets: []ContentItem{
			{Name: "Widget1", Path: "WIDGETS/W1", Depends: []string{"Lib1"}},
		},
	}

	assert.NoError(t, m.Validate())
}

func TestAllPaths_LibrariesFirst(t *testing.T) {
	m := &Manifest{
		Scripts:   []ContentItem{{Name: "S", Path: "scripts/s"}},
		Tools:     []ContentItem{{Name: "T", Path: "tools/t"}},
		Widgets:   []ContentItem{{Name: "W", Path: "widgets/w"}},
		Libraries: []ContentItem{{Name: "L", Path: "libs/l"}},
	}

	paths := m.AllPaths()

	assert.Equal(t, []string{"libs/l", "scripts/s", "tools/t", "widgets/w"}, paths)
	assert.Equal(t, "libs/l", paths[0], "libraries should come first")
}
