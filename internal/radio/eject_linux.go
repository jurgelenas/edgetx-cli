//go:build linux

package radio

import (
	"fmt"
	"os/exec"
	"strings"

	"github.com/jurgelenas/edgetx-cli/pkg/logging"
)

// Eject syncs, unmounts, and powers off the block device backing mountPoint.
func Eject(mountPoint string) error {
	logging.Info("ejecting device...")

	out, err := exec.Command("findmnt", "-no", "SOURCE", mountPoint).Output()
	if err != nil {
		return fmt.Errorf("could not determine block device for %s: %w", mountPoint, err)
	}

	blockDevice := strings.TrimSpace(string(out))
	if blockDevice == "" {
		return fmt.Errorf("could not determine block device for %s", mountPoint)
	}

	disk := stripPartitionNumber(blockDevice)

	if err := exec.Command("sync").Run(); err != nil {
		return fmt.Errorf("sync failed: %w", err)
	}

	if out, err := exec.Command("udisksctl", "unmount", "-b", blockDevice, "--no-user-interaction").CombinedOutput(); err != nil {
		return fmt.Errorf("unmount failed: %s: %w", strings.TrimSpace(string(out)), err)
	}

	if out, err := exec.Command("udisksctl", "power-off", "-b", disk, "--no-user-interaction").CombinedOutput(); err != nil {
		return fmt.Errorf("power-off failed: %s: %w", strings.TrimSpace(string(out)), err)
	}

	logging.Infof("ejected %s (%s)", blockDevice, disk)
	return nil
}

func stripPartitionNumber(device string) string {
	d := device
	for len(d) > 0 && d[len(d)-1] >= '0' && d[len(d)-1] <= '9' {
		d = d[:len(d)-1]
	}
	return d
}
