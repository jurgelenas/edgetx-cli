package packages

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"
)

// RemoveOptions configures a remove operation.
type RemoveOptions struct {
	SDRoot string
	Query  string // package source or name
	DryRun bool
}

// RemoveResult holds the outcome of a remove operation.
type RemoveResult struct {
	Package InstalledPackage
}

// Remove removes an installed package from the SD card.
func Remove(opts RemoveOptions) (*RemoveResult, error) {
	state, err := LoadState(opts.SDRoot)
	if err != nil {
		return nil, err
	}

	query, _ := splitQueryVersion(opts.Query)
	pkg, err := state.Find(query)
	if err != nil {
		return nil, err
	}

	result := &RemoveResult{Package: *pkg}

	if !opts.DryRun {
		removeTrackedFiles(opts.SDRoot, pkg.Name)

		state.Remove(pkg.Source)
		if err := state.Save(opts.SDRoot); err != nil {
			return nil, fmt.Errorf("saving state: %w", err)
		}
	}

	return result, nil
}

// removeTrackedFiles removes files installed by a package. It reads the .list
// file for precise file-level removal. If no .list exists (e.g. packages
// installed before file tracking was added), it falls back to removing the
// entire directory paths.
func removeTrackedFiles(sdRoot, name string) {
	files, _ := LoadFileList(sdRoot, name)
	for _, f := range files {
		os.Remove(filepath.Join(sdRoot, f))
	}
	for _, f := range files {
		cleanEmptyParents(sdRoot, f)
	}
	RemoveFileList(sdRoot, name)
}

// cleanEmptyParents removes empty parent directories up to the SD root.
func cleanEmptyParents(sdRoot, relPath string) {
	parts := strings.Split(relPath, "/")
	// Walk up from parent of the path.
	for i := len(parts) - 1; i > 0; i-- {
		parent := filepath.Join(sdRoot, filepath.Join(parts[:i]...))
		entries, err := os.ReadDir(parent)
		if err != nil || len(entries) > 0 {
			break
		}
		os.Remove(parent)
	}
}
