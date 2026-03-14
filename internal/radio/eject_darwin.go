//go:build darwin

package radio

import (
	"fmt"
	"os/exec"
	"strings"

	"github.com/jurgelenas/edgetx-cli/pkg/logging"
)

// Eject unmounts and ejects the volume at mountPoint using diskutil.
func Eject(mountPoint string) error {
	logging.Info("ejecting device...")

	if out, err := exec.Command("diskutil", "unmount", mountPoint).CombinedOutput(); err != nil {
		return fmt.Errorf("unmount failed: %s: %w", strings.TrimSpace(string(out)), err)
	}

	if out, err := exec.Command("diskutil", "eject", mountPoint).CombinedOutput(); err != nil {
		return fmt.Errorf("eject failed: %s: %w", strings.TrimSpace(string(out)), err)
	}

	logging.Infof("ejected %s", mountPoint)
	return nil
}
