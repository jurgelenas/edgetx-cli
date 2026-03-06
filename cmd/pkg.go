package cmd

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/jurgelenas/edgetx-cli/pkg/radio"
	"github.com/pterm/pterm"
	"github.com/spf13/cobra"
)

var pkgCmd = &cobra.Command{
	Use:   "pkg",
	Short: "Package management commands",
	Long:  "Install, update, remove, and list EdgeTX Lua script packages.",
}

func init() {
	rootCmd.AddCommand(pkgCmd)
}

// resolveSDRoot returns the --dir value if set, or auto-detects a connected
// radio's SD card mount point.
func resolveSDRoot(dirFlag string) (string, error) {
	if dirFlag != "" {
		info, err := os.Stat(dirFlag)
		if err != nil {
			return "", fmt.Errorf("directory %q does not exist", dirFlag)
		}
		if !info.IsDir() {
			return "", fmt.Errorf("%q is not a directory", dirFlag)
		}
		// Auto-create RADIO/ subdir for state file if needed.
		os.MkdirAll(fmt.Sprintf("%s/RADIO", dirFlag), 0o755)
		return dirFlag, nil
	}

	mediaDir, err := radio.DefaultMediaDir()
	if err != nil {
		return "", err
	}

	const detectTimeout = 60 * time.Second

	spinner, _ := pterm.DefaultSpinner.
		WithText("Waiting for EdgeTX radio...").
		Start()

	sdRoot, err := radio.WaitForMount(mediaDir, detectTimeout)
	if err != nil {
		spinner.Fail("No EdgeTX radio detected")
		return "", err
	}
	spinner.Success("EdgeTX radio detected at " + sdRoot)

	return sdRoot, nil
}

// printSDCardInfo prints the SD card version if available.
func printSDCardInfo(sdRoot string) {
	versionFile := filepath.Join(sdRoot, "edgetx.sdcard.version")
	if version, err := os.ReadFile(versionFile); err == nil {
		sdVersion := strings.TrimSpace(string(version))
		pterm.Info.Printfln("SD card at %s (v%s)", sdRoot, sdVersion)
	} else {
		pterm.Info.Printfln("SD card at %s", sdRoot)
	}
	pterm.Println()
}
