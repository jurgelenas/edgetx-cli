package radio

import (
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/stretchr/testify/assert"
)

func TestDetectMount_SingleCard(t *testing.T) {
	mediaDir := t.TempDir()

	radioDir := filepath.Join(mediaDir, "EDGETX_RADIO")
	if !assert.NoError(t, os.MkdirAll(radioDir, 0o755)) {
		return
	}
	if !assert.NoError(t, os.WriteFile(filepath.Join(radioDir, "edgetx.sdcard.version"), []byte("2.12"), 0o644)) {
		return
	}

	mount, err := DetectMount(mediaDir)
	if !assert.NoError(t, err) {
		return
	}
	assert.Equal(t, radioDir, mount)
}

func TestDetectMount_NoCard(t *testing.T) {
	mediaDir := t.TempDir()

	_, err := DetectMount(mediaDir)
	assert.Error(t, err)
	assert.Contains(t, err.Error(), "no EdgeTX SD card detected")
}

func TestDetectMount_MultipleCards(t *testing.T) {
	mediaDir := t.TempDir()

	for _, name := range []string{"RADIO_A", "RADIO_B"} {
		radioDir := filepath.Join(mediaDir, name)
		if !assert.NoError(t, os.MkdirAll(radioDir, 0o755)) {
			return
		}
		if !assert.NoError(t, os.WriteFile(filepath.Join(radioDir, "edgetx.sdcard.version"), []byte("2.12"), 0o644)) {
			return
		}
	}

	_, err := DetectMount(mediaDir)
	assert.Error(t, err)
	assert.Contains(t, err.Error(), "multiple EdgeTX SD cards detected")
}

func TestWaitForMount_ImmediateSuccess(t *testing.T) {
	mediaDir := t.TempDir()

	radioDir := filepath.Join(mediaDir, "EDGETX_RADIO")
	if !assert.NoError(t, os.MkdirAll(radioDir, 0o755)) {
		return
	}
	if !assert.NoError(t, os.WriteFile(filepath.Join(radioDir, "edgetx.sdcard.version"), []byte("2.12"), 0o644)) {
		return
	}

	mount, err := WaitForMount(mediaDir, 2*time.Second)
	if !assert.NoError(t, err) {
		return
	}
	assert.Equal(t, radioDir, mount)
}

func TestWaitForMount_DelayedDetection(t *testing.T) {
	mediaDir := t.TempDir()

	radioDir := filepath.Join(mediaDir, "EDGETX_RADIO")
	go func() {
		time.Sleep(800 * time.Millisecond)
		_ = os.MkdirAll(radioDir, 0o755)
		_ = os.WriteFile(filepath.Join(radioDir, "edgetx.sdcard.version"), []byte("2.12"), 0o644)
	}()

	mount, err := WaitForMount(mediaDir, 5*time.Second)
	if !assert.NoError(t, err) {
		return
	}
	assert.Equal(t, radioDir, mount)
}

func TestWaitForMount_Timeout(t *testing.T) {
	mediaDir := t.TempDir()

	_, err := WaitForMount(mediaDir, 1*time.Second)
	assert.Error(t, err)
	assert.Contains(t, err.Error(), "no EdgeTX SD card detected")
}

func TestWaitForMount_MultipleDevicesReturnsImmediately(t *testing.T) {
	mediaDir := t.TempDir()

	for _, name := range []string{"RADIO_A", "RADIO_B"} {
		radioDir := filepath.Join(mediaDir, name)
		if !assert.NoError(t, os.MkdirAll(radioDir, 0o755)) {
			return
		}
		if !assert.NoError(t, os.WriteFile(filepath.Join(radioDir, "edgetx.sdcard.version"), []byte("2.12"), 0o644)) {
			return
		}
	}

	start := time.Now()
	_, err := WaitForMount(mediaDir, 5*time.Second)
	elapsed := time.Since(start)

	assert.Error(t, err)
	assert.Contains(t, err.Error(), "multiple EdgeTX SD cards detected")
	assert.Less(t, elapsed, 1*time.Second, "should return immediately for non-retryable errors")
}
