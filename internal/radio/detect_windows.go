//go:build windows

package radio

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"
)

// DefaultMediaDir is not applicable on Windows; an empty string signals
// that DetectMount should scan drive letters instead.
func DefaultMediaDir() (string, error) {
	return "", nil
}

// DetectMount scans for an EdgeTX SD card. On Windows, if mediaDir is empty
// it iterates drive letters D: through Z: looking for edgetx.sdcard.version.
// If mediaDir is provided it behaves like the Unix variants.
func DetectMount(mediaDir string) (string, error) {
	var searchDirs []string

	if mediaDir == "" {
		for letter := 'D'; letter <= 'Z'; letter++ {
			drive := string(letter) + `:\`
			if info, err := os.Stat(drive); err == nil && info.IsDir() {
				searchDirs = append(searchDirs, drive)
			}
		}
	} else {
		entries, err := os.ReadDir(mediaDir)
		if err != nil {
			return "", fmt.Errorf("scanning %s: %w", mediaDir, err)
		}
		for _, entry := range entries {
			if entry.IsDir() {
				searchDirs = append(searchDirs, filepath.Join(mediaDir, entry.Name()))
			}
		}
	}

	var candidates []string
	for _, dir := range searchDirs {
		versionFile := filepath.Join(dir, "edgetx.sdcard.version")
		if _, err := os.Stat(versionFile); err == nil {
			candidates = append(candidates, dir)
		}
	}

	switch len(candidates) {
	case 0:
		return "", fmt.Errorf("no EdgeTX SD card detected -- make sure the radio is connected in USB Storage mode")
	case 1:
		return candidates[0], nil
	default:
		return "", fmt.Errorf("multiple EdgeTX SD cards detected: %s -- disconnect extra devices", strings.Join(candidates, ", "))
	}
}
