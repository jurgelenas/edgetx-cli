package cmd

import (
	"os"

	"github.com/edgetx/cli/pkg/logging"
	"github.com/pterm/pterm"
	"github.com/spf13/cobra"
)

var (
	verbose   bool
	logFormat string
)

var rootCmd = &cobra.Command{
	Use:   "edgetx-cli",
	Short: "CLI tool for managing EdgeTX radios",
	Long:  "A command-line interface for managing EdgeTX radio SD cards, packages, and configurations.",
	PersistentPreRun: func(cmd *cobra.Command, args []string) {
		level := "info"
		if verbose {
			level = "debug"
		}
		logging.Setup(logging.Config{
			Level:  level,
			Format: logFormat,
		})
		if logFormat == "json" {
			pterm.DisableOutput()
		}
	},
}

func Execute() {
	if err := rootCmd.Execute(); err != nil {
		os.Exit(1)
	}
}

func init() {
	rootCmd.PersistentFlags().BoolVarP(&verbose, "verbose", "v", false, "enable debug logging")
	rootCmd.PersistentFlags().StringVar(&logFormat, "log-format", "text", "log output format (text, json)")
}
