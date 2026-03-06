package manifest

import (
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"strings"

	toml "github.com/pelletier/go-toml/v2"
	"golang.org/x/mod/semver"
)

var validName = regexp.MustCompile(`^[a-zA-Z0-9][a-zA-Z0-9_-]*$`)

const FileName = "edgetx.toml"

type Package struct {
	Name            string `toml:"name"`
	Description     string `toml:"description"`
	License         string `toml:"license,omitempty"`
	SourceDir       string `toml:"source_dir,omitempty"`
	Binary          bool   `toml:"binary,omitempty"`
	MinEdgeTXVersion string `toml:"min_edgetx_version,omitempty"`
}

type ContentItem struct {
	Name    string   `toml:"name"`
	Path    string   `toml:"path"`
	Depends []string `toml:"depends,omitempty"`
	Exclude []string `toml:"exclude,omitempty"`
}

type Manifest struct {
	Package   Package       `toml:"package"`
	Libraries []ContentItem `toml:"libraries"`
	Tools     []ContentItem `toml:"tools"`
	Telemetry []ContentItem `toml:"telemetry"`
	Functions []ContentItem `toml:"functions"`
	Mixes     []ContentItem `toml:"mixes"`
	Widgets   []ContentItem `toml:"widgets"`
}

// Load reads and parses edgetx.toml from the given directory.
func Load(dir string) (*Manifest, error) {
	path := filepath.Join(dir, FileName)

	data, err := os.ReadFile(path)
	if err != nil {
		return nil, fmt.Errorf("reading manifest %s: %w", path, err)
	}

	var m Manifest
	if err := toml.Unmarshal(data, &m); err != nil {
		return nil, fmt.Errorf("parsing manifest %s: %w", path, err)
	}

	if err := m.Validate(dir); err != nil {
		return nil, fmt.Errorf("invalid manifest %s: %w", path, err)
	}

	return &m, nil
}

// Validate checks that all depends references resolve to a [[libraries]] entry,
// that source_dir (if set) exists, and that all content paths exist under the
// source root.
func (m *Manifest) Validate(manifestDir string) error {
	if m.Package.Name == "" {
		return fmt.Errorf("package name is required")
	}
	if !validName.MatchString(m.Package.Name) {
		return fmt.Errorf("package name %q must contain only alphanumeric characters, dashes, and underscores", m.Package.Name)
	}

	if v := m.Package.MinEdgeTXVersion; v != "" {
		sv := v
		if !strings.HasPrefix(sv, "v") {
			sv = "v" + sv
		}
		if !semver.IsValid(sv) {
			return fmt.Errorf("min_edgetx_version %q is not a valid semver version", v)
		}
	}

	libs := make(map[string]bool, len(m.Libraries))
	for _, lib := range m.Libraries {
		libs[lib.Name] = true
	}

	var unresolved []string
	for _, items := range [][]ContentItem{m.Tools, m.Telemetry, m.Functions, m.Mixes, m.Widgets} {
		for _, item := range items {
			for _, dep := range item.Depends {
				if !libs[dep] {
					unresolved = append(unresolved, fmt.Sprintf("%s depends on %q", item.Name, dep))
				}
			}
		}
	}

	if len(unresolved) > 0 {
		return fmt.Errorf("unresolved library dependencies: %v", unresolved)
	}

	sourceRoot := m.SourceRoot(manifestDir)

	if info, err := os.Stat(sourceRoot); err != nil {
		return fmt.Errorf("source directory %q does not exist", sourceRoot)
	} else if !info.IsDir() {
		return fmt.Errorf("source directory %q is not a directory", sourceRoot)
	}

	var missing []string
	for _, item := range m.ContentItems() {
		p := filepath.Join(sourceRoot, item.Path)
		if _, err := os.Stat(p); err != nil {
			missing = append(missing, item.Path)
		}
	}
	if len(missing) > 0 {
		return fmt.Errorf("content paths not found under %s: %v", sourceRoot, missing)
	}

	return nil
}

// SourceRoot returns the absolute path to the source directory. If SourceDir
// is set in [package], it is resolved relative to manifestDir. Otherwise
// manifestDir itself is returned.
func (m *Manifest) SourceRoot(manifestDir string) string {
	if m.Package.SourceDir == "" {
		return manifestDir
	}
	return filepath.Join(manifestDir, m.Package.SourceDir)
}

// ContentItems returns every content item, libraries first so dependencies
// are copied before the items that depend on them.
func (m *Manifest) ContentItems() []ContentItem {
	var items []ContentItem
	for _, group := range [][]ContentItem{m.Libraries, m.Tools, m.Telemetry, m.Functions, m.Mixes, m.Widgets} {
		items = append(items, group...)
	}
	return items
}

// AllPaths returns every content path, libraries first so dependencies are
// copied before the items that depend on them.
func (m *Manifest) AllPaths() []string {
	var paths []string
	for _, groups := range [][]ContentItem{m.Libraries, m.Tools, m.Telemetry, m.Functions, m.Mixes, m.Widgets} {
		for _, item := range groups {
			paths = append(paths, item.Path)
		}
	}
	return paths
}
