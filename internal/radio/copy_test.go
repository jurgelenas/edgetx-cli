package radio

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/stretchr/testify/assert"
)

func createSourceTree(t *testing.T, base string) {
	t.Helper()

	files := map[string]string{
		"SCRIPTS/ELRS/crsf.lua":             "-- crsf lib",
		"SCRIPTS/ELRS/shim.lua":             "-- shim lib",
		"SCRIPTS/TOOLS/ExpressLRS/elrs.lua": "-- elrs tool",
		"WIDGETS/ELRSTelemetry/main.lua":    "-- telemetry widget",
	}

	for relPath, content := range files {
		fullPath := filepath.Join(base, relPath)
		if !assert.NoError(t, os.MkdirAll(filepath.Dir(fullPath), 0o755)) {
			return
		}
		if !assert.NoError(t, os.WriteFile(fullPath, []byte(content), 0o644)) {
			return
		}
	}
}

func TestCopyPaths_CopiesFiles(t *testing.T) {
	srcDir := t.TempDir()
	destDir := t.TempDir()
	createSourceTree(t, srcDir)

	paths := []string{
		"SCRIPTS/ELRS",
		"SCRIPTS/TOOLS/ExpressLRS",
		"WIDGETS/ELRSTelemetry",
	}

	var copiedFiles []string
	opts := CopyOptions{
		OnFile: func(dest string) {
			copiedFiles = append(copiedFiles, dest)
		},
	}

	copied, err := CopyPaths(srcDir, destDir, paths, opts)
	if !assert.NoError(t, err) {
		return
	}
	assert.Equal(t, 4, copied)
	assert.Len(t, copiedFiles, 4, "OnFile should have been called for each file")

	assert.FileExists(t, filepath.Join(destDir, "SCRIPTS/ELRS/crsf.lua"))
	assert.FileExists(t, filepath.Join(destDir, "SCRIPTS/ELRS/shim.lua"))
	assert.FileExists(t, filepath.Join(destDir, "SCRIPTS/TOOLS/ExpressLRS/elrs.lua"))
	assert.FileExists(t, filepath.Join(destDir, "WIDGETS/ELRSTelemetry/main.lua"))

	content, err := os.ReadFile(filepath.Join(destDir, "SCRIPTS/ELRS/crsf.lua"))
	if !assert.NoError(t, err) {
		return
	}
	assert.Equal(t, "-- crsf lib", string(content))
}

func TestCopyPaths_DryRun(t *testing.T) {
	srcDir := t.TempDir()
	destDir := t.TempDir()
	createSourceTree(t, srcDir)

	paths := []string{"SCRIPTS/ELRS", "WIDGETS/ELRSTelemetry"}

	opts := CopyOptions{DryRun: true}

	copied, err := CopyPaths(srcDir, destDir, paths, opts)
	if !assert.NoError(t, err) {
		return
	}
	assert.Equal(t, 0, copied)

	entries, err := os.ReadDir(destDir)
	if !assert.NoError(t, err) {
		return
	}
	assert.Empty(t, entries, "dest should be empty in dry-run mode")
}

func TestCopyPaths_MissingSourceSkipped(t *testing.T) {
	srcDir := t.TempDir()
	destDir := t.TempDir()
	createSourceTree(t, srcDir)

	paths := []string{
		"DOES_NOT_EXIST",
		"SCRIPTS/ELRS",
	}

	opts := CopyOptions{}

	copied, err := CopyPaths(srcDir, destDir, paths, opts)
	if !assert.NoError(t, err) {
		return
	}
	assert.Equal(t, 2, copied, "should have copied the existing ELRS files")

	assert.FileExists(t, filepath.Join(destDir, "SCRIPTS/ELRS/crsf.lua"))
	assert.FileExists(t, filepath.Join(destDir, "SCRIPTS/ELRS/shim.lua"))
}

func TestCountFiles(t *testing.T) {
	srcDir := t.TempDir()
	createSourceTree(t, srcDir)

	count := CountFiles(srcDir, []string{
		"SCRIPTS/ELRS",
		"SCRIPTS/TOOLS/ExpressLRS",
		"WIDGETS/ELRSTelemetry",
	}, nil)
	assert.Equal(t, 4, count)
}

func TestCountFiles_MissingPathIgnored(t *testing.T) {
	srcDir := t.TempDir()
	createSourceTree(t, srcDir)

	count := CountFiles(srcDir, []string{
		"DOES_NOT_EXIST",
		"SCRIPTS/ELRS",
	}, nil)
	assert.Equal(t, 2, count)
}

func TestCountFiles_WithExclude(t *testing.T) {
	srcDir := t.TempDir()
	createSourceTree(t, srcDir)

	count := CountFiles(srcDir, []string{"SCRIPTS/ELRS"}, []string{"crsf.lua"})
	assert.Equal(t, 1, count, "crsf.lua should be excluded from count")
}

func TestCopyPaths_ExcludesFiles(t *testing.T) {
	srcDir := t.TempDir()
	destDir := t.TempDir()
	createSourceTree(t, srcDir)

	// Add an extra file that we will exclude.
	presetsPath := filepath.Join(srcDir, "WIDGETS/ELRSTelemetry/presets.txt")
	if !assert.NoError(t, os.WriteFile(presetsPath, []byte("user prefs"), 0o644)) {
		return
	}

	opts := CopyOptions{
		Exclude: []string{"presets.txt"},
	}

	copied, err := CopyPaths(srcDir, destDir, []string{"WIDGETS/ELRSTelemetry"}, opts)
	if !assert.NoError(t, err) {
		return
	}
	assert.Equal(t, 1, copied, "only main.lua should be copied, presets.txt excluded")

	assert.FileExists(t, filepath.Join(destDir, "WIDGETS/ELRSTelemetry/main.lua"))
	assert.NoFileExists(t, filepath.Join(destDir, "WIDGETS/ELRSTelemetry/presets.txt"))
}

func TestCopyPaths_ExcludeGlob(t *testing.T) {
	srcDir := t.TempDir()
	destDir := t.TempDir()
	createSourceTree(t, srcDir)

	opts := CopyOptions{
		Exclude: []string{"*.lua"},
	}

	copied, err := CopyPaths(srcDir, destDir, []string{"SCRIPTS/ELRS"}, opts)
	if !assert.NoError(t, err) {
		return
	}
	assert.Equal(t, 0, copied, "all .lua files should be excluded")
}

func TestCopyPaths_DefaultExcludeLuac(t *testing.T) {
	srcDir := t.TempDir()
	destDir := t.TempDir()
	createSourceTree(t, srcDir)

	// Add compiled .luac files alongside the .lua sources.
	for _, rel := range []string{"SCRIPTS/ELRS/crsf.luac", "SCRIPTS/ELRS/shim.luac"} {
		p := filepath.Join(srcDir, rel)
		if !assert.NoError(t, os.WriteFile(p, []byte("compiled"), 0o644)) {
			return
		}
	}

	opts := CopyOptions{Exclude: DefaultExclude}

	copied, err := CopyPaths(srcDir, destDir, []string{"SCRIPTS/ELRS"}, opts)
	if !assert.NoError(t, err) {
		return
	}
	assert.Equal(t, 2, copied, "only .lua files should be copied, .luac excluded via DefaultExclude")

	assert.FileExists(t, filepath.Join(destDir, "SCRIPTS/ELRS/crsf.lua"))
	assert.FileExists(t, filepath.Join(destDir, "SCRIPTS/ELRS/shim.lua"))
	assert.NoFileExists(t, filepath.Join(destDir, "SCRIPTS/ELRS/crsf.luac"))
	assert.NoFileExists(t, filepath.Join(destDir, "SCRIPTS/ELRS/shim.luac"))
}

func TestCountFiles_DefaultExcludeLuac(t *testing.T) {
	srcDir := t.TempDir()
	createSourceTree(t, srcDir)

	luacPath := filepath.Join(srcDir, "SCRIPTS/ELRS/crsf.luac")
	if !assert.NoError(t, os.WriteFile(luacPath, []byte("compiled"), 0o644)) {
		return
	}

	count := CountFiles(srcDir, []string{"SCRIPTS/ELRS"}, DefaultExclude)
	assert.Equal(t, 2, count, ".luac should be excluded via DefaultExclude")
}
