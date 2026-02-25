//go:build darwin

package radio

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"
)

// DefaultMediaDir returns the Volumes directory on macOS.
func DefaultMediaDir() (string, error) {
	return "/Volumes", nil
}

// DetectMount scans mediaDir for a mounted EdgeTX SD card by looking for
// edgetx.sdcard.version in each subdirectory.
func DetectMount(mediaDir string) (string, error) {
	entries, err := os.ReadDir(mediaDir)
	if err != nil {
		return "", fmt.Errorf("scanning %s: %w", mediaDir, err)
	}

	var candidates []string
	for _, entry := range entries {
		if !entry.IsDir() {
			continue
		}
		mountPoint := filepath.Join(mediaDir, entry.Name())
		versionFile := filepath.Join(mountPoint, "edgetx.sdcard.version")
		if _, err := os.Stat(versionFile); err == nil {
			candidates = append(candidates, mountPoint)
		}
	}

	switch len(candidates) {
	case 0:
		return "", fmt.Errorf("no EdgeTX SD card detected under %s -- make sure the radio is connected in USB Storage mode", mediaDir)
	case 1:
		return candidates[0], nil
	default:
		return "", fmt.Errorf("multiple EdgeTX SD cards detected: %s -- disconnect extra devices", strings.Join(candidates, ", "))
	}
}
