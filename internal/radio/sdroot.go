package radio

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"
)

// ValidateSDDir checks that dir exists and is a directory, then ensures the
// RADIO/ subdirectory exists (creating it if necessary).
func ValidateSDDir(dir string) error {
	info, err := os.Stat(dir)
	if err != nil {
		return fmt.Errorf("directory %q does not exist", dir)
	}
	if !info.IsDir() {
		return fmt.Errorf("%q is not a directory", dir)
	}
	os.MkdirAll(fmt.Sprintf("%s/RADIO", dir), 0o755)
	return nil
}

// SDCardVersion reads the edgetx.sdcard.version file from the SD card root
// and returns the trimmed version string. Returns an empty string if the file
// does not exist or cannot be read.
func SDCardVersion(sdRoot string) string {
	data, err := os.ReadFile(filepath.Join(sdRoot, "edgetx.sdcard.version"))
	if err != nil {
		return ""
	}
	return strings.TrimSpace(string(data))
}
