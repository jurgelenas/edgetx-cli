package scaffold

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/edgetx/cli/pkg/manifest"
	"github.com/stretchr/testify/assert"
)

const baseTOML = `[package]
name = "test-pack"
version = "1.0.0"
description = "A test package"
`

const baseTOMLWithLib = `[package]
name = "test-pack"
version = "1.0.0"
description = "A test package"

[[libraries]]
name = "SharedLib"
path = "SCRIPTS/SharedLib"
`

func setupDir(t *testing.T, tomlContent string, extraDirs ...string) string {
	t.Helper()
	dir := t.TempDir()
	assert.NoError(t, os.WriteFile(filepath.Join(dir, manifest.FileName), []byte(tomlContent), 0o644))
	for _, d := range extraDirs {
		assert.NoError(t, os.MkdirAll(filepath.Join(dir, d), 0o755))
	}
	return dir
}

func TestRun_Tool(t *testing.T) {
	dir := setupDir(t, baseTOML)

	result, err := Run(Options{Type: "tool", Name: "MyTool", SrcDir: dir})
	assert.NoError(t, err)

	assert.Equal(t, filepath.Join(dir, "SCRIPTS/TOOLS/MyTool/main.lua"), result.FilePath)
	assert.Equal(t, "SCRIPTS/TOOLS/MyTool", result.ContentPath)

	content, err := os.ReadFile(result.FilePath)
	assert.NoError(t, err)
	assert.Contains(t, string(content), "local function run(event, touchState)")

	// Re-load manifest to verify it parses correctly
	m, err := manifest.Load(dir)
	assert.NoError(t, err)
	assert.Len(t, m.Tools, 1)
	assert.Equal(t, "MyTool", m.Tools[0].Name)
	assert.Equal(t, "SCRIPTS/TOOLS/MyTool", m.Tools[0].Path)
}

func TestRun_Telemetry(t *testing.T) {
	dir := setupDir(t, baseTOML)

	result, err := Run(Options{Type: "telemetry", Name: "MyTlm", SrcDir: dir})
	assert.NoError(t, err)

	assert.Equal(t, filepath.Join(dir, "SCRIPTS/TELEMETRY/MyTlm.lua"), result.FilePath)
	assert.Equal(t, "SCRIPTS/TELEMETRY/MyTlm.lua", result.ContentPath)

	content, err := os.ReadFile(result.FilePath)
	assert.NoError(t, err)
	assert.Contains(t, string(content), "local function background()")
}

func TestRun_Function(t *testing.T) {
	dir := setupDir(t, baseTOML)

	result, err := Run(Options{Type: "function", Name: "MyFn", SrcDir: dir})
	assert.NoError(t, err)

	assert.Equal(t, filepath.Join(dir, "SCRIPTS/FUNCTIONS/MyFn.lua"), result.FilePath)

	content, err := os.ReadFile(result.FilePath)
	assert.NoError(t, err)
	assert.Contains(t, string(content), "return { init = init, run = run }")
}

func TestRun_Mix(t *testing.T) {
	dir := setupDir(t, baseTOML)

	result, err := Run(Options{Type: "mix", Name: "MyMix", SrcDir: dir})
	assert.NoError(t, err)

	assert.Equal(t, filepath.Join(dir, "SCRIPTS/MIXES/MyMix.lua"), result.FilePath)

	content, err := os.ReadFile(result.FilePath)
	assert.NoError(t, err)
	assert.Contains(t, string(content), "local input = {}")
	assert.Contains(t, string(content), "local output = {}")
}

func TestRun_Widget(t *testing.T) {
	dir := setupDir(t, baseTOML)

	result, err := Run(Options{Type: "widget", Name: "MyWdgt", SrcDir: dir})
	assert.NoError(t, err)

	assert.Equal(t, filepath.Join(dir, "WIDGETS/MyWdgt/main.lua"), result.FilePath)
	assert.Equal(t, "WIDGETS/MyWdgt", result.ContentPath)

	content, err := os.ReadFile(result.FilePath)
	assert.NoError(t, err)
	assert.Contains(t, string(content), `local name = "MyWdgt"`)
	assert.Contains(t, string(content), "local function create(zone, options)")
}

func TestRun_Library(t *testing.T) {
	dir := setupDir(t, baseTOML)

	result, err := Run(Options{Type: "library", Name: "MyLib", SrcDir: dir})
	assert.NoError(t, err)

	assert.Equal(t, filepath.Join(dir, "SCRIPTS/MyLib/main.lua"), result.FilePath)
	assert.Equal(t, "SCRIPTS/MyLib", result.ContentPath)

	content, err := os.ReadFile(result.FilePath)
	assert.NoError(t, err)
	assert.Contains(t, string(content), "local M = {}")
	assert.Contains(t, string(content), "return M")

	// Re-load manifest to verify it parses correctly
	m, err := manifest.Load(dir)
	assert.NoError(t, err)
	assert.Len(t, m.Libraries, 1)
	assert.Equal(t, "MyLib", m.Libraries[0].Name)
	assert.Equal(t, "SCRIPTS/MyLib", m.Libraries[0].Path)
}

func TestRun_InvalidType(t *testing.T) {
	dir := setupDir(t, baseTOML)

	_, err := Run(Options{Type: "bogus", Name: "Test", SrcDir: dir})
	assert.Error(t, err)
	assert.Contains(t, err.Error(), "unknown script type")
	assert.Contains(t, err.Error(), "valid types")
}

func TestRun_NameTooLong(t *testing.T) {
	dir := setupDir(t, baseTOML)

	_, err := Run(Options{Type: "telemetry", Name: "TooLong", SrcDir: dir})
	assert.Error(t, err)
	assert.Contains(t, err.Error(), "too long")
	assert.Contains(t, err.Error(), "max 6")
}

func TestRun_DuplicateName(t *testing.T) {
	toml := baseTOML + `
[[tools]]
name = "MyTool"
path = "SCRIPTS/TOOLS/MyTool"
`
	dir := setupDir(t, toml, "SCRIPTS/TOOLS/MyTool")

	_, err := Run(Options{Type: "tool", Name: "MyTool", SrcDir: dir})
	assert.Error(t, err)
	assert.Contains(t, err.Error(), "already exists")
}

func TestRun_InvalidChars(t *testing.T) {
	dir := setupDir(t, baseTOML)

	_, err := Run(Options{Type: "tool", Name: "My Tool", SrcDir: dir})
	assert.Error(t, err)
	assert.Contains(t, err.Error(), "invalid name")

	_, err = Run(Options{Type: "tool", Name: "1Bad", SrcDir: dir})
	assert.Error(t, err)
	assert.Contains(t, err.Error(), "invalid name")

	_, err = Run(Options{Type: "tool", Name: "my-tool", SrcDir: dir})
	assert.Error(t, err)
	assert.Contains(t, err.Error(), "invalid name")
}

func TestRun_UnresolvedDepends(t *testing.T) {
	dir := setupDir(t, baseTOML)

	_, err := Run(Options{Type: "tool", Name: "MyTool", Depends: []string{"NonExistent"}, SrcDir: dir})
	assert.Error(t, err)
	assert.Contains(t, err.Error(), "unresolved dependencies")
	assert.Contains(t, err.Error(), "NonExistent")
}

func TestRun_ValidDepends(t *testing.T) {
	dir := setupDir(t, baseTOMLWithLib, "SCRIPTS/SharedLib")

	result, err := Run(Options{Type: "tool", Name: "MyTool", Depends: []string{"SharedLib"}, SrcDir: dir})
	assert.NoError(t, err)
	assert.NotNil(t, result)

	tomlData, err := os.ReadFile(filepath.Join(dir, manifest.FileName))
	assert.NoError(t, err)
	assert.Contains(t, string(tomlData), `depends = ["SharedLib"]`)
}

func TestRun_AppendPreservesExisting(t *testing.T) {
	dir := setupDir(t, baseTOML)

	_, err := Run(Options{Type: "tool", Name: "MyTool", SrcDir: dir})
	assert.NoError(t, err)

	tomlData, err := os.ReadFile(filepath.Join(dir, manifest.FileName))
	assert.NoError(t, err)

	content := string(tomlData)
	assert.True(t, strings.HasPrefix(content, "[package]"), "original content should be preserved at the start")
	assert.Contains(t, content, `name = "test-pack"`)
	assert.Contains(t, content, "[[tools]]")
	assert.Contains(t, content, `name = "MyTool"`)
}
