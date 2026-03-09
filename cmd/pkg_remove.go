package cmd

import (
	"path/filepath"

	"github.com/jurgelenas/edgetx-cli/pkg/packages"
	"github.com/jurgelenas/edgetx-cli/pkg/radio"
	"github.com/jurgelenas/edgetx-cli/pkg/source"
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
or by its display name (e.g. expresslrs). Use :: to target a specific subpath
variant, or use --path.

Examples:
  edgetx-cli pkg remove ExpressLRS/Lua-Scripts
  edgetx-cli pkg remove expresslrs
  edgetx-cli pkg remove Org/Repo::edgetx.c480x272.yml
  edgetx-cli pkg remove Org/Repo --path edgetx.c480x272.yml
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

	query := source.Parse(args[0]).WithSubPath(pkgRemovePath).Full()

	prepared, err := packages.PrepareRemove(packages.RemoveOptions{
		SDRoot: sdRoot,
		Query:  query,
	})
	if err != nil {
		return err
	}

	pterm.DefaultHeader.Println(prepared.Package.Name)
	pterm.Println()

	if pkgRemoveDryRun {
		result, err := prepared.Execute(true, nil)
		if err != nil {
			return err
		}
		pterm.Warning.Println("Would remove the following paths:")
		for _, p := range result.Package.Paths {
			pterm.Printfln("  %s", p)
		}
	} else {
		bar, _ := pterm.DefaultProgressbar.
			WithTotal(prepared.TotalFiles()).
			WithTitle("Removing").
			Start()

		onFile := func(path string) {
			bar.UpdateTitle(filepath.Base(path))
			bar.Increment()
		}

		result, err := prepared.Execute(false, onFile)
		bar.Stop()
		if err != nil {
			return err
		}

		pterm.Println()
		pterm.Success.Printfln("Removed %s (%s) - %d file(s)", result.Package.Name, result.Package.Source, result.FilesRemoved)
		for _, p := range result.Package.Paths {
			pterm.Printfln("  %s", p)
		}
	}

	if pkgRemoveEject && !pkgRemoveDryRun {
		return radio.Eject(sdRoot)
	}

	return nil
}
