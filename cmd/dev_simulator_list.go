package cmd

import (
	"fmt"

	"github.com/jurgelenas/edgetx-cli/pkg/simulator"
	"github.com/pterm/pterm"
	"github.com/spf13/cobra"
)

var devSimulatorListCmd = &cobra.Command{
	Use:   "list",
	Short: "List available radio models for the simulator",
	Long:  "Fetch and display all radio models available from the EdgeTX WASM simulator catalog.",
	RunE:  runSimulatorList,
}

func init() {
	devSimulatorCmd.AddCommand(devSimulatorListCmd)
}

func runSimulatorList(cmd *cobra.Command, args []string) error {
	spinner, _ := pterm.DefaultSpinner.
		WithText("Fetching radio catalog...").
		Start()

	catalog, err := simulator.FetchCatalog()
	if err != nil {
		spinner.Fail("Failed to fetch radio catalog")
		return err
	}
	spinner.Success(fmt.Sprintf("Loaded %d radios", len(catalog)))

	pterm.Println()
	pterm.DefaultHeader.Printfln("Available Radios (%d)", len(catalog))

	tableData := pterm.TableData{
		{"Name", "Display", "Depth", "WASM"},
	}

	for _, r := range catalog {
		display := fmt.Sprintf("%dx%d", r.Display.W, r.Display.H)
		depth := fmt.Sprintf("%d-bit", r.Display.Depth)
		tableData = append(tableData, []string{
			r.Name,
			display,
			depth,
			r.WASM,
		})
	}

	pterm.DefaultTable.WithHasHeader().WithData(tableData).Render()
	return nil
}
