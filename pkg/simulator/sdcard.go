package simulator

import (
	"fmt"
	"os"
	"path/filepath"

	"github.com/jurgelenas/edgetx-cli/pkg/manifest"
	"github.com/jurgelenas/edgetx-cli/pkg/radio"
)

var sdcardDirs = []string{
	"RADIO",
	"MODELS",
	"SCRIPTS/TOOLS",
	"SCRIPTS/TELEMETRY",
	"SCRIPTS/FUNCTIONS",
	"SCRIPTS/MIXES",
	"SCRIPTS/WIDGETS",
	"SOUNDS",
	"IMAGES",
}

// SDCardPath returns the default SD card directory for a radio.
func SDCardPath(radioKey string) (string, error) {
	cache, err := cacheDir()
	if err != nil {
		return "", err
	}
	return filepath.Join(cache, radioKey, "sdcard"), nil
}

// SettingsPath returns the default settings directory for a radio.
func SettingsPath(radioKey string) (string, error) {
	cache, err := cacheDir()
	if err != nil {
		return "", err
	}
	return filepath.Join(cache, radioKey, "settings"), nil
}

// EnsureStructure creates the standard EdgeTX SD card directory structure.
func EnsureStructure(sdcardDir, settingsDir string) error {
	for _, dir := range sdcardDirs {
		if err := os.MkdirAll(filepath.Join(sdcardDir, dir), 0o755); err != nil {
			return fmt.Errorf("creating %s: %w", dir, err)
		}
	}
	if err := os.MkdirAll(settingsDir, 0o755); err != nil {
		return fmt.Errorf("creating settings dir: %w", err)
	}
	return nil
}

// Reset removes and recreates the SD card and settings directories.
func Reset(sdcardDir, settingsDir string) error {
	if err := os.RemoveAll(sdcardDir); err != nil {
		return fmt.Errorf("removing SD card dir: %w", err)
	}
	if err := os.RemoveAll(settingsDir); err != nil {
		return fmt.Errorf("removing settings dir: %w", err)
	}
	return EnsureStructure(sdcardDir, settingsDir)
}

// InstallPackage copies a package's content items into the simulator SD card.
func InstallPackage(sdcardDir string, m *manifest.Manifest, manifestDir string) error {
	items := m.ContentItems(true)
	for _, item := range items {
		sourceRoot, err := m.ResolveContentPath(manifestDir, item.Path)
		if err != nil {
			return fmt.Errorf("resolving %s: %w", item.Path, err)
		}
		exclude := append(radio.DefaultExclude, item.Exclude...)
		_, err = radio.CopyPaths(sourceRoot, sdcardDir, []string{item.Path}, radio.CopyOptions{
			Exclude: exclude,
		})
		if err != nil {
			return fmt.Errorf("copying %s: %w", item.Path, err)
		}
	}
	return nil
}
