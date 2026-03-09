package manifest

import (
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"strings"

	"golang.org/x/mod/semver"
	"gopkg.in/yaml.v3"
)

var validName = regexp.MustCompile(`^[a-zA-Z0-9][a-zA-Z0-9_-]*$`)

const FileName = "edgetx.yml"

type Package struct {
	Name            string `yaml:"name"`
	Description     string `yaml:"description"`
	License         string `yaml:"license,omitempty"`
	SourceDir       string `yaml:"source_dir,omitempty"`
	Binary          bool   `yaml:"binary,omitempty"`
	MinEdgeTXVersion string `yaml:"min_edgetx_version,omitempty"`
}

type ContentItem struct {
	Name    string   `yaml:"name"`
	Path    string   `yaml:"path"`
	Depends []string `yaml:"depends,omitempty"`
	Exclude []string `yaml:"exclude,omitempty"`
	Dev     bool     `yaml:"dev,omitempty"`
}

type Manifest struct {
	Package   Package       `yaml:"package"`
	Libraries []ContentItem `yaml:"libraries"`
	Tools     []ContentItem `yaml:"tools"`
	Telemetry []ContentItem `yaml:"telemetry"`
	Functions []ContentItem `yaml:"functions"`
	Mixes     []ContentItem `yaml:"mixes"`
	Widgets   []ContentItem `yaml:"widgets"`
	Sounds    []ContentItem `yaml:"sounds"`
}

// Load reads and parses edgetx.yml from the given directory.
func Load(dir string) (*Manifest, error) {
	path := filepath.Join(dir, FileName)

	data, err := os.ReadFile(path)
	if err != nil {
		return nil, fmt.Errorf("reading manifest %s: %w", path, err)
	}

	var m Manifest
	if err := yaml.Unmarshal(data, &m); err != nil {
		return nil, fmt.Errorf("parsing manifest %s: %w", path, err)
	}

	if err := m.Validate(dir); err != nil {
		return nil, fmt.Errorf("invalid manifest %s: %w", path, err)
	}

	return &m, nil
}

// Validate checks that all depends references resolve to a libraries entry,
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
	devLibs := make(map[string]bool)
	for _, lib := range m.Libraries {
		libs[lib.Name] = true
		if lib.Dev {
			devLibs[lib.Name] = true
		}
	}

	var unresolved []string
	var devErrors []string
	for _, items := range [][]ContentItem{m.Tools, m.Telemetry, m.Functions, m.Mixes, m.Widgets, m.Sounds} {
		for _, item := range items {
			for _, dep := range item.Depends {
				if !libs[dep] {
					unresolved = append(unresolved, fmt.Sprintf("%s depends on %q", item.Name, dep))
				} else if !item.Dev && devLibs[dep] {
					devErrors = append(devErrors, fmt.Sprintf("%s depends on dev library %q", item.Name, dep))
				}
			}
		}
	}

	if len(unresolved) > 0 {
		return fmt.Errorf("unresolved library dependencies: %v", unresolved)
	}

	if len(devErrors) > 0 {
		return fmt.Errorf("non-dev items depend on dev libraries: %v", devErrors)
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
// is set in package, it is resolved relative to manifestDir. Otherwise
// manifestDir itself is returned.
func (m *Manifest) SourceRoot(manifestDir string) string {
	if m.Package.SourceDir == "" {
		return manifestDir
	}
	return filepath.Join(manifestDir, m.Package.SourceDir)
}

// ContentItems returns content items, libraries first so dependencies are
// copied before the items that depend on them. When includeDev is false,
// items marked as dev are excluded.
func (m *Manifest) ContentItems(includeDev ...bool) []ContentItem {
	dev := len(includeDev) > 0 && includeDev[0]
	var items []ContentItem
	for _, group := range [][]ContentItem{m.Libraries, m.Tools, m.Telemetry, m.Functions, m.Mixes, m.Widgets, m.Sounds} {
		for _, item := range group {
			if !dev && item.Dev {
				continue
			}
			items = append(items, item)
		}
	}
	return items
}

// AllPaths returns content paths, libraries first so dependencies are
// copied before the items that depend on them. When includeDev is false,
// items marked as dev are excluded.
func (m *Manifest) AllPaths(includeDev ...bool) []string {
	var paths []string
	for _, item := range m.ContentItems(includeDev...) {
		paths = append(paths, item.Path)
	}
	return paths
}
