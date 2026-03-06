package manifest

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/stretchr/testify/assert"
)

const validYAML = `package:
  name: test-pack
  description: A test package

libraries:
  - name: SharedLib
    path: SCRIPTS/SharedLib

tools:
  - name: MyTool
    path: SCRIPTS/TOOLS/MyTool

telemetry:
  - name: MyTelemetry
    path: SCRIPTS/TELEMETRY/MyTelemetry

functions:
  - name: MyFunction
    path: SCRIPTS/FUNCTIONS/MyFunction

mixes:
  - name: MyMix
    path: SCRIPTS/MIXES/MyMix

widgets:
  - name: MyWidget
    path: WIDGETS/MyWidget
    depends:
      - SharedLib
    exclude:
      - presets.txt
`

func TestLoad_ValidManifest(t *testing.T) {
	dir := t.TempDir()
	for _, p := range []string{"SCRIPTS/SharedLib", "SCRIPTS/TOOLS/MyTool", "SCRIPTS/TELEMETRY/MyTelemetry", "SCRIPTS/FUNCTIONS/MyFunction", "SCRIPTS/MIXES/MyMix", "WIDGETS/MyWidget"} {
		os.MkdirAll(filepath.Join(dir, p), 0o755)
	}
	if !assert.NoError(t, os.WriteFile(filepath.Join(dir, FileName), []byte(validYAML), 0o644)) {
		return
	}

	m, err := Load(dir)
	if !assert.NoError(t, err) {
		return
	}

	assert.Equal(t, "test-pack", m.Package.Name)
	assert.Equal(t, "A test package", m.Package.Description)
	assert.False(t, m.Package.Binary)

	assert.Len(t, m.Libraries, 1)
	assert.Equal(t, "SharedLib", m.Libraries[0].Name)

	assert.Len(t, m.Tools, 1)
	assert.Len(t, m.Telemetry, 1)
	assert.Len(t, m.Functions, 1)
	assert.Len(t, m.Mixes, 1)
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

func TestLoad_MalformedYAML(t *testing.T) {
	dir := t.TempDir()
	if !assert.NoError(t, os.WriteFile(filepath.Join(dir, FileName), []byte(":\n  :\n    - [invalid"), 0o644)) {
		return
	}

	_, err := Load(dir)
	assert.Error(t, err)
	assert.Contains(t, err.Error(), "parsing manifest")
}

func TestValidate_UnresolvedDep(t *testing.T) {
	dir := t.TempDir()
	os.MkdirAll(filepath.Join(dir, "WIDGETS/Bad"), 0o755)

	m := &Manifest{
		Package: Package{Name: "test"},
		Widgets: []ContentItem{
			{Name: "BadWidget", Path: "WIDGETS/Bad", Depends: []string{"NonExistent"}},
		},
	}

	err := m.Validate(dir)
	assert.Error(t, err)
	assert.Contains(t, err.Error(), "NonExistent")
	assert.Contains(t, err.Error(), "BadWidget")
}

func TestValidate_AllDepsResolved(t *testing.T) {
	dir := t.TempDir()
	os.MkdirAll(filepath.Join(dir, "SCRIPTS/Lib1"), 0o755)
	os.MkdirAll(filepath.Join(dir, "WIDGETS/W1"), 0o755)

	m := &Manifest{
		Package: Package{Name: "test"},
		Libraries: []ContentItem{
			{Name: "Lib1", Path: "SCRIPTS/Lib1"},
		},
		Widgets: []ContentItem{
			{Name: "Widget1", Path: "WIDGETS/W1", Depends: []string{"Lib1"}},
		},
	}

	assert.NoError(t, m.Validate(dir))
}

func TestValidate_NewTypeDepsResolved(t *testing.T) {
	dir := t.TempDir()
	os.MkdirAll(filepath.Join(dir, "SCRIPTS/Lib1"), 0o755)
	os.MkdirAll(filepath.Join(dir, "SCRIPTS/TELEMETRY/T1"), 0o755)
	os.MkdirAll(filepath.Join(dir, "SCRIPTS/FUNCTIONS/F1"), 0o755)
	os.MkdirAll(filepath.Join(dir, "SCRIPTS/MIXES/M1"), 0o755)

	m := &Manifest{
		Package: Package{Name: "test"},
		Libraries: []ContentItem{
			{Name: "Lib1", Path: "SCRIPTS/Lib1"},
		},
		Telemetry: []ContentItem{
			{Name: "Telem1", Path: "SCRIPTS/TELEMETRY/T1", Depends: []string{"Lib1"}},
		},
		Functions: []ContentItem{
			{Name: "Func1", Path: "SCRIPTS/FUNCTIONS/F1", Depends: []string{"Lib1"}},
		},
		Mixes: []ContentItem{
			{Name: "Mix1", Path: "SCRIPTS/MIXES/M1", Depends: []string{"Lib1"}},
		},
	}

	assert.NoError(t, m.Validate(dir))
}

func TestValidate_NewTypeUnresolvedDep(t *testing.T) {
	dir := t.TempDir()
	os.MkdirAll(filepath.Join(dir, "SCRIPTS/TELEMETRY/T1"), 0o755)

	m := &Manifest{
		Package: Package{Name: "test"},
		Telemetry: []ContentItem{
			{Name: "Telem1", Path: "SCRIPTS/TELEMETRY/T1", Depends: []string{"Missing"}},
		},
	}

	err := m.Validate(dir)
	assert.Error(t, err)
	assert.Contains(t, err.Error(), "Missing")
	assert.Contains(t, err.Error(), "Telem1")
}

func TestValidate_NameEmpty(t *testing.T) {
	dir := t.TempDir()
	m := &Manifest{}
	err := m.Validate(dir)
	assert.Error(t, err)
	assert.Contains(t, err.Error(), "name is required")
}

func TestValidate_NameInvalid(t *testing.T) {
	dir := t.TempDir()

	invalid := []string{"has space", "has.dot", "has/slash", "@special", "über"}
	for _, name := range invalid {
		m := &Manifest{Package: Package{Name: name}}
		err := m.Validate(dir)
		assert.Error(t, err, "name %q should be invalid", name)
	}
}

func TestValidate_NameValid(t *testing.T) {
	dir := t.TempDir()

	valid := []string{"expresslrs", "my-tool", "my_tool", "Tool123", "a"}
	for _, name := range valid {
		m := &Manifest{Package: Package{Name: name}}
		err := m.Validate(dir)
		// May fail for other reasons (no content paths), but not for name.
		if err != nil {
			assert.NotContains(t, err.Error(), "package name", "name %q should be valid", name)
		}
	}
}

func TestValidate_SourceDirNotExists(t *testing.T) {
	dir := t.TempDir()

	m := &Manifest{
		Package: Package{Name: "test", SourceDir: "nonexistent"},
	}

	err := m.Validate(dir)
	assert.Error(t, err)
	assert.Contains(t, err.Error(), "source directory")
}

func TestValidate_ContentPathNotExists(t *testing.T) {
	dir := t.TempDir()
	os.MkdirAll(filepath.Join(dir, "src"), 0o755)

	m := &Manifest{
		Package: Package{Name: "test", SourceDir: "src"},
		Libraries: []ContentItem{
			{Name: "Missing", Path: "SCRIPTS/Missing"},
		},
	}

	err := m.Validate(dir)
	assert.Error(t, err)
	assert.Contains(t, err.Error(), "SCRIPTS/Missing")
}

func TestValidate_ValidSourceDirAndPaths(t *testing.T) {
	dir := t.TempDir()
	os.MkdirAll(filepath.Join(dir, "src/SCRIPTS/Lib1"), 0o755)
	os.MkdirAll(filepath.Join(dir, "src/WIDGETS/W1"), 0o755)

	m := &Manifest{
		Package: Package{Name: "test", SourceDir: "src"},
		Libraries: []ContentItem{
			{Name: "Lib1", Path: "SCRIPTS/Lib1"},
		},
		Widgets: []ContentItem{
			{Name: "Widget1", Path: "WIDGETS/W1", Depends: []string{"Lib1"}},
		},
	}

	assert.NoError(t, m.Validate(dir))
}

func TestSourceRoot_WithSourceDir(t *testing.T) {
	m := &Manifest{
		Package: Package{Name: "test", SourceDir: "src"},
	}
	assert.Equal(t, "/project/src", m.SourceRoot("/project"))
}

func TestSourceRoot_Empty(t *testing.T) {
	m := &Manifest{}
	assert.Equal(t, "/project", m.SourceRoot("/project"))
}

func TestContentItems(t *testing.T) {
	m := &Manifest{
		Libraries: []ContentItem{{Name: "L", Path: "libs/l"}},
		Tools:     []ContentItem{{Name: "T", Path: "tools/t"}},
		Telemetry: []ContentItem{{Name: "Te", Path: "telemetry/te"}},
		Functions: []ContentItem{{Name: "F", Path: "functions/f"}},
		Mixes:     []ContentItem{{Name: "M", Path: "mixes/m"}},
		Widgets:   []ContentItem{{Name: "W", Path: "widgets/w"}},
	}

	items := m.ContentItems()
	assert.Len(t, items, 6)
	assert.Equal(t, "L", items[0].Name, "libraries should come first")
	assert.Equal(t, "T", items[1].Name)
	assert.Equal(t, "Te", items[2].Name)
	assert.Equal(t, "F", items[3].Name)
	assert.Equal(t, "M", items[4].Name)
	assert.Equal(t, "W", items[5].Name)
}

func TestAllPaths_LibrariesFirst(t *testing.T) {
	m := &Manifest{
		Libraries: []ContentItem{{Name: "L", Path: "libs/l"}},
		Tools:     []ContentItem{{Name: "T", Path: "tools/t"}},
		Telemetry: []ContentItem{{Name: "Te", Path: "telemetry/te"}},
		Functions: []ContentItem{{Name: "F", Path: "functions/f"}},
		Mixes:     []ContentItem{{Name: "M", Path: "mixes/m"}},
		Widgets:   []ContentItem{{Name: "W", Path: "widgets/w"}},
	}

	paths := m.AllPaths()

	assert.Equal(t, []string{"libs/l", "tools/t", "telemetry/te", "functions/f", "mixes/m", "widgets/w"}, paths)
	assert.Equal(t, "libs/l", paths[0], "libraries should come first")
}

func TestLoad_WithSourceDir(t *testing.T) {
	dir := t.TempDir()
	os.MkdirAll(filepath.Join(dir, "src/SCRIPTS/Lib"), 0o755)

	yamlContent := `package:
  name: test
  source_dir: src

libraries:
  - name: Lib
    path: SCRIPTS/Lib
`
	assert.NoError(t, os.WriteFile(filepath.Join(dir, FileName), []byte(yamlContent), 0o644))

	m, err := Load(dir)
	if !assert.NoError(t, err) {
		return
	}
	assert.Equal(t, "src", m.Package.SourceDir)
	assert.Equal(t, filepath.Join(dir, "src"), m.SourceRoot(dir))
}

func TestValidate_MinEdgeTXVersionValid(t *testing.T) {
	dir := t.TempDir()
	os.MkdirAll(filepath.Join(dir, "WIDGETS/W"), 0o755)

	m := &Manifest{
		Package: Package{Name: "test", MinEdgeTXVersion: "2.12.0"},
		Widgets: []ContentItem{{Name: "W", Path: "WIDGETS/W"}},
	}

	assert.NoError(t, m.Validate(dir))
}

func TestValidate_MinEdgeTXVersionWithVPrefix(t *testing.T) {
	dir := t.TempDir()
	os.MkdirAll(filepath.Join(dir, "WIDGETS/W"), 0o755)

	m := &Manifest{
		Package: Package{Name: "test", MinEdgeTXVersion: "v2.12.0"},
		Widgets: []ContentItem{{Name: "W", Path: "WIDGETS/W"}},
	}

	assert.NoError(t, m.Validate(dir))
}

func TestValidate_MinEdgeTXVersionInvalid(t *testing.T) {
	dir := t.TempDir()

	m := &Manifest{
		Package: Package{Name: "test", MinEdgeTXVersion: "not-a-version"},
	}

	err := m.Validate(dir)
	assert.Error(t, err)
	assert.Contains(t, err.Error(), "min_edgetx_version")
	assert.Contains(t, err.Error(), "not a valid semver")
}

func TestLoad_BinaryTrue(t *testing.T) {
	dir := t.TempDir()
	os.MkdirAll(filepath.Join(dir, "WIDGETS/W"), 0o755)

	yamlContent := `package:
  name: binary-pkg
  description: A binary package
  binary: true

widgets:
  - name: W
    path: WIDGETS/W
`
	assert.NoError(t, os.WriteFile(filepath.Join(dir, FileName), []byte(yamlContent), 0o644))

	m, err := Load(dir)
	if !assert.NoError(t, err) {
		return
	}
	assert.True(t, m.Package.Binary)
}

func TestContentItems_ExcludesDevByDefault(t *testing.T) {
	m := &Manifest{
		Libraries: []ContentItem{
			{Name: "Lib", Path: "libs/lib"},
			{Name: "DevLib", Path: "libs/devlib", Dev: true},
		},
		Widgets: []ContentItem{
			{Name: "W", Path: "widgets/w"},
			{Name: "DevW", Path: "widgets/devw", Dev: true},
		},
	}

	items := m.ContentItems()
	assert.Len(t, items, 2)
	assert.Equal(t, "Lib", items[0].Name)
	assert.Equal(t, "W", items[1].Name)

	itemsWithDev := m.ContentItems(true)
	assert.Len(t, itemsWithDev, 4)
}

func TestAllPaths_ExcludesDevByDefault(t *testing.T) {
	m := &Manifest{
		Libraries: []ContentItem{
			{Name: "Lib", Path: "libs/lib"},
			{Name: "DevLib", Path: "libs/devlib", Dev: true},
		},
	}

	assert.Equal(t, []string{"libs/lib"}, m.AllPaths())
	assert.Equal(t, []string{"libs/lib", "libs/devlib"}, m.AllPaths(true))
}

func TestValidate_NonDevDependsOnDevLibrary(t *testing.T) {
	dir := t.TempDir()
	os.MkdirAll(filepath.Join(dir, "SCRIPTS/DevLib"), 0o755)
	os.MkdirAll(filepath.Join(dir, "WIDGETS/W"), 0o755)

	m := &Manifest{
		Package: Package{Name: "test"},
		Libraries: []ContentItem{
			{Name: "DevLib", Path: "SCRIPTS/DevLib", Dev: true},
		},
		Widgets: []ContentItem{
			{Name: "Widget", Path: "WIDGETS/W", Depends: []string{"DevLib"}},
		},
	}

	err := m.Validate(dir)
	assert.Error(t, err)
	assert.Contains(t, err.Error(), "non-dev items depend on dev libraries")
	assert.Contains(t, err.Error(), "Widget")
}

func TestValidate_DevItemDependsOnDevLibrary(t *testing.T) {
	dir := t.TempDir()
	os.MkdirAll(filepath.Join(dir, "SCRIPTS/DevLib"), 0o755)
	os.MkdirAll(filepath.Join(dir, "WIDGETS/W"), 0o755)

	m := &Manifest{
		Package: Package{Name: "test"},
		Libraries: []ContentItem{
			{Name: "DevLib", Path: "SCRIPTS/DevLib", Dev: true},
		},
		Widgets: []ContentItem{
			{Name: "Widget", Path: "WIDGETS/W", Depends: []string{"DevLib"}, Dev: true},
		},
	}

	assert.NoError(t, m.Validate(dir))
}

func TestLoad_DevField(t *testing.T) {
	dir := t.TempDir()
	os.MkdirAll(filepath.Join(dir, "SCRIPTS/Lib"), 0o755)
	os.MkdirAll(filepath.Join(dir, "SCRIPTS/TOOLS/DevTool"), 0o755)

	yamlContent := `package:
  name: test
libraries:
  - name: Lib
    path: SCRIPTS/Lib
tools:
  - name: DevTool
    path: SCRIPTS/TOOLS/DevTool
    dev: true
`
	assert.NoError(t, os.WriteFile(filepath.Join(dir, FileName), []byte(yamlContent), 0o644))

	m, err := Load(dir)
	if !assert.NoError(t, err) {
		return
	}
	assert.False(t, m.Libraries[0].Dev)
	assert.True(t, m.Tools[0].Dev)
}

func TestLoad_BinaryDefault(t *testing.T) {
	dir := t.TempDir()
	os.MkdirAll(filepath.Join(dir, "WIDGETS/W"), 0o755)

	yamlContent := `package:
  name: source-pkg
  description: A source package

widgets:
  - name: W
    path: WIDGETS/W
`
	assert.NoError(t, os.WriteFile(filepath.Join(dir, FileName), []byte(yamlContent), 0o644))

	m, err := Load(dir)
	if !assert.NoError(t, err) {
		return
	}
	assert.False(t, m.Package.Binary)
}
