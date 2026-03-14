package radio

import (
	"archive/zip"
	"os"
	"path/filepath"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func createBackupSourceTree(t *testing.T, base string) {
	t.Helper()
	files := map[string]string{
		"SCRIPTS/ELRS/crsf.lua":          "-- crsf lib",
		"SCRIPTS/TOOLS/elrs.lua":         "-- elrs tool",
		"WIDGETS/ELRSTelemetry/main.lua": "-- telemetry widget",
		"edgetx.sdcard.version":          "v2.10.0",
	}
	for relPath, content := range files {
		fullPath := filepath.Join(base, relPath)
		require.NoError(t, os.MkdirAll(filepath.Dir(fullPath), 0o755))
		require.NoError(t, os.WriteFile(fullPath, []byte(content), 0o644))
	}
}

func TestBackupDir_CopiesAllFiles(t *testing.T) {
	srcDir := t.TempDir()
	destDir := t.TempDir()
	createBackupSourceTree(t, srcDir)

	copied, err := BackupDir(srcDir, destDir, BackupOptions{})
	require.NoError(t, err)
	assert.Equal(t, 4, copied)

	content, err := os.ReadFile(filepath.Join(destDir, "SCRIPTS/ELRS/crsf.lua"))
	require.NoError(t, err)
	assert.Equal(t, "-- crsf lib", string(content))

	content, err = os.ReadFile(filepath.Join(destDir, "edgetx.sdcard.version"))
	require.NoError(t, err)
	assert.Equal(t, "v2.10.0", string(content))
}

func TestBackupDir_PreservesStructure(t *testing.T) {
	srcDir := t.TempDir()
	destDir := t.TempDir()
	createBackupSourceTree(t, srcDir)

	_, err := BackupDir(srcDir, destDir, BackupOptions{})
	require.NoError(t, err)

	assert.FileExists(t, filepath.Join(destDir, "SCRIPTS/ELRS/crsf.lua"))
	assert.FileExists(t, filepath.Join(destDir, "SCRIPTS/TOOLS/elrs.lua"))
	assert.FileExists(t, filepath.Join(destDir, "WIDGETS/ELRSTelemetry/main.lua"))
	assert.FileExists(t, filepath.Join(destDir, "edgetx.sdcard.version"))
}

func TestBackupDir_CallsOnFile(t *testing.T) {
	srcDir := t.TempDir()
	destDir := t.TempDir()
	createBackupSourceTree(t, srcDir)

	var calledFiles []string
	opts := BackupOptions{
		OnFile: func(dest string) {
			calledFiles = append(calledFiles, dest)
		},
	}

	copied, err := BackupDir(srcDir, destDir, opts)
	require.NoError(t, err)
	assert.Equal(t, 4, copied)
	assert.Len(t, calledFiles, 4)
}

func TestCompressDir_CreatesZip(t *testing.T) {
	srcDir := t.TempDir()
	createBackupSourceTree(t, srcDir)

	// Copy to a backup dir first (CompressDir removes the source).
	backupDir := filepath.Join(t.TempDir(), "backup")
	_, err := BackupDir(srcDir, backupDir, BackupOptions{})
	require.NoError(t, err)

	zipPath := filepath.Join(t.TempDir(), "backup.zip")

	var compressed []string
	err = CompressDir(backupDir, zipPath, func(relPath string) {
		compressed = append(compressed, relPath)
	})
	require.NoError(t, err)
	assert.Len(t, compressed, 4)

	// Verify zip contents.
	r, err := zip.OpenReader(zipPath)
	require.NoError(t, err)
	defer r.Close()

	var names []string
	for _, f := range r.File {
		names = append(names, f.Name)
	}
	assert.Contains(t, names, "SCRIPTS/ELRS/crsf.lua")
	assert.Contains(t, names, "SCRIPTS/TOOLS/elrs.lua")
	assert.Contains(t, names, "WIDGETS/ELRSTelemetry/main.lua")
	assert.Contains(t, names, "edgetx.sdcard.version")
}

func TestCompressDir_RemovesSourceDir(t *testing.T) {
	srcDir := t.TempDir()
	createBackupSourceTree(t, srcDir)

	backupDir := filepath.Join(t.TempDir(), "backup")
	_, err := BackupDir(srcDir, backupDir, BackupOptions{})
	require.NoError(t, err)

	zipPath := filepath.Join(t.TempDir(), "backup.zip")
	err = CompressDir(backupDir, zipPath, nil)
	require.NoError(t, err)

	_, err = os.Stat(backupDir)
	assert.True(t, os.IsNotExist(err), "source directory should be removed after compression")
}
