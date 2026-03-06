package cmd

import (
	"context"
	"fmt"
	"os"
	"os/signal"
	"path/filepath"
	"sync/atomic"

	"github.com/jurgelenas/edgetx-cli/pkg/logging"
	"github.com/jurgelenas/edgetx-cli/pkg/manifest"
	pkgsync "github.com/jurgelenas/edgetx-cli/pkg/sync"
	"github.com/pterm/pterm"
	"github.com/spf13/cobra"
)

var (
	syncSrcDir string
	syncNoDev  bool
)

var syncCmd = &cobra.Command{
	Use:   "sync <target-dir>",
	Short: "Watch source files and sync changes to a target directory",
	Long: `Sync reads an edgetx.yml manifest from the source directory and copies
all declared content (libraries, tools, telemetry, functions, mixes, widgets) to the specified
target directory. It then watches for file changes and syncs them
continuously until you press Ctrl+C.

This is useful during development to keep an EdgeTX simulator SD card
directory in sync with your source files. When you edit a Lua script in
your source repository, the change is automatically copied to the
simulator's SD card directory so you can immediately test it.

The source directory must contain an edgetx.yml manifest that declares
which content paths to sync. If the manifest includes a source_dir field,
paths are resolved relative to that subdirectory.

Examples:
  edgetx dev sync /path/to/edgetx-sdcard
  edgetx dev sync --src-dir ./expresslrs-lua-scripts ../edgetx-sdcard`,
	Args: cobra.ExactArgs(1),
	RunE: runSync,
}

func init() {
	syncCmd.Flags().StringVar(&syncSrcDir, "src-dir", ".", "source directory containing edgetx.yml")
	syncCmd.Flags().BoolVar(&syncNoDev, "no-dev", false, "exclude development dependencies from sync")
	devCmd.AddCommand(syncCmd)
}

func runSync(cmd *cobra.Command, args []string) error {
	srcDir, err := filepath.Abs(syncSrcDir)
	if err != nil {
		return fmt.Errorf("resolving source directory: %w", err)
	}

	targetDir, err := filepath.Abs(args[0])
	if err != nil {
		return fmt.Errorf("resolving target directory: %w", err)
	}

	if info, err := os.Stat(targetDir); err != nil {
		return fmt.Errorf("target directory %q does not exist", targetDir)
	} else if !info.IsDir() {
		return fmt.Errorf("target %q is not a directory", targetDir)
	}

	logging.Debugf("loading manifest from %s", srcDir)

	m, err := manifest.Load(srcDir)
	if err != nil {
		return err
	}

	sourceRoot := m.SourceRoot(srcDir)

	pterm.DefaultHeader.Println(m.Package.Name)
	pterm.Println(m.Package.Description)
	pterm.Println()
	pterm.Info.Printfln("Source: %s", sourceRoot)
	pterm.Info.Printfln("Target: %s", targetDir)
	pterm.Println()

	includeDev := !syncNoDev

	type categoryGroup struct {
		label string
		items []manifest.ContentItem
	}
	groups := []categoryGroup{
		{"Libraries", m.Libraries},
		{"Tools", m.Tools},
		{"Telemetry", m.Telemetry},
		{"Functions", m.Functions},
		{"Mixes", m.Mixes},
		{"Widgets", m.Widgets},
	}
	for _, g := range groups {
		if len(g.items) == 0 {
			continue
		}
		pterm.DefaultSection.Println(g.label)
		for _, item := range g.items {
			if !includeDev && item.Dev {
				continue
			}
			suffix := ""
			if item.Dev {
				suffix = " [dev]"
			}
			pterm.Printfln("  %s (%s)%s", item.Name, item.Path, suffix)
		}
	}
	pterm.Println()

	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt)
	defer stop()

	allItems := m.ContentItems(includeDev)

	// Initial sync.
	bar, _ := pterm.DefaultProgressbar.
		WithTitle("Initial sync").
		WithTotal(0).
		Start()

	var initialTotal int
	opts := pkgsync.Options{
		SourceRoot: sourceRoot,
		TargetDir:  targetDir,
		Items:      allItems,
		Callbacks: pkgsync.Callbacks{
			OnInitialCopyStart: func(total int) {
				initialTotal = total
				bar.Total = total
			},
			OnFileCopied: func(e pkgsync.Event) {
				bar.UpdateTitle(filepath.Base(e.RelPath))
				bar.Increment()
				logging.Debugf("    %s", e.RelPath)
			},
		},
	}

	copied, err := pkgsync.InitialSync(opts)
	bar.Stop()
	if err != nil {
		return err
	}

	pterm.Println()
	pterm.Success.Printfln("Initial sync: %d/%d file(s) copied to %s", copied, initialTotal, targetDir)
	pterm.Println()

	// Watch phase.
	var syncCount atomic.Int32

	spinner, _ := pterm.DefaultSpinner.
		WithText("Watching for changes... (Ctrl+C to stop)").
		Start()

	opts.Callbacks = pkgsync.Callbacks{
		OnSyncEvent: func(e pkgsync.Event) {
			n := syncCount.Add(1)
			spinner.UpdateText(fmt.Sprintf("[%d] %s: %s", n, e.Op, e.RelPath))
		},
		OnError: func(err error) {
			logging.WithField("error", err).Warn("sync error")
		},
	}

	err = pkgsync.Watch(ctx, opts)
	spinner.Success("Sync stopped")

	total := syncCount.Load()
	if total > 0 {
		pterm.Success.Printfln("%d file(s) synced during session", total)
	}

	return err
}
