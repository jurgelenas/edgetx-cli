package packages

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/stretchr/testify/assert"
)

func TestLoadState_NoFile(t *testing.T) {
	dir := t.TempDir()
	s, err := LoadState(dir)
	assert.NoError(t, err)
	assert.Empty(t, s.Packages)
}

func TestLoadState_ValidFile(t *testing.T) {
	dir := t.TempDir()
	os.MkdirAll(filepath.Join(dir, "RADIO"), 0o755)

	content := `[[packages]]
source = "ExpressLRS/Lua-Scripts"
name = "expresslrs"
channel = "tag"
version = "v1.6.0"
commit = "abc123def456789"
paths = ["SCRIPTS/TOOLS/ELRS", "SCRIPTS/ELRS"]

[[packages]]
source = "local::/home/user/project"
name = "my-tool"
channel = "local"
paths = ["SCRIPTS/TOOLS/MyTool"]
`
	assert.NoError(t, os.WriteFile(filepath.Join(dir, StateFileName), []byte(content), 0o644))

	s, err := LoadState(dir)
	assert.NoError(t, err)
	assert.Len(t, s.Packages, 2)

	assert.Equal(t, "ExpressLRS/Lua-Scripts", s.Packages[0].Source)
	assert.Equal(t, "expresslrs", s.Packages[0].Name)
	assert.Equal(t, "tag", s.Packages[0].Channel)
	assert.Equal(t, "v1.6.0", s.Packages[0].Version)
	assert.Equal(t, "abc123def456789", s.Packages[0].Commit)
	assert.Equal(t, []string{"SCRIPTS/TOOLS/ELRS", "SCRIPTS/ELRS"}, s.Packages[0].Paths)

	assert.Equal(t, "local", s.Packages[1].Channel)
	assert.Empty(t, s.Packages[1].Version)
	assert.Empty(t, s.Packages[1].Commit)
}

func TestLoadState_MalformedTOML(t *testing.T) {
	dir := t.TempDir()
	os.MkdirAll(filepath.Join(dir, "RADIO"), 0o755)
	os.WriteFile(filepath.Join(dir, StateFileName), []byte("{{invalid"), 0o644)

	_, err := LoadState(dir)
	assert.Error(t, err)
}

func TestSave_CreatesFile(t *testing.T) {
	dir := t.TempDir()

	s := &State{
		Packages: []InstalledPackage{
			{
				Source:  "Org/Repo",
				Name:    "my-pkg",
				Channel: "tag",
				Version: "v1.0.0",
				Commit:  "abc123",
				Paths:   []string{"SCRIPTS/TOOLS/MyTool"},
			},
		},
	}

	assert.NoError(t, s.Save(dir))
	assert.FileExists(t, filepath.Join(dir, StateFileName))
}

func TestSave_RoundTrip(t *testing.T) {
	dir := t.TempDir()

	original := &State{
		Packages: []InstalledPackage{
			{
				Source:  "Org/Repo",
				Name:    "my-pkg",
				Channel: "tag",
				Version: "v1.0.0",
				Commit:  "abc123",
				Paths:   []string{"SCRIPTS/TOOLS/MyTool"},
			},
			{
				Source:  "local::/home/user/proj",
				Name:    "local-pkg",
				Channel: "local",
				Paths:   []string{"SCRIPTS/TOOLS/Local"},
			},
		},
	}

	assert.NoError(t, original.Save(dir))

	loaded, err := LoadState(dir)
	assert.NoError(t, err)
	assert.Equal(t, len(original.Packages), len(loaded.Packages))

	for i := range original.Packages {
		assert.Equal(t, original.Packages[i].Source, loaded.Packages[i].Source)
		assert.Equal(t, original.Packages[i].Name, loaded.Packages[i].Name)
		assert.Equal(t, original.Packages[i].Channel, loaded.Packages[i].Channel)
		assert.Equal(t, original.Packages[i].Version, loaded.Packages[i].Version)
		assert.Equal(t, original.Packages[i].Commit, loaded.Packages[i].Commit)
		assert.Equal(t, original.Packages[i].Paths, loaded.Packages[i].Paths)
	}
}

func TestFindBySource_Found(t *testing.T) {
	s := &State{
		Packages: []InstalledPackage{
			{Source: "A/B", Name: "a"},
			{Source: "C/D", Name: "c"},
			{Source: "E/F", Name: "e"},
		},
	}

	pkg := s.FindBySource("C/D")
	assert.NotNil(t, pkg)
	assert.Equal(t, "c", pkg.Name)
}

func TestFindBySource_NotFound(t *testing.T) {
	s := &State{
		Packages: []InstalledPackage{
			{Source: "A/B", Name: "a"},
		},
	}

	assert.Nil(t, s.FindBySource("X/Y"))
}

func TestFind_BySource(t *testing.T) {
	s := &State{
		Packages: []InstalledPackage{
			{Source: "ExpressLRS/Lua-Scripts", Name: "expresslrs"},
		},
	}

	pkg, err := s.Find("ExpressLRS/Lua-Scripts")
	assert.NoError(t, err)
	assert.Equal(t, "expresslrs", pkg.Name)
}

func TestFind_ByName(t *testing.T) {
	s := &State{
		Packages: []InstalledPackage{
			{Source: "ExpressLRS/Lua-Scripts", Name: "expresslrs"},
		},
	}

	pkg, err := s.Find("expresslrs")
	assert.NoError(t, err)
	assert.Equal(t, "ExpressLRS/Lua-Scripts", pkg.Source)
}

func TestFind_SourcePrecedence(t *testing.T) {
	s := &State{
		Packages: []InstalledPackage{
			{Source: "expresslrs", Name: "something-else"},
			{Source: "ExpressLRS/Lua-Scripts", Name: "expresslrs"},
		},
	}

	pkg, err := s.Find("expresslrs")
	assert.NoError(t, err)
	assert.Equal(t, "something-else", pkg.Name, "source match should take precedence")
}

func TestFind_AmbiguousName(t *testing.T) {
	s := &State{
		Packages: []InstalledPackage{
			{Source: "A/B", Name: "shared-name"},
			{Source: "C/D", Name: "shared-name"},
		},
	}

	_, err := s.Find("shared-name")
	assert.Error(t, err)
	assert.Contains(t, err.Error(), "ambiguous")
}

func TestFind_NotFound(t *testing.T) {
	s := &State{
		Packages: []InstalledPackage{
			{Source: "A/B", Name: "a"},
		},
	}

	_, err := s.Find("nonexistent")
	assert.Error(t, err)
	assert.Contains(t, err.Error(), "not found")
}

func TestRemove(t *testing.T) {
	s := &State{
		Packages: []InstalledPackage{
			{Source: "A/B", Name: "a"},
			{Source: "C/D", Name: "c"},
			{Source: "E/F", Name: "e"},
		},
	}

	s.Remove("C/D")
	assert.Len(t, s.Packages, 2)
	assert.Nil(t, s.FindBySource("C/D"))
	assert.NotNil(t, s.FindBySource("A/B"))
	assert.NotNil(t, s.FindBySource("E/F"))
}

func TestAdd_NewPackage(t *testing.T) {
	s := &State{}
	s.Add(InstalledPackage{Source: "A/B", Name: "a"})
	assert.Len(t, s.Packages, 1)
}

func TestAdd_ReplaceExisting(t *testing.T) {
	s := &State{
		Packages: []InstalledPackage{
			{Source: "A/B", Name: "old", Version: "v1.0.0"},
		},
	}

	s.Add(InstalledPackage{Source: "A/B", Name: "new", Version: "v2.0.0"})
	assert.Len(t, s.Packages, 1)
	assert.Equal(t, "new", s.Packages[0].Name)
	assert.Equal(t, "v2.0.0", s.Packages[0].Version)
}

func TestAllInstalledPaths(t *testing.T) {
	s := &State{
		Packages: []InstalledPackage{
			{Source: "A/B", Paths: []string{"SCRIPTS/TOOLS/A", "SCRIPTS/LIB"}},
			{Source: "C/D", Paths: []string{"WIDGETS/C"}},
		},
	}

	paths := s.AllInstalledPaths()
	assert.Equal(t, "A/B", paths["SCRIPTS/TOOLS/A"])
	assert.Equal(t, "A/B", paths["SCRIPTS/LIB"])
	assert.Equal(t, "C/D", paths["WIDGETS/C"])
	assert.Len(t, paths, 3)
}

func TestFileList_RoundTrip(t *testing.T) {
	dir := t.TempDir()

	files := []string{
		"SCRIPTS/TOOLS/MyTool/main.lua",
		"SCRIPTS/TOOLS/MyTool/helpers.lua",
		"WIDGETS/MyWidget/main.lua",
	}

	assert.NoError(t, SaveFileList(dir, "my-pkg", files))

	loaded, err := LoadFileList(dir, "my-pkg")
	assert.NoError(t, err)
	assert.Equal(t, files, loaded)
}

func TestFileList_NotFound(t *testing.T) {
	dir := t.TempDir()

	files, err := LoadFileList(dir, "nonexistent")
	assert.NoError(t, err)
	assert.Nil(t, files)
}

func TestFileList_Remove(t *testing.T) {
	dir := t.TempDir()

	assert.NoError(t, SaveFileList(dir, "my-pkg", []string{"a.lua"}))
	RemoveFileList(dir, "my-pkg")

	files, err := LoadFileList(dir, "my-pkg")
	assert.NoError(t, err)
	assert.Nil(t, files)
}
