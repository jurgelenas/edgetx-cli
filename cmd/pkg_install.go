package cmd

import (
	"fmt"
	"path/filepath"

	"github.com/jurgelenas/edgetx-cli/internal/packages"
	"github.com/jurgelenas/edgetx-cli/internal/radio"
	"github.com/jurgelenas/edgetx-cli/internal/repository"
	"github.com/jurgelenas/edgetx-cli/pkg/source"
	"github.com/pterm/pterm"
	"github.com/spf13/cobra"
)

var (
	pkgInstallDir    string
	pkgInstallEject  bool
	pkgInstallDryRun bool
	pkgInstallDev    bool
	pkgInstallPath   string
)

var pkgInstallCmd = &cobra.Command{
	Use:   "install <package>",
	Short: "Install a package to the SD card",
	Long: `Install a package from a Git repository or local directory to a connected
EdgeTX radio's SD card.

Use :: to specify an alternate manifest file or subdirectory within the repo,
and @ to pin a version (tag, branch, or commit). The --path flag can also be
used instead of inline ::.

Examples:
  edgetx-cli pkg install ExpressLRS/Lua-Scripts
  edgetx-cli pkg install ExpressLRS/Lua-Scripts@v1.6.0
  edgetx-cli pkg install Org/Repo::edgetx.c480x272.yml@branch
  edgetx-cli pkg install Org/Repo --path edgetx.c480x272.yml
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
	pkgInstallCmd.Flags().StringVar(&pkgInstallPath, "path", "", "manifest file or subdirectory within the repo")
	pkgCmd.AddCommand(pkgInstallCmd)
}

func runPkgInstall(cmd *cobra.Command, args []string) error {
	src := source.Parse(args[0])
	refInput := src.Base
	if src.Version != "" {
		refInput += "@" + src.Version
	}
	ref, err := repository.ParsePackageRef(refInput)
	if err != nil {
		return err
	}
	// --path flag overrides inline ::
	if pkgInstallPath != "" {
		ref.SubPath = pkgInstallPath
	} else {
		ref.SubPath = src.SubPath
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
