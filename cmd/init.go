package cmd

import (
	"fmt"
	"os"
	"path/filepath"

	"github.com/jurgelenas/edgetx-cli/pkg/manifest"
	"github.com/pterm/pterm"
	"github.com/spf13/cobra"
)

var initSrcDir string

var initCmd = &cobra.Command{
	Use:   "init [name]",
	Short: "Initialize a new edgetx.yml manifest",
	Long: `Create a new edgetx.yml manifest in the specified directory.

If a name argument is provided it is used as the package name, otherwise
the directory name is used.`,
	Args: cobra.MaximumNArgs(1),
	RunE: runInit,
}

func init() {
	initCmd.Flags().StringVar(&initSrcDir, "src-dir", ".", "directory to create edgetx.yml in")
	devCmd.AddCommand(initCmd)
}

func runInit(cmd *cobra.Command, args []string) error {
	dir, err := filepath.Abs(initSrcDir)
	if err != nil {
		return fmt.Errorf("resolving directory: %w", err)
	}

	ymlPath := filepath.Join(dir, manifest.FileName)
	if _, err := os.Stat(ymlPath); err == nil {
		return fmt.Errorf("%s already exists in %s", manifest.FileName, dir)
	}

	name := filepath.Base(dir)
	if len(args) > 0 {
		name = args[0]
	}

	content := fmt.Sprintf("package:\n  name: %s\n  description: \"\"\n  license: \"\"\n", name)

	if err := os.WriteFile(ymlPath, []byte(content), 0o644); err != nil {
		return fmt.Errorf("writing manifest: %w", err)
	}

	pterm.Success.Printfln("Created %s", ymlPath)
	return nil
}
