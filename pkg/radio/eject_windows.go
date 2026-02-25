//go:build windows

package radio

import (
	"fmt"
	"os/exec"
	"strings"

	"github.com/edgetx/cli/pkg/logging"
)

// Eject removes the volume at mountPoint using mountvol on Windows.
func Eject(mountPoint string) error {
	logging.Info("ejecting device...")

	driveLetter := mountPoint
	if len(driveLetter) >= 2 && driveLetter[1] == ':' {
		driveLetter = driveLetter[:2] + `\`
	}

	if out, err := exec.Command("mountvol", driveLetter, "/D").CombinedOutput(); err != nil {
		return fmt.Errorf("eject failed: %s: %w", strings.TrimSpace(string(out)), err)
	}

	logging.Infof("ejected %s", driveLetter)
	return nil
}
