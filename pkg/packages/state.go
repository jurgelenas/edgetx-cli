package packages

import (
	"encoding/csv"
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"gopkg.in/yaml.v3"
)

const StateFileName = "RADIO/packages.yml"
const fileListDir = "RADIO/packages"

// InstalledPackage describes a single package installed on the SD card.
type InstalledPackage struct {
	Source  string   `yaml:"source"`  // canonical ID: "Org/Repo", "host/org/repo", or "local::/abs/path"
	Name    string   `yaml:"name"`    // display name from remote edgetx.yml package name
	Channel string   `yaml:"channel"` // "tag", "branch", "commit", or "local"
	Version string   `yaml:"version,omitempty"` // tag name or branch name (empty for commit/local)
	Commit  string   `yaml:"commit,omitempty"`  // full SHA (empty for local)
	Paths   []string `yaml:"paths"`   // relative paths on SD card
}

// State holds the list of installed packages on an SD card.
type State struct {
	Packages []InstalledPackage `yaml:"packages"`
}

// LoadState reads the state file from the SD card root. If the file does not
// exist, an empty state is returned.
func LoadState(sdRoot string) (*State, error) {
	path := filepath.Join(sdRoot, StateFileName)

	data, err := os.ReadFile(path)
	if os.IsNotExist(err) {
		return &State{}, nil
	}
	if err != nil {
		return nil, fmt.Errorf("reading state file: %w", err)
	}

	var s State
	if err := yaml.Unmarshal(data, &s); err != nil {
		return nil, fmt.Errorf("parsing state file %s: %w", path, err)
	}

	return &s, nil
}

// Save writes the state file to the SD card root.
func (s *State) Save(sdRoot string) error {
	path := filepath.Join(sdRoot, StateFileName)

	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		return fmt.Errorf("creating state directory: %w", err)
	}

	data, err := yaml.Marshal(s)
	if err != nil {
		return fmt.Errorf("marshaling state: %w", err)
	}

	return os.WriteFile(path, data, 0o644)
}

// FindBySource returns the installed package with the given canonical source,
// or nil if not found.
func (s *State) FindBySource(canonical string) *InstalledPackage {
	for i := range s.Packages {
		if s.Packages[i].Source == canonical {
			return &s.Packages[i]
		}
	}
	return nil
}

// FindByName returns all installed packages whose Name matches the query.
func (s *State) FindByName(name string) []*InstalledPackage {
	var matches []*InstalledPackage
	for i := range s.Packages {
		if s.Packages[i].Name == name {
			matches = append(matches, &s.Packages[i])
		}
	}
	return matches
}

// Find looks up a package by source first, then by name. Returns an error if
// the query matches multiple packages by name (ambiguous) or matches nothing.
func (s *State) Find(query string) (*InstalledPackage, error) {
	// Try source first.
	if pkg := s.FindBySource(query); pkg != nil {
		return pkg, nil
	}

	// Try name.
	matches := s.FindByName(query)
	switch len(matches) {
	case 0:
		return nil, fmt.Errorf("package %q not found", query)
	case 1:
		return matches[0], nil
	default:
		sources := make([]string, len(matches))
		for i, m := range matches {
			sources[i] = m.Source
		}
		return nil, fmt.Errorf("ambiguous package name %q matches multiple sources: %v", query, sources)
	}
}

// Remove deletes the package with the given canonical source from the state.
func (s *State) Remove(canonical string) {
	filtered := s.Packages[:0]
	for _, pkg := range s.Packages {
		if pkg.Source != canonical {
			filtered = append(filtered, pkg)
		}
	}
	s.Packages = filtered
}

// Add adds or replaces a package in the state. If a package with the same
// source already exists, it is replaced.
func (s *State) Add(pkg InstalledPackage) {
	for i := range s.Packages {
		if s.Packages[i].Source == pkg.Source {
			s.Packages[i] = pkg
			return
		}
	}
	s.Packages = append(s.Packages, pkg)
}

// AllInstalledPaths returns a map of every installed path to its owning source.
func (s *State) AllInstalledPaths() map[string]string {
	m := make(map[string]string)
	for _, pkg := range s.Packages {
		for _, p := range pkg.Paths {
			m[p] = pkg.Source
		}
	}
	return m
}

// fileListPath returns the path to the .list file for a given package name.
func fileListPath(sdRoot, name string) string {
	return filepath.Join(sdRoot, fileListDir, name+".list")
}

// SaveFileList writes the list of installed files for a package as CSV.
// Each row contains a single relative path from the SD root.
func SaveFileList(sdRoot, name string, files []string) error {
	path := fileListPath(sdRoot, name)
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		return fmt.Errorf("creating file list directory: %w", err)
	}

	f, err := os.Create(path)
	if err != nil {
		return fmt.Errorf("creating file list: %w", err)
	}
	defer f.Close()

	w := csv.NewWriter(f)
	for _, file := range files {
		if err := w.Write([]string{file}); err != nil {
			return fmt.Errorf("writing file list: %w", err)
		}
	}
	w.Flush()
	return w.Error()
}

// LoadFileList reads the list of installed files for a package from CSV.
// Returns nil (not an error) if the file does not exist.
func LoadFileList(sdRoot, name string) ([]string, error) {
	path := fileListPath(sdRoot, name)
	f, err := os.Open(path)
	if os.IsNotExist(err) {
		return nil, nil
	}
	if err != nil {
		return nil, fmt.Errorf("reading file list: %w", err)
	}
	defer f.Close()

	records, err := csv.NewReader(f).ReadAll()
	if err != nil {
		return nil, fmt.Errorf("parsing file list: %w", err)
	}

	files := make([]string, 0, len(records))
	for _, record := range records {
		if len(record) > 0 && record[0] != "" {
			files = append(files, record[0])
		}
	}
	return files, nil
}

// RemoveFileList deletes the .list file for a package.
func RemoveFileList(sdRoot, name string) {
	os.Remove(fileListPath(sdRoot, name))
}

// splitQueryVersion splits a query like "ExpressLRS/Lua-Scripts@v1.6.0" into
// ("ExpressLRS/Lua-Scripts", "v1.6.0"). If there is no "@", version is empty.
func splitQueryVersion(query string) (string, string) {
	// Local paths (starting with . / or ~) should not be split on @.
	if len(query) > 0 && (query[0] == '.' || query[0] == '/' || query[0] == '~') {
		return query, ""
	}
	if i := strings.LastIndex(query, "@"); i > 0 {
		return query[:i], query[i+1:]
	}
	return query, ""
}
