package radio

import (
	"strings"
	"time"
)

const noCardPrefix = "no EdgeTX SD card detected"

func isNoDeviceError(err error) bool {
	return err != nil && strings.HasPrefix(err.Error(), noCardPrefix)
}

// WaitForMount polls DetectMount at the given interval until a device is found
// or the timeout expires. Non-retryable errors (e.g. multiple devices) are
// returned immediately.
func WaitForMount(mediaDir string, timeout time.Duration) (string, error) {
	const pollInterval = 500 * time.Millisecond
	deadline := time.Now().Add(timeout)

	for {
		mount, err := DetectMount(mediaDir)
		if err == nil {
			return mount, nil
		}
		if !isNoDeviceError(err) {
			return "", err
		}
		if time.Now().After(deadline) {
			return "", err
		}
		time.Sleep(pollInterval)
	}
}
