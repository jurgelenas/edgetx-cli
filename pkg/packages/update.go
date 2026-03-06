package packages

import (
	"fmt"
	"path/filepath"

	"github.com/jurgelenas/edgetx-cli/pkg/manifest"
	"github.com/jurgelenas/edgetx-cli/pkg/radio"
	"github.com/jurgelenas/edgetx-cli/pkg/repository"
)

// UpdateOptions configures an update operation.
type UpdateOptions struct {
	SDRoot     string
	Query      string // package ref, source, or name (ignored if All is true)
	All        bool
	DryRun     bool
	OnFile     func(dest string)
	BeforeCopy func(name string, totalFiles int) // called before copying each package
}

// UpdateResult holds the outcome of updating a single package.
type UpdateResult struct {
	Package     InstalledPackage
	FilesCopied int
	UpToDate    bool
}

// Update updates one or all installed packages.
func Update(opts UpdateOptions) ([]UpdateResult, error) {
	if opts.Query == "" && !opts.All {
		return nil, fmt.Errorf("specify a package name or use --all")
	}

	state, err := LoadState(opts.SDRoot)
	if err != nil {
		return nil, err
	}

	var targets []InstalledPackage
	var originalSources []string // tracks the state-file source for each target
	var versionOverride string

	if opts.All {
		targets = make([]InstalledPackage, len(state.Packages))
		copy(targets, state.Packages)
		originalSources = make([]string, len(targets))
		for i, t := range targets {
			originalSources[i] = t.Source
		}
	} else {
		query, version := splitQueryVersion(opts.Query)
		versionOverride = version
		pkg, err := state.Find(query)
		if err != nil {
			// Query didn't match by source or name. Try parsing it as a
			// remote package ref — resolve it to discover the manifest
			// name, then find the installed package by that name. This
			// handles the case where a package was installed locally but
			// the user wants to update from a remote source.
			ref, refErr := repository.ParsePackageRef(query)
			if refErr == nil && ref.IsLocal {
				m, mErr := manifest.Load(ref.LocalPath)
				if mErr == nil {
					if matches := state.FindByName(m.Package.Name); len(matches) == 1 {
						pkg = matches[0]
						err = nil
					}
				}
			} else if refErr == nil && !ref.IsLocal {
				if version != "" {
					ref.Version = version
				}
				result, cloneErr := repository.CloneAndCheckout(ref)
				if cloneErr == nil {
					if matches := state.FindByName(result.Manifest.Package.Name); len(matches) == 1 {
						// Found the installed package by manifest name.
						// Switch its source to the remote ref so updateSingle
						// uses the new source (and reuses the already-cloned
						// result via cache).
						target := *matches[0]
						originalSources = []string{target.Source}
						target.Source = ref.Canonical()
						target.Channel = result.Resolved.Channel
						pkg = &target
						err = nil
					}
				}
			}
			if err != nil {
				return nil, err
			}
		}
		if len(originalSources) == 0 {
			originalSources = []string{pkg.Source}
		}
		targets = []InstalledPackage{*pkg}
	}

	var results []UpdateResult
	for i, pkg := range targets {
		result, err := updateSingle(opts.SDRoot, pkg, originalSources[i], state, versionOverride, opts.DryRun, opts.OnFile, opts.BeforeCopy)
		if err != nil {
			return results, fmt.Errorf("updating %s: %w", pkg.Source, err)
		}
		results = append(results, *result)
	}

	return results, nil
}

func updateSingle(sdRoot string, pkg InstalledPackage, originalSource string, state *State, versionOverride string, dryRun bool, onFile func(string), beforeCopy func(string, int)) (*UpdateResult, error) {
	if pkg.Channel == "commit" && versionOverride == "" {
		return &UpdateResult{Package: pkg, UpToDate: true}, nil
	}

	var m *manifest.Manifest
	var sourceDir string
	var newChannel, newVersion, newCommit string

	if pkg.Channel == "local" {
		// Re-copy from local path.
		localPath := pkg.Source[len("local::"):]
		var err error
		m, err = manifest.Load(localPath)
		if err != nil {
			return nil, err
		}
		sourceDir = m.SourceRoot(localPath)
		newChannel = "local"
	} else {
		// Parse source back to ref.
		ref, err := repository.ParsePackageRef(pkg.Source)
		if err != nil {
			return nil, fmt.Errorf("parsing source %q: %w", pkg.Source, err)
		}

		if versionOverride != "" {
			// Explicit version requested.
			ref.Version = versionOverride
		} else if pkg.Channel == "branch" {
			// Stay on same branch.
			ref.Version = pkg.Version
		}
		// tag channel with no override: leave version empty to get latest.

		result, err := repository.CloneAndCheckout(ref)
		if err != nil {
			return nil, err
		}

		// Check if already up to date.
		if result.Resolved.Hash.String() == pkg.Commit {
			return &UpdateResult{Package: pkg, UpToDate: true}, nil
		}

		m = result.Manifest
		sourceDir = m.SourceRoot(result.Dir)
		newChannel = result.Resolved.Channel
		newVersion = result.Resolved.Version
		newCommit = result.Resolved.Hash.String()
	}

	newPaths := m.AllPaths()

	// Check conflicts. Skip both the current source and the original source
	// (they differ when switching e.g. from local to remote).
	if err := CheckConflicts(state, newPaths, originalSource); err != nil {
		return nil, err
	}

	totalCopied := 0
	if !dryRun {
		// Remove old files using tracked file list.
		removeTrackedFiles(sdRoot, pkg.Name)
		// Remove old state entry (by original source, in case it changed).
		state.Remove(originalSource)

		if beforeCopy != nil {
			beforeCopy(m.Package.Name, CountInstallFiles(sourceDir, m))
		}

		// Copy new files and track them.
		var copiedFiles []string
		for _, item := range m.ContentItems() {
			exclude := buildExclude(m.Package.Binary, item)
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
			n, err := radio.CopyPaths(sourceDir, sdRoot, []string{item.Path}, copyOpts)
			if err != nil {
				return nil, fmt.Errorf("copying %s: %w", item.Path, err)
			}
			totalCopied += n
		}

		// Update state with new source.
		updated := InstalledPackage{
			Source:  pkg.Source,
			Name:    m.Package.Name,
			Channel: newChannel,
			Version: newVersion,
			Commit:  newCommit,
			Paths:   newPaths,
		}
		state.Add(updated)
		if err := state.Save(sdRoot); err != nil {
			return nil, fmt.Errorf("saving state: %w", err)
		}
		if err := SaveFileList(sdRoot, updated.Name, copiedFiles); err != nil {
			return nil, fmt.Errorf("saving file list: %w", err)
		}

		return &UpdateResult{Package: updated, FilesCopied: totalCopied}, nil
	}

	return &UpdateResult{
		Package: InstalledPackage{
			Source:  pkg.Source,
			Name:    m.Package.Name,
			Channel: newChannel,
			Version: newVersion,
			Commit:  newCommit,
			Paths:   newPaths,
		},
	}, nil
}
