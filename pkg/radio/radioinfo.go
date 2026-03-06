package radio

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"

	"gopkg.in/yaml.v3"
)

// RadioInfo holds metadata read from RADIO/radio.yml on the SD card.
type RadioInfo struct {
	Semver string `yaml:"semver"`
	Board  string `yaml:"board"`
}

// LoadRadioInfo reads RADIO/radio.yml from the given SD card root.
// Returns (nil, nil) if the file does not exist.
func LoadRadioInfo(sdRoot string) (*RadioInfo, error) {
	path := filepath.Join(sdRoot, "RADIO", "radio.yml")

	data, err := os.ReadFile(path)
	if err != nil {
		if errors.Is(err, os.ErrNotExist) {
			return nil, nil
		}
		return nil, fmt.Errorf("reading radio info: %w", err)
	}

	var info RadioInfo
	if err := yaml.Unmarshal(data, &info); err != nil {
		return nil, fmt.Errorf("parsing radio info %s: %w", path, err)
	}

	return &info, nil
}
