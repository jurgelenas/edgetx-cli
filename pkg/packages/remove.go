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
	Package      InstalledPackage
	FilesRemoved int
}

// PreparedRemove holds the state needed to execute a package removal.
type PreparedRemove struct {
	Package InstalledPackage
	Files   []string // tracked files to remove
	state   *State
	sdRoot  string
}

// TotalFiles returns the number of files that will be removed.
func (p *PreparedRemove) TotalFiles() int {
	return len(p.Files)
}

// PrepareRemove resolves the package and file list without deleting anything.
func PrepareRemove(opts RemoveOptions) (*PreparedRemove, error) {
	state, err := LoadState(opts.SDRoot)
	if err != nil {
		return nil, err
	}

	query, _ := splitQueryVersion(opts.Query)
	pkg, err := state.Find(query)
	if err != nil {
		return nil, err
	}

	files, _ := LoadFileList(opts.SDRoot, pkg.Name)

	return &PreparedRemove{
		Package: *pkg,
		Files:   files,
		state:   state,
		sdRoot:  opts.SDRoot,
	}, nil
}

// Execute performs the removal. If dryRun is true, no files are deleted.
// The optional onFile callback is called for each file removed.
func (p *PreparedRemove) Execute(dryRun bool, onFile func(string)) (*RemoveResult, error) {
	result := &RemoveResult{Package: p.Package}

	if dryRun {
		return result, nil
	}

	for _, f := range p.Files {
		full := filepath.Join(p.sdRoot, f)
		os.Remove(full)
		if onFile != nil {
			onFile(f)
		}
	}
	result.FilesRemoved = len(p.Files)

	for _, f := range p.Files {
		cleanEmptyParents(p.sdRoot, f)
	}
	RemoveFileList(p.sdRoot, p.Package.Name)

	p.state.Remove(p.Package.Source)
	if err := p.state.Save(p.sdRoot); err != nil {
		return nil, fmt.Errorf("saving state: %w", err)
	}

	return result, nil
}

// Remove removes an installed package from the SD card.
func Remove(opts RemoveOptions) (*RemoveResult, error) {
	prepared, err := PrepareRemove(opts)
	if err != nil {
		return nil, err
	}
	return prepared.Execute(opts.DryRun, nil)
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
