package cmd

import (
	"fmt"
	"path/filepath"

	"github.com/jurgelenas/edgetx-cli/pkg/packages"
	"github.com/jurgelenas/edgetx-cli/pkg/radio"
	"github.com/pterm/pterm"
	"github.com/spf13/cobra"
)

var (
	pkgUpdateDir    string
	pkgUpdateAll    bool
	pkgUpdateEject  bool
	pkgUpdateDryRun bool
)

var pkgUpdateCmd = &cobra.Command{
	Use:   "update [package]",
	Short: "Update installed package(s)",
	Long: `Update an installed package to the latest version, or update all packages
with --all.

Examples:
  edgetx-cli pkg update .
  edgetx-cli pkg update ExpressLRS/Lua-Scripts
  edgetx-cli pkg update expresslrs
  edgetx-cli pkg update --all
  edgetx-cli pkg update --all --eject`,
	Args: cobra.MaximumNArgs(1),
	RunE: runPkgUpdate,
}

func init() {
	pkgUpdateCmd.Flags().StringVar(&pkgUpdateDir, "dir", "", "SD card directory (auto-detect if not set)")
	pkgUpdateCmd.Flags().BoolVar(&pkgUpdateAll, "all", false, "update all installed packages")
	pkgUpdateCmd.Flags().BoolVar(&pkgUpdateEject, "eject", false, "safely unmount radio after update")
	pkgUpdateCmd.Flags().BoolVar(&pkgUpdateDryRun, "dry-run", false, "show what would be updated without writing anything")
	pkgCmd.AddCommand(pkgUpdateCmd)
}

func runPkgUpdate(cmd *cobra.Command, args []string) error {
	sdRoot, err := resolveSDRoot(pkgUpdateDir)
	if err != nil {
		return err
	}

	printSDCardInfo(sdRoot)

	query := ""
	if len(args) > 0 {
		query = args[0]
	}

	if pkgUpdateDryRun {
		pterm.Warning.Println("Dry-run mode: no files will be written")
		pterm.Println()
	}

	spinner, _ := pterm.DefaultSpinner.
		WithText("Checking for updates...").
		Start()

	var bar *pterm.ProgressbarPrinter

	results, err := packages.Update(packages.UpdateOptions{
		SDRoot: sdRoot,
		Query:  query,
		All:    pkgUpdateAll,
		DryRun: pkgUpdateDryRun,
		BeforeCopy: func(name string, totalFiles int) {
			spinner.Success(fmt.Sprintf("Updating %s", name))
			pterm.Println()
			pterm.DefaultHeader.Println(name)
			bar, _ = pterm.DefaultProgressbar.
				WithTotal(totalFiles).
				WithTitle("Updating").
				Start()
		},
		OnFile: func(dest string) {
			if bar != nil {
				bar.UpdateTitle(filepath.Base(dest))
				bar.Increment()
			}
		},
	})

	if bar != nil {
		bar.Stop()
		pterm.Println()
	}

	if err != nil {
		if bar == nil {
			spinner.Fail("Update failed")
		}
		return err
	}

	if bar == nil {
		spinner.Success("Update check complete")
		pterm.Println()
	}

	for _, r := range results {
		if r.UpToDate {
			pterm.Info.Printfln("%s (%s) is already up to date", r.Package.Name, r.Package.Source)
			continue
		}

		info := fmt.Sprintf("%s -> %s", r.Package.Source, r.Package.Channel)
		if r.Package.Version != "" {
			info += " " + r.Package.Version
		}
		if r.Package.Commit != "" && len(r.Package.Commit) > 7 {
			info += " (" + r.Package.Commit[:7] + ")"
		}

		if r.FilesCopied > 0 {
			pterm.Success.Printfln("Updated %s: %d file(s) copied", info, r.FilesCopied)
		} else {
			pterm.Warning.Printfln("Would update %s", info)
		}
	}

	if pkgUpdateEject && !pkgUpdateDryRun {
		return radio.Eject(sdRoot)
	}

	return nil
}
