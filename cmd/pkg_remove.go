package cmd

import (
	"github.com/jurgelenas/edgetx-cli/pkg/packages"
	"github.com/jurgelenas/edgetx-cli/pkg/radio"
	"github.com/pterm/pterm"
	"github.com/spf13/cobra"
)

var (
	pkgRemoveDir    string
	pkgRemovePath   string
	pkgRemoveEject  bool
	pkgRemoveDryRun bool
)

var pkgRemoveCmd = &cobra.Command{
	Use:   "remove <package>",
	Short: "Remove an installed package from the SD card",
	Long: `Remove an installed package and all its files from the SD card.

The package can be specified by its source path (e.g. ExpressLRS/Lua-Scripts)
or by its display name (e.g. expresslrs).

Examples:
  edgetx-cli pkg remove ExpressLRS/Lua-Scripts
  edgetx-cli pkg remove expresslrs
  edgetx-cli pkg remove expresslrs --eject`,
	Args: cobra.ExactArgs(1),
	RunE: runPkgRemove,
}

func init() {
	pkgRemoveCmd.Flags().StringVar(&pkgRemoveDir, "dir", "", "SD card directory (auto-detect if not set)")
	pkgRemoveCmd.Flags().StringVar(&pkgRemovePath, "path", "", "manifest file or subdirectory within the repo")
	pkgRemoveCmd.Flags().BoolVar(&pkgRemoveEject, "eject", false, "safely unmount radio after removal")
	pkgRemoveCmd.Flags().BoolVar(&pkgRemoveDryRun, "dry-run", false, "show what would be removed without deleting anything")
	pkgCmd.AddCommand(pkgRemoveCmd)
}

func runPkgRemove(cmd *cobra.Command, args []string) error {
	sdRoot, err := resolveSDRoot(pkgRemoveDir)
	if err != nil {
		return err
	}

	printSDCardInfo(sdRoot)

	if pkgRemoveDryRun {
		pterm.Warning.Println("Dry-run mode: no files will be deleted")
		pterm.Println()
	}

	query := insertSubPath(args[0], pkgRemovePath)

	result, err := packages.Remove(packages.RemoveOptions{
		SDRoot: sdRoot,
		Query:  query,
		DryRun: pkgRemoveDryRun,
	})
	if err != nil {
		return err
	}

	pterm.DefaultHeader.Println(result.Package.Name)
	pterm.Println()

	if pkgRemoveDryRun {
		pterm.Warning.Println("Would remove the following paths:")
		for _, p := range result.Package.Paths {
			pterm.Printfln("  %s", p)
		}
	} else {
		pterm.Success.Printfln("Removed %s (%s)", result.Package.Name, result.Package.Source)
		for _, p := range result.Package.Paths {
			pterm.Printfln("  %s", p)
		}
	}

	if pkgRemoveEject && !pkgRemoveDryRun {
		return radio.Eject(sdRoot)
	}

	return nil
}
