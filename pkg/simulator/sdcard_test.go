package simulator

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func TestEnsureStructure(t *testing.T) {
	tmpDir := t.TempDir()
	sdcardDir := filepath.Join(tmpDir, "sdcard")
	settingsDir := filepath.Join(tmpDir, "settings")

	err := EnsureStructure(sdcardDir, settingsDir)
	require.NoError(t, err)

	// Verify all expected directories exist.
	for _, dir := range sdcardDirs {
		path := filepath.Join(sdcardDir, dir)
		info, err := os.Stat(path)
		require.NoError(t, err, "directory %s should exist", dir)
		assert.True(t, info.IsDir(), "%s should be a directory", dir)
	}

	// Verify settings directory exists.
	info, err := os.Stat(settingsDir)
	require.NoError(t, err)
	assert.True(t, info.IsDir())
}

func TestReset(t *testing.T) {
	tmpDir := t.TempDir()
	sdcardDir := filepath.Join(tmpDir, "sdcard")
	settingsDir := filepath.Join(tmpDir, "settings")

	// Create structure first.
	require.NoError(t, EnsureStructure(sdcardDir, settingsDir))

	// Add a file that should be removed on reset.
	testFile := filepath.Join(sdcardDir, "SCRIPTS", "TOOLS", "test.lua")
	require.NoError(t, os.WriteFile(testFile, []byte("-- test"), 0o644))

	// Reset.
	err := Reset(sdcardDir, settingsDir)
	require.NoError(t, err)

	// File should be gone.
	_, err = os.Stat(testFile)
	assert.True(t, os.IsNotExist(err))

	// Structure should still exist.
	for _, dir := range sdcardDirs {
		path := filepath.Join(sdcardDir, dir)
		_, err := os.Stat(path)
		assert.NoError(t, err, "directory %s should exist after reset", dir)
	}
}

func TestEnsureStructure_Idempotent(t *testing.T) {
	tmpDir := t.TempDir()
	sdcardDir := filepath.Join(tmpDir, "sdcard")
	settingsDir := filepath.Join(tmpDir, "settings")

	// Call twice — should not error.
	require.NoError(t, EnsureStructure(sdcardDir, settingsDir))
	require.NoError(t, EnsureStructure(sdcardDir, settingsDir))
}
