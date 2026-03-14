package radio

import (
	"archive/zip"
	"fmt"
	"io"
	"os"
	"path/filepath"
)

// BackupOptions configures a BackupDir invocation.
type BackupOptions struct {
	OnFile func(dest string) // called for each file copied; nil = no-op
}

// BackupDir recursively copies all files from srcDir to destDir, preserving
// directory structure. Returns the total number of files copied.
func BackupDir(srcDir, destDir string, opts BackupOptions) (int, error) {
	copied := 0
	err := filepath.Walk(srcDir, func(path string, info os.FileInfo, err error) error {
		if err != nil {
			return err
		}
		if info.IsDir() {
			return nil
		}

		relPath, err := filepath.Rel(srcDir, path)
		if err != nil {
			return err
		}
		destPath := filepath.Join(destDir, relPath)

		if err := copySingleFile(path, destPath, CopyOptions{OnFile: opts.OnFile}); err != nil {
			return fmt.Errorf("copying %s: %w", relPath, err)
		}
		copied++
		return nil
	})
	return copied, err
}

// CountAllFiles returns the total number of regular files under dir.
func CountAllFiles(dir string) int {
	count := 0
	filepath.Walk(dir, func(_ string, info os.FileInfo, err error) error {
		if err != nil {
			return nil
		}
		if !info.IsDir() {
			count++
		}
		return nil
	})
	return count
}

// CompressDir creates a zip archive at zipPath from the contents of srcDir,
// preserving relative paths. The onFile callback is called for each file added.
// On success, srcDir is removed.
func CompressDir(srcDir, zipPath string, onFile func(relPath string)) error {
	outFile, err := os.Create(zipPath)
	if err != nil {
		return fmt.Errorf("creating zip file: %w", err)
	}
	defer outFile.Close()

	w := zip.NewWriter(outFile)
	defer w.Close()

	err = filepath.Walk(srcDir, func(path string, info os.FileInfo, err error) error {
		if err != nil {
			return err
		}
		if info.IsDir() {
			return nil
		}

		relPath, err := filepath.Rel(srcDir, path)
		if err != nil {
			return err
		}

		if onFile != nil {
			onFile(relPath)
		}

		header, err := zip.FileInfoHeader(info)
		if err != nil {
			return err
		}
		header.Name = filepath.ToSlash(relPath)
		header.Method = zip.Deflate

		writer, err := w.CreateHeader(header)
		if err != nil {
			return err
		}

		f, err := os.Open(path)
		if err != nil {
			return err
		}
		defer f.Close()

		_, err = io.Copy(writer, f)
		return err
	})
	if err != nil {
		return err
	}

	if err := w.Close(); err != nil {
		return err
	}
	if err := outFile.Close(); err != nil {
		return err
	}

	return os.RemoveAll(srcDir)
}
