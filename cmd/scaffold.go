package cmd

import (
	"fmt"
	"path/filepath"
	"strings"

	"github.com/edgetx/cli/pkg/scaffold"
	"github.com/pterm/pterm"
	"github.com/spf13/cobra"
)

var (
	scaffoldSrcDir  string
	scaffoldDepends string
)

var scaffoldCmd = &cobra.Command{
	Use:   "scaffold <type> <name>",
	Short: "Generate boilerplate for a new EdgeTX Lua script",
	Long: `Generate boilerplate for a new EdgeTX Lua script and update the manifeadd 

Supported types: tool, telemetry, function, mix, widget

Each type generates the correct Lua boilerplate with the required return table
and creates the file at the conventional path. The edgetx.toml manifest is
updated with a new entry for the script.`,
	Args: cobra.ExactArgs(2),
	RunE: runScaffold,
}

func init() {
	scaffoldCmd.Flags().StringVar(&scaffoldSrcDir, "src-dir", ".", "source directory containing edgetx.toml")
	scaffoldCmd.Flags().StringVar(&scaffoldDepends, "depends", "", "comma-separated library dependencies")
	devCmd.AddCommand(scaffoldCmd)
}

func runScaffold(cmd *cobra.Command, args []string) error {
	srcDir, err := filepath.Abs(scaffoldSrcDir)
	if err != nil {
		return fmt.Errorf("resolving source directory: %w", err)
	}

	var depends []string
	if scaffoldDepends != "" {
		depends = strings.Split(scaffoldDepends, ",")
		for i := range depends {
			depends[i] = strings.TrimSpace(depends[i])
		}
	}

	result, err := scaffold.Run(scaffold.Options{
		Type:    args[0],
		Name:    args[1],
		Depends: depends,
		SrcDir:  srcDir,
	})
	if err != nil {
		return err
	}

	pterm.Success.Printfln("Created %s", result.FilePath)
	pterm.Info.Printfln("Added [[%s]] entry for %q to edgetx.toml", scaffold.Types[args[0]].TOMLKey, args[1])

	return nil
}
