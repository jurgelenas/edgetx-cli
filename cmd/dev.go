package cmd

import "github.com/spf13/cobra"

var devCmd = &cobra.Command{
	Use:   "dev",
	Short: "Development workflow commands",
}

func init() {
	rootCmd.AddCommand(devCmd)
}
