package packages

import (
	"fmt"
	"path/filepath"

	"github.com/jurgelenas/edgetx-cli/pkg/logging"
	"github.com/jurgelenas/edgetx-cli/pkg/manifest"
	"github.com/jurgelenas/edgetx-cli/pkg/radio"
	"github.com/jurgelenas/edgetx-cli/pkg/repository"
)

// InstallOptions configures an install operation.
type InstallOptions struct {
	SDRoot string
	Ref    repository.PackageRef
	DryRun bool
	Dev    bool               // include dev dependencies
	OnFile func(dest string) // called for each file before copy
}

// InstallResult holds the outcome of an install operation.
type InstallResult struct {
	Package     InstalledPackage
	FilesCopied int
}

// PreparedInstall holds the resolved manifest and metadata, ready for execution.
type PreparedInstall struct {
	Manifest   *manifest.Manifest
	SourceDir  string
	Package    InstalledPackage
	includeDev bool
	state      *State
}

// TotalFiles returns the number of files that will be copied.
func (p *PreparedInstall) TotalFiles() int {
	return CountInstallFiles(p.SourceDir, p.Manifest, p.includeDev)
}

// PrepareInstall resolves the package ref, loads the manifest, and checks for
// conflicts — but does not copy any files. Call Execute() on the result to
// perform the actual install.
func PrepareInstall(opts InstallOptions) (*PreparedInstall, error) {
	state, err := LoadState(opts.SDRoot)
	if err != nil {
		return nil, err
	}

	canonical := opts.Ref.Canonical()

	var m *manifest.Manifest
	var sourceDir string
	var channel, version, commit string

	if opts.Ref.IsLocal {
		m, err = manifest.Load(opts.Ref.LocalPath)
		if err != nil {
			return nil, err
		}
		sourceDir = m.SourceRoot(opts.Ref.LocalPath)
		channel = "local"
	} else {
		result, err := repository.CloneAndCheckout(opts.Ref)
		if err != nil {
			return nil, err
		}
		m = result.Manifest
		sourceDir = m.SourceRoot(result.Dir)
		channel = result.Resolved.Channel
		version = result.Resolved.Version
		commit = result.Resolved.Hash.String()
	}

	if m.Package.MinEdgeTXVersion != "" {
		radioInfo, err := radio.LoadRadioInfo(opts.SDRoot)
		if err != nil {
			return nil, fmt.Errorf("checking radio version: %w", err)
		}
		if radioInfo != nil && radioInfo.Semver != "" {
			if err := radio.CheckVersionCompatibility(radioInfo.Semver, m.Package.MinEdgeTXVersion); err != nil {
				return nil, err
			}
		} else {
			logging.Warnf("could not determine radio firmware version, skipping version check")
		}
	}

	// If the same package is already installed (by source or manifest name),
	// remove the old files so the new version can replace it cleanly.
	existing := state.FindBySource(canonical)
	if existing == nil {
		if matches := state.FindByName(m.Package.Name); len(matches) == 1 {
			existing = matches[0]
		}
	}
	if existing != nil {
		removeTrackedFiles(opts.SDRoot, existing.Name)
		state.Remove(existing.Source)
	}

	paths := m.AllPaths(opts.Dev)

	if err := CheckConflicts(state, paths, ""); err != nil {
		return nil, err
	}

	return &PreparedInstall{
		Manifest:   m,
		SourceDir:  sourceDir,
		includeDev: opts.Dev,
		Package: InstalledPackage{
			Source:  canonical,
			Name:    m.Package.Name,
			Channel: channel,
			Version: version,
			Commit:  commit,
			Paths:   paths,
			Dev:     opts.Dev,
		},
		state: state,
	}, nil
}

// Execute copies the files and updates the state. Returns the install result.
func (p *PreparedInstall) Execute(sdRoot string, dryRun bool, onFile func(string)) (*InstallResult, error) {
	totalCopied := 0
	var copiedFiles []string

	if !dryRun {
		for _, item := range p.Manifest.ContentItems(p.includeDev) {
			exclude := buildExclude(p.Manifest.Package.Binary, item)
			copyOpts := radio.CopyOptions{
				Exclude: exclude,
				OnFile: func(dest string) {
					rel, _ := filepath.Rel(sdRoot, dest)
					copiedFiles = append(copiedFiles, rel)
					if onFile != nil {
						onFile(dest)
					}
				},
			}
			n, err := radio.CopyPaths(p.SourceDir, sdRoot, []string{item.Path}, copyOpts)
			if err != nil {
				return nil, fmt.Errorf("copying %s: %w", item.Path, err)
			}
			totalCopied += n
		}

		p.state.Add(p.Package)
		if err := p.state.Save(sdRoot); err != nil {
			return nil, fmt.Errorf("saving state: %w", err)
		}
		if err := SaveFileList(sdRoot, p.Package.Name, copiedFiles); err != nil {
			return nil, fmt.Errorf("saving file list: %w", err)
		}
	}

	return &InstallResult{
		Package:     p.Package,
		FilesCopied: totalCopied,
	}, nil
}

// Install is a convenience wrapper that prepares and executes in one call.
func Install(opts InstallOptions) (*InstallResult, error) {
	prepared, err := PrepareInstall(opts)
	if err != nil {
		return nil, err
	}
	return prepared.Execute(opts.SDRoot, opts.DryRun, opts.OnFile)
}

// buildExclude returns the exclude patterns for a content item, merging
// DefaultExclude for source packages.
func buildExclude(binary bool, item manifest.ContentItem) []string {
	if binary {
		return item.Exclude
	}
	return append(radio.DefaultExclude, item.Exclude...)
}

// CountInstallFiles returns the total number of files that would be copied.
func CountInstallFiles(sourceDir string, m *manifest.Manifest, includeDev ...bool) int {
	total := 0
	for _, item := range m.ContentItems(includeDev...) {
		exclude := buildExclude(m.Package.Binary, item)
		total += radio.CountFiles(sourceDir, []string{item.Path}, exclude)
	}
	return total
}
