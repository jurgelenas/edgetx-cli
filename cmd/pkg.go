package cmd

import (
	"time"

	"github.com/jurgelenas/edgetx-cli/internal/radio"
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
		if err := radio.ValidateSDDir(dirFlag); err != nil {
			return "", err
		}
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
	if v := radio.SDCardVersion(sdRoot); v != "" {
		pterm.Info.Printfln("SD card at %s (v%s)", sdRoot, v)
	} else {
		pterm.Info.Printfln("SD card at %s", sdRoot)
	}
	pterm.Println()
}
