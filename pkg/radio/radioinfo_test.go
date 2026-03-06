package radio

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/stretchr/testify/assert"
)

func TestLoadRadioInfo_ValidFile(t *testing.T) {
	dir := t.TempDir()
	os.MkdirAll(filepath.Join(dir, "RADIO"), 0o755)
	os.WriteFile(filepath.Join(dir, "RADIO/radio.yml"), []byte("semver: 2.12.0\nboard: x10\n"), 0o644)

	info, err := LoadRadioInfo(dir)
	assert.NoError(t, err)
	assert.NotNil(t, info)
	assert.Equal(t, "2.12.0", info.Semver)
	assert.Equal(t, "x10", info.Board)
}

func TestLoadRadioInfo_MissingFile(t *testing.T) {
	dir := t.TempDir()

	info, err := LoadRadioInfo(dir)
	assert.NoError(t, err)
	assert.Nil(t, info)
}

func TestLoadRadioInfo_MalformedYAML(t *testing.T) {
	dir := t.TempDir()
	os.MkdirAll(filepath.Join(dir, "RADIO"), 0o755)
	os.WriteFile(filepath.Join(dir, "RADIO/radio.yml"), []byte("{{invalid yaml"), 0o644)

	_, err := LoadRadioInfo(dir)
	assert.Error(t, err)
	assert.Contains(t, err.Error(), "parsing radio info")
}

func TestLoadRadioInfo_MissingSemverField(t *testing.T) {
	dir := t.TempDir()
	os.MkdirAll(filepath.Join(dir, "RADIO"), 0o755)
	os.WriteFile(filepath.Join(dir, "RADIO/radio.yml"), []byte("board: x10\n"), 0o644)

	info, err := LoadRadioInfo(dir)
	assert.NoError(t, err)
	assert.NotNil(t, info)
	assert.Equal(t, "", info.Semver)
}
