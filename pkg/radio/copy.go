package radio

import (
	"fmt"
	"io"
	"os"
	"path/filepath"

	"github.com/edgetx/cli/pkg/logging"
)

// DefaultExclude contains glob patterns that are always excluded from copy
// operations (e.g. compiled Lua bytecode).
var DefaultExclude = []string{"*.luac"}

// CopyOptions configures a CopyPaths invocation.
type CopyOptions struct {
	DryRun  bool
	Exclude []string          // additional glob patterns matched against the filename
	OnFile  func(dest string) // called for each file before copy; nil = no-op
}

// CopyPaths copies each relative path from srcDir to destDir. Directories are
// copied recursively. Returns the number of files copied.
func CopyPaths(srcDir, destDir string, paths []string, opts CopyOptions) (int, error) {
	copied := 0

	for _, relPath := range paths {
		src := filepath.Join(srcDir, relPath)

		info, err := os.Stat(src)
		if err != nil {
			logging.WithField("path", relPath).Warn("source does not exist, skipping")
			continue
		}

		if info.IsDir() {
			n, err := copyDir(src, srcDir, destDir, opts)
			if err != nil {
				return copied, fmt.Errorf("copying directory %s: %w", relPath, err)
			}
			copied += n
		} else {
			if IsExcluded(filepath.Base(src), opts.Exclude) {
				logging.Debugf("excluded: %s", relPath)
				continue
			}
			if err := copySingleFile(src, filepath.Join(destDir, relPath), opts); err != nil {
				return copied, fmt.Errorf("copying file %s: %w", relPath, err)
			}
			if !opts.DryRun {
				copied++
			}
		}
	}

	return copied, nil
}

// CountFiles returns the total number of regular files under the given
// relative paths within srcDir, excluding files matching the given patterns.
func CountFiles(srcDir string, paths []string, exclude []string) int {
	count := 0
	for _, relPath := range paths {
		src := filepath.Join(srcDir, relPath)
		info, err := os.Stat(src)
		if err != nil {
			continue
		}
		if !info.IsDir() {
			if !IsExcluded(filepath.Base(src), exclude) {
				count++
			}
			continue
		}
		filepath.Walk(src, func(path string, fi os.FileInfo, err error) error {
			if err != nil {
				return nil
			}
			if !fi.IsDir() && !IsExcluded(fi.Name(), exclude) {
				count++
			}
			return nil
		})
	}
	return count
}

func copyDir(srcRoot, srcBase, destBase string, opts CopyOptions) (int, error) {
	copied := 0
	err := filepath.Walk(srcRoot, func(path string, info os.FileInfo, err error) error {
		if err != nil {
			return err
		}
		if info.IsDir() {
			return nil
		}

		if IsExcluded(info.Name(), opts.Exclude) {
			logging.Debugf("excluded: %s", path)
			return nil
		}

		relFile, err := filepath.Rel(srcBase, path)
		if err != nil {
			return err
		}
		destFile := filepath.Join(destBase, relFile)

		if err := copySingleFile(path, destFile, opts); err != nil {
			return err
		}
		if !opts.DryRun {
			copied++
		}
		return nil
	})
	return copied, err
}

func copySingleFile(src, dest string, opts CopyOptions) error {
	if opts.OnFile != nil {
		opts.OnFile(dest)
	}

	if opts.DryRun {
		return nil
	}

	if err := os.MkdirAll(filepath.Dir(dest), 0o755); err != nil {
		return err
	}

	in, err := os.Open(src)
	if err != nil {
		return err
	}
	defer in.Close()

	out, err := os.Create(dest)
	if err != nil {
		return err
	}
	defer out.Close()

	if _, err := io.Copy(out, in); err != nil {
		return err
	}

	return out.Close()
}

// IsExcluded checks if filename matches any DefaultExclude pattern or any of
// the caller-supplied patterns.
func IsExcluded(filename string, patterns []string) bool {
	for _, pattern := range DefaultExclude {
		if matched, _ := filepath.Match(pattern, filename); matched {
			return true
		}
	}
	for _, pattern := range patterns {
		if matched, _ := filepath.Match(pattern, filename); matched {
			return true
		}
	}
	return false
}
