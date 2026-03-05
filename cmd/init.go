package cmd

import (
	"fmt"
	"os"
	"path/filepath"

	"github.com/edgetx/cli/pkg/manifest"
	"github.com/pterm/pterm"
	"github.com/spf13/cobra"
)

var initSrcDir string

var initCmd = &cobra.Command{
	Use:   "init [name]",
	Short: "Initialize a new edgetx.toml manifest",
	Long: `Create a new edgetx.toml manifest in the specified directory.

If a name argument is provided it is used as the package name, otherwise
the directory name is used.`,
	Args: cobra.MaximumNArgs(1),
	RunE: runInit,
}

func init() {
	initCmd.Flags().StringVar(&initSrcDir, "src-dir", ".", "directory to create edgetx.toml in")
	devCmd.AddCommand(initCmd)
}

func runInit(cmd *cobra.Command, args []string) error {
	dir, err := filepath.Abs(initSrcDir)
	if err != nil {
		return fmt.Errorf("resolving directory: %w", err)
	}

	tomlPath := filepath.Join(dir, manifest.FileName)
	if _, err := os.Stat(tomlPath); err == nil {
		return fmt.Errorf("%s already exists in %s", manifest.FileName, dir)
	}

	name := filepath.Base(dir)
	if len(args) > 0 {
		name = args[0]
	}

	content := fmt.Sprintf(`[package]
name = %q
version = "0.1.0"
description = ""
`, name)

	if err := os.WriteFile(tomlPath, []byte(content), 0o644); err != nil {
		return fmt.Errorf("writing manifest: %w", err)
	}

	pterm.Success.Printfln("Created %s", tomlPath)
	return nil
}
