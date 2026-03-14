package cmd

import (
	"github.com/jurgelenas/edgetx-cli/internal/radio"
	"github.com/pterm/pterm"
	"github.com/spf13/cobra"
)

var ejectCmd = &cobra.Command{
	Use:   "eject",
	Short: "Safely eject a connected EdgeTX radio",
	Long: `Eject detects a connected EdgeTX radio and safely unmounts its SD card.

Examples:
  edgetx-cli eject`,
	RunE: runEject,
}

func init() {
	rootCmd.AddCommand(ejectCmd)
}

func runEject(cmd *cobra.Command, args []string) error {
	sdRoot, err := resolveSDRoot("")
	if err != nil {
		return err
	}

	printSDCardInfo(sdRoot)

	if err := radio.Eject(sdRoot); err != nil {
		return err
	}

	pterm.Success.Println("Radio safely ejected")
	return nil
}
