package cmd

import (
	"fmt"
	"os"
	"path/filepath"

	"github.com/jurgelenas/edgetx-cli/internal/radio"
	"github.com/pterm/pterm"
	"github.com/spf13/cobra"
)

var (
	backupCompress   bool
	backupDirectory  string
	backupName       string
	backupEject      bool
)

var backupCmd = &cobra.Command{
	Use:   "backup",
	Short: "Back up an EdgeTX radio's SD card contents",
	Long: `Backup detects a connected EdgeTX radio and copies the entire SD card
contents to a local directory. Optionally compresses the backup into a
zip archive.

The backup is saved to the current directory by default, or to a custom
directory specified with --directory. The backup name defaults to
backup-YYYYMMDD-HHMMSS but can be overridden with --name.

Examples:
  edgetx-cli backup
  edgetx-cli backup --compress
  edgetx-cli backup --directory ~/backups --name my-radio  # creates my-radio-2026-03-05/
  edgetx-cli backup --compress --eject`,
	RunE: runBackup,
}

func init() {
	backupCmd.Flags().BoolVar(&backupCompress, "compress", false, "create a .zip archive instead of a directory")
	backupCmd.Flags().StringVar(&backupDirectory, "directory", ".", "output directory for the backup")
	backupCmd.Flags().StringVar(&backupName, "name", "", "custom backup name prefix (date is always appended)")
	backupCmd.Flags().BoolVar(&backupEject, "eject", false, "safely unmount radio after backup")
	rootCmd.AddCommand(backupCmd)
}

func runBackup(cmd *cobra.Command, args []string) error {
	outDir, err := filepath.Abs(backupDirectory)
	if err != nil {
		return fmt.Errorf("resolving output directory: %w", err)
	}
	if info, err := os.Stat(outDir); err != nil {
		return fmt.Errorf("output directory %q does not exist", outDir)
	} else if !info.IsDir() {
		return fmt.Errorf("output path %q is not a directory", outDir)
	}

	name := radio.BackupDirName(backupName)

	// Detect radio.
	radioDir, err := resolveSDRoot("")
	if err != nil {
		return err
	}

	printSDCardInfo(radioDir)

	pterm.DefaultHeader.Println("Backup")
	pterm.Println()

	// Count files for progress bar.
	totalFiles := radio.CountAllFiles(radioDir)

	destDir := filepath.Join(outDir, name)
	if err := os.MkdirAll(destDir, 0o755); err != nil {
		return fmt.Errorf("creating backup directory: %w", err)
	}

	bar, _ := pterm.DefaultProgressbar.
		WithTotal(totalFiles).
		WithTitle("Backing up files").
		Start()

	onFile := func(dest string) {
		bar.UpdateTitle(filepath.Base(dest))
		bar.Increment()
	}

	copied, err := radio.BackupDir(radioDir, destDir, radio.BackupOptions{OnFile: onFile})
	bar.Stop()
	if err != nil {
		return err
	}

	pterm.Println()

	outputPath := destDir
	if backupCompress {
		zipPath := destDir + ".zip"

		zipTotal := radio.CountAllFiles(destDir)
		zipBar, _ := pterm.DefaultProgressbar.
			WithTotal(zipTotal).
			WithTitle("Compressing").
			Start()

		err := radio.CompressDir(destDir, zipPath, func(relPath string) {
			zipBar.UpdateTitle(filepath.Base(relPath))
			zipBar.Increment()
		})
		zipBar.Stop()
		if err != nil {
			return fmt.Errorf("compressing backup: %w", err)
		}

		pterm.Println()
		outputPath = zipPath
	}

	pterm.Success.Printfln("Backed up %d files to %s", copied, outputPath)

	if backupEject {
		return radio.Eject(radioDir)
	}

	return nil
}
