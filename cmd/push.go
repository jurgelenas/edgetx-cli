package cmd

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/edgetx/cli/pkg/logging"
	"github.com/edgetx/cli/pkg/manifest"
	"github.com/edgetx/cli/pkg/radio"
	"github.com/pterm/pterm"
	"github.com/spf13/cobra"
)

var (
	srcDir string
	eject  bool
	dryRun bool
)

var pushCmd = &cobra.Command{
	Use:   "push",
	Short: "Push package contents to an EdgeTX radio SD card",
	Long: `Push reads an edgetx.toml manifest from the source directory and copies
the declared content (libraries, tools, telemetry, functions, mixes, widgets) to a connected
EdgeTX radio's SD card.

The radio is auto-detected by scanning mounted volumes for the presence
of an edgetx.sdcard.version file.`,
	RunE: runPush,
}

func init() {
	pushCmd.Flags().StringVar(&srcDir, "src-dir", ".", "source directory containing edgetx.toml")
	pushCmd.Flags().BoolVar(&eject, "eject", false, "safely unmount and power off the radio after copying")
	pushCmd.Flags().BoolVar(&dryRun, "dry-run", false, "show what would be copied without writing anything")
	devCmd.AddCommand(pushCmd)
}

func runPush(cmd *cobra.Command, args []string) error {
	srcDir, err := filepath.Abs(srcDir)
	if err != nil {
		return fmt.Errorf("resolving source directory: %w", err)
	}

	logging.Debugf("loading manifest from %s", srcDir)

	m, err := manifest.Load(srcDir)
	if err != nil {
		return err
	}

	sourceRoot := m.SourceRoot(srcDir)

	pterm.DefaultHeader.Println(fmt.Sprintf("%s v%s", m.Package.Name, m.Package.Version))
	pterm.Println(m.Package.Description)
	pterm.Println()

	mediaDir, err := radio.DefaultMediaDir()
	if err != nil {
		return err
	}

	const detectTimeout = 60 * time.Second

	spinner, _ := pterm.DefaultSpinner.
		WithText("Waiting for EdgeTX radio...").
		Start()

	destDir, err := radio.WaitForMount(mediaDir, detectTimeout)
	if err != nil {
		spinner.Fail("No EdgeTX radio detected")
		return err
	}
	spinner.Success("EdgeTX radio detected")

	sdVersion := ""
	versionFile := filepath.Join(destDir, "edgetx.sdcard.version")
	if version, err := os.ReadFile(versionFile); err == nil {
		sdVersion = strings.TrimSpace(string(version))
	}

	pterm.Info.Printfln("Detected EdgeTX SD card at %s (v%s)", destDir, sdVersion)
	pterm.Println()

	if dryRun {
		pterm.Warning.Println("Dry-run mode: no files will be written")
		pterm.Println()
	}

	type categoryGroup struct {
		label string
		items []manifest.ContentItem
	}
	groups := []categoryGroup{
		{"Libraries", m.Libraries},
		{"Tools", m.Tools},
		{"Telemetry", m.Telemetry},
		{"Functions", m.Functions},
		{"Mixes", m.Mixes},
		{"Widgets", m.Widgets},
	}

	// Collect all items for counting and display.
	var allItems []manifest.ContentItem
	for _, g := range groups {
		if len(g.items) == 0 {
			continue
		}
		pterm.DefaultSection.Println(g.label)
		for _, item := range g.items {
			pterm.Printfln("  %s (%s)", item.Name, item.Path)
			allItems = append(allItems, item)
		}
	}

	pterm.Println()

	totalFiles := 0
	for _, item := range allItems {
		totalFiles += radio.CountFiles(sourceRoot, []string{item.Path}, item.Exclude)
	}

	bar, _ := pterm.DefaultProgressbar.
		WithTotal(totalFiles).
		WithTitle("Copying files").
		Start()

	onFile := func(dest string) {
		bar.UpdateTitle(filepath.Base(dest))
		bar.Increment()
		logging.Debugf("    %s", dest)
	}

	totalCopied := 0
	for _, item := range allItems {
		opts := radio.CopyOptions{
			DryRun:  dryRun,
			Exclude: item.Exclude,
			OnFile:  onFile,
		}
		n, err := radio.CopyPaths(sourceRoot, destDir, []string{item.Path}, opts)
		if err != nil {
			bar.Stop()
			return err
		}
		totalCopied += n
	}

	bar.Stop()
	pterm.Println()

	if dryRun {
		pterm.Warning.Printfln("Dry-run: would copy %d file(s) to %s", totalFiles, destDir)
	} else {
		pterm.Success.Printfln("Copied %d file(s) to %s", totalCopied, destDir)
	}

	if eject {
		return radio.Eject(destDir)
	}

	return nil
}
