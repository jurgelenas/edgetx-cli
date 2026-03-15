package cmd

import (
	"context"
	"fmt"
	"os"
	"os/exec"
	"os/signal"
	"path/filepath"
	"strings"
	"time"

	"github.com/jurgelenas/edgetx-cli/pkg/logging"
	"github.com/jurgelenas/edgetx-cli/pkg/manifest"
	"github.com/jurgelenas/edgetx-cli/pkg/simulator"
	"github.com/pterm/pterm"
	"github.com/spf13/cobra"
)

var (
	simRadio      string
	simSDCard     string
	simNoWatch    bool
	simReset      bool
	simHeadless   bool
	simTimeout    time.Duration
	simScreenshot string
	simScript     string
)

var devSimulatorCmd = &cobra.Command{
	Use:   "simulator",
	Short: "Run the EdgeTX WASM simulator",
	Long: `Run a WASM-based EdgeTX radio simulator with a native desktop window.

The simulator downloads pre-built WASM radio firmware, renders the LCD display,
and provides interactive controls for buttons, switches, and trims.

If the current directory contains an edgetx.yml manifest, the package is
automatically installed into the simulator's SD card and changes are watched.

Examples:
  edgetx dev simulator --radio tx16s
  edgetx dev simulator --radio nv14 --headless --timeout 5s --screenshot lcd.png
  edgetx dev simulator --radio tx16s --script test.txt`,
	RunE: runSimulator,
}

func init() {
	devSimulatorCmd.Flags().StringVar(&simRadio, "radio", "", "radio model (e.g., tx16s). Interactive picker if omitted")
	devSimulatorCmd.Flags().StringVar(&simSDCard, "sdcard", "", "custom SD card directory")
	devSimulatorCmd.Flags().BoolVar(&simNoWatch, "no-watch", false, "disable auto-sync when package detected")
	devSimulatorCmd.Flags().BoolVar(&simReset, "reset", false, "reset simulator SD card to defaults before starting")
	devSimulatorCmd.Flags().BoolVar(&simHeadless, "headless", false, "run without GUI window (for testing/CI)")
	devSimulatorCmd.Flags().DurationVar(&simTimeout, "timeout", 0, "auto-exit after duration (e.g., 5s, 30s)")
	devSimulatorCmd.Flags().StringVar(&simScreenshot, "screenshot", "", "save LCD framebuffer as PNG at exit")
	devSimulatorCmd.Flags().StringVar(&simScript, "script", "", "execute action script for automated testing")
	devCmd.AddCommand(devSimulatorCmd)
}

func runSimulator(cmd *cobra.Command, args []string) error {
	// WAMR's wasi-threads are killed by Go's async preemption signals (SIGURG).
	// GODEBUG=asyncpreemptoff=1 must be set before the Go runtime starts.
	// If not set, re-exec ourselves with it.
	if !strings.Contains(os.Getenv("GODEBUG"), "asyncpreemptoff=1") {
		exe, err := os.Executable()
		if err != nil {
			return fmt.Errorf("finding executable: %w", err)
		}
		godebug := os.Getenv("GODEBUG")
		if godebug != "" {
			godebug += ","
		}
		godebug += "asyncpreemptoff=1"

		c := exec.Command(exe, os.Args[1:]...)
		c.Stdin = os.Stdin
		c.Stdout = os.Stdout
		c.Stderr = os.Stderr
		c.Env = append(os.Environ(), "GODEBUG="+godebug)
		if err := c.Run(); err != nil {
			if exitErr, ok := err.(*exec.ExitError); ok {
				os.Exit(exitErr.ExitCode())
			}
			return err
		}
		return nil
	}

	// Fetch radio catalog.
	spinner, _ := pterm.DefaultSpinner.
		WithText("Fetching radio catalog...").
		Start()

	catalog, err := simulator.FetchCatalog()
	if err != nil {
		spinner.Fail("Failed to fetch radio catalog")
		return err
	}
	spinner.Success(fmt.Sprintf("Loaded %d radios", len(catalog)))

	// Select radio.
	var radio *simulator.RadioDef
	if simRadio != "" {
		radio, err = simulator.FindRadio(catalog, simRadio)
		if err != nil {
			return err
		}
	} else {
		// Interactive picker.
		names := make([]string, len(catalog))
		for i, r := range catalog {
			names[i] = fmt.Sprintf("%s (%dx%d, %d-bit)", r.Name, r.Display.W, r.Display.H, r.Display.Depth)
		}
		selected, err := pterm.DefaultInteractiveSelect.
			WithOptions(names).
			WithDefaultText("Select a radio").
			Show()
		if err != nil {
			return err
		}
		// Find the selected radio.
		for i, name := range names {
			if name == selected {
				radio = &catalog[i]
				break
			}
		}
		if radio == nil {
			return fmt.Errorf("no radio selected")
		}
	}

	pterm.Info.Printfln("Radio: %s (%dx%d, %d-bit depth)", radio.Name, radio.Display.W, radio.Display.H, radio.Display.Depth)

	// Download WASM binary.
	wasmSpinner, _ := pterm.DefaultSpinner.
		WithText(fmt.Sprintf("Downloading %s firmware...", radio.Name)).
		Start()

	wasmPath, err := simulator.EnsureWASM(radio, func(downloaded, total int64) {
		if total > 0 {
			pct := float64(downloaded) / float64(total) * 100
			wasmSpinner.UpdateText(fmt.Sprintf("Downloading firmware... %.0f%%", pct))
		}
	})
	if err != nil {
		wasmSpinner.Fail("Failed to download firmware")
		return err
	}
	wasmSpinner.Success("Firmware ready")

	// Resolve SD card directory.
	radioKey := radio.Key()
	sdcardDir := simSDCard
	if sdcardDir == "" {
		sdcardDir, err = simulator.SDCardPath(radioKey)
		if err != nil {
			return err
		}
	}
	settingsDir, err := simulator.SettingsPath(radioKey)
	if err != nil {
		return err
	}

	// Reset if requested.
	if simReset {
		logging.Info("resetting simulator SD card...")
		if err := simulator.Reset(sdcardDir, settingsDir); err != nil {
			return err
		}
	}

	// Ensure directory structure.
	if err := simulator.EnsureStructure(sdcardDir, settingsDir); err != nil {
		return err
	}

	pterm.Info.Printfln("SD card: %s", sdcardDir)

	// Check for package in CWD.
	var m *manifest.Manifest
	var watchDir string

	cwd, _ := os.Getwd()
	if m, err = manifest.Load(cwd); err == nil {
		pterm.Info.Printfln("Package detected: %s", m.Package.Name)

		// Install package into simulator SD card.
		installSpinner, _ := pterm.DefaultSpinner.
			WithText("Installing package into simulator...").
			Start()

		if err := simulator.InstallPackage(sdcardDir, m, cwd); err != nil {
			installSpinner.Fail("Failed to install package")
			return err
		}
		installSpinner.Success("Package installed")

		if !simNoWatch {
			watchDir = cwd
		}
	}

	// Resolve script path.
	scriptPath := simScript
	if scriptPath != "" {
		scriptPath, err = filepath.Abs(scriptPath)
		if err != nil {
			return fmt.Errorf("resolving script path: %w", err)
		}
	}

	// Print keyboard shortcuts.
	if !simHeadless {
		pterm.Println()
		pterm.Info.Println(simulator.PrintKeyboardShortcuts())
	}

	// Signal handling.
	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt)
	defer stop()

	// Create and run simulator.
	sim, err := simulator.New(simulator.Options{
		Radio:          radio,
		WASMPath:       wasmPath,
		SDCardDir:      sdcardDir,
		SettingsDir:    settingsDir,
		WatchDir:       watchDir,
		Manifest:       m,
		Headless:       simHeadless,
		Timeout:        simTimeout,
		ScreenshotPath: simScreenshot,
		ScriptPath:     scriptPath,
	})
	if err != nil {
		return err
	}

	pterm.Println()
	pterm.Info.Println("Starting simulator... (Ctrl+C to stop)")

	return sim.Run(ctx)
}
