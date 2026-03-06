package cmd

import (
	"github.com/jurgelenas/edgetx-cli/pkg/packages"
	"github.com/pterm/pterm"
	"github.com/spf13/cobra"
)

var pkgListDir string

var pkgListCmd = &cobra.Command{
	Use:   "list",
	Short: "List installed packages",
	Long:  "List all packages installed on the SD card.",
	RunE:  runPkgList,
}

func init() {
	pkgListCmd.Flags().StringVar(&pkgListDir, "dir", "", "SD card directory (auto-detect if not set)")
	pkgCmd.AddCommand(pkgListCmd)
}

func runPkgList(cmd *cobra.Command, args []string) error {
	sdRoot, err := resolveSDRoot(pkgListDir)
	if err != nil {
		return err
	}

	printSDCardInfo(sdRoot)

	state, err := packages.LoadState(sdRoot)
	if err != nil {
		return err
	}

	if len(state.Packages) == 0 {
		pterm.Info.Println("No packages installed")
		return nil
	}

	pterm.DefaultHeader.Printfln("Installed Packages (%d)", len(state.Packages))

	tableData := pterm.TableData{
		{"Source", "Name", "Channel", "Version", "Commit"},
	}

	for _, pkg := range state.Packages {
		commit := ""
		if len(pkg.Commit) > 7 {
			commit = pkg.Commit[:7]
		} else {
			commit = pkg.Commit
		}
		tableData = append(tableData, []string{
			pkg.Source,
			pkg.Name,
			pkg.Channel,
			pkg.Version,
			commit,
		})
	}

	pterm.DefaultTable.WithHasHeader().WithData(tableData).Render()
	return nil
}
