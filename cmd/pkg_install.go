package cmd

import (
	"fmt"
	"path/filepath"

	"github.com/jurgelenas/edgetx-cli/pkg/packages"
	"github.com/jurgelenas/edgetx-cli/pkg/radio"
	"github.com/jurgelenas/edgetx-cli/pkg/repository"
	"github.com/pterm/pterm"
	"github.com/spf13/cobra"
)

var (
	pkgInstallDir    string
	pkgInstallEject  bool
	pkgInstallDryRun bool
	pkgInstallDev    bool
)

var pkgInstallCmd = &cobra.Command{
	Use:   "install <package>",
	Short: "Install a package to the SD card",
	Long: `Install a package from a Git repository or local directory to a connected
EdgeTX radio's SD card.

Examples:
  edgetx-cli pkg install ExpressLRS/Lua-Scripts
  edgetx-cli pkg install ExpressLRS/Lua-Scripts@v1.6.0
  edgetx-cli pkg install .
  edgetx-cli pkg install ./my-project --dir /tmp/sdcard`,
	Args: cobra.ExactArgs(1),
	RunE: runPkgInstall,
}

func init() {
	pkgInstallCmd.Flags().StringVar(&pkgInstallDir, "dir", "", "SD card directory (auto-detect if not set)")
	pkgInstallCmd.Flags().BoolVar(&pkgInstallEject, "eject", false, "safely unmount radio after install")
	pkgInstallCmd.Flags().BoolVar(&pkgInstallDryRun, "dry-run", false, "show what would be installed without writing anything")
	pkgInstallCmd.Flags().BoolVar(&pkgInstallDev, "dev", false, "include development dependencies")
	pkgCmd.AddCommand(pkgInstallCmd)
}

func runPkgInstall(cmd *cobra.Command, args []string) error {
	ref, err := repository.ParsePackageRef(args[0])
	if err != nil {
		return err
	}

	sdRoot, err := resolveSDRoot(pkgInstallDir)
	if err != nil {
		return err
	}

	printSDCardInfo(sdRoot)

	if pkgInstallDryRun {
		pterm.Warning.Println("Dry-run mode: no files will be written")
		pterm.Println()
	}

	// Prepare: resolve manifest, check conflicts.
	var prepared *packages.PreparedInstall
	if !ref.IsLocal {
		spinner, _ := pterm.DefaultSpinner.
			WithText(fmt.Sprintf("Fetching %s...", ref.Canonical())).
			Start()

		prepared, err = packages.PrepareInstall(packages.InstallOptions{
			SDRoot: sdRoot,
			Ref:    ref,
			Dev:    pkgInstallDev,
		})
		if err != nil {
			spinner.Fail("Failed to fetch package")
			return err
		}
		spinner.Success(fmt.Sprintf("Fetched %s", prepared.Package.Name))
	} else {
		prepared, err = packages.PrepareInstall(packages.InstallOptions{
			SDRoot: sdRoot,
			Ref:    ref,
			Dev:    pkgInstallDev,
		})
		if err != nil {
			return err
		}
	}

	// Header.
	pterm.DefaultHeader.Println(prepared.Package.Name)
	if prepared.Manifest.Package.Description != "" {
		pterm.Println(prepared.Manifest.Package.Description)
	}
	pterm.Println()

	// Progress bar.
	totalFiles := prepared.TotalFiles()

	bar, _ := pterm.DefaultProgressbar.
		WithTotal(totalFiles).
		WithTitle("Installing").
		Start()

	onFile := func(dest string) {
		bar.UpdateTitle(filepath.Base(dest))
		bar.Increment()
	}

	result, err := prepared.Execute(sdRoot, pkgInstallDryRun, onFile)
	bar.Stop()
	if err != nil {
		return err
	}

	pterm.Println()

	if pkgInstallDryRun {
		pterm.Warning.Printfln("Dry-run: would install %d file(s) to %s", totalFiles, sdRoot)
	} else {
		pterm.Success.Printfln("Installed %d file(s) to %s", result.FilesCopied, sdRoot)
	}

	printChannelInfo(result.Package)

	if pkgInstallEject && !pkgInstallDryRun {
		return radio.Eject(sdRoot)
	}

	return nil
}

func printChannelInfo(pkg packages.InstalledPackage) {
	info := pkg.Channel
	if pkg.Version != "" {
		info += " " + pkg.Version
	}
	if pkg.Commit != "" && len(pkg.Commit) > 7 {
		info += " (" + pkg.Commit[:7] + ")"
	}
	pterm.Info.Printfln("Channel: %s", info)
}
