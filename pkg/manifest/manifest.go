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

// StringOrSlice is a YAML type that accepts both a single string and a list
// of strings. This allows source_dir to be written as either:
//
//	source_dir: "src"
//	source_dir: [a, b]
type StringOrSlice []string

func (s *StringOrSlice) UnmarshalYAML(value *yaml.Node) error {
	if value.Kind == yaml.ScalarNode {
		*s = []string{value.Value}
		return nil
	}
	var slice []string
	if err := value.Decode(&slice); err != nil {
		return err
	}
	*s = slice
	return nil
}

type Package struct {
	Name             string        `yaml:"name"`
	Description      string        `yaml:"description"`
	License          string        `yaml:"license,omitempty"`
	SourceDirs       StringOrSlice `yaml:"source_dir,omitempty"`
	Binary           bool          `yaml:"binary,omitempty"`
	MinEdgeTXVersion string        `yaml:"min_edgetx_version,omitempty"`
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
	Images    []ContentItem `yaml:"images"`
	Files     []ContentItem `yaml:"files"`
}

// Load reads and parses edgetx.yml from the given directory.
func Load(dir string) (*Manifest, error) {
	return LoadFile(filepath.Join(dir, FileName))
}

// LoadFile reads and parses a manifest from the given file path.
func LoadFile(path string) (*Manifest, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, fmt.Errorf("reading manifest %s: %w", path, err)
	}

	var m Manifest
	if err := yaml.Unmarshal(data, &m); err != nil {
		return nil, fmt.Errorf("parsing manifest %s: %w", path, err)
	}

	manifestDir := filepath.Dir(path)
	if err := m.Validate(manifestDir); err != nil {
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
	for _, items := range [][]ContentItem{m.Tools, m.Telemetry, m.Functions, m.Mixes, m.Widgets, m.Sounds, m.Images, m.Files} {
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

	sourceRoots := m.SourceRoots(manifestDir)

	for _, root := range sourceRoots {
		if info, err := os.Stat(root); err != nil {
			return fmt.Errorf("source directory %q does not exist", root)
		} else if !info.IsDir() {
			return fmt.Errorf("source directory %q is not a directory", root)
		}
	}

	var missing []string
	for _, item := range m.ContentItems() {
		if _, err := m.ResolveContentPath(manifestDir, item.Path); err != nil {
			missing = append(missing, item.Path)
		}
	}
	if len(missing) > 0 {
		return fmt.Errorf("content paths not found: %v", missing)
	}

	return nil
}

// SourceRoots returns the absolute paths to all source directories. If
// SourceDirs is empty, returns []string{manifestDir}. Otherwise each entry
// is resolved relative to manifestDir.
func (m *Manifest) SourceRoots(manifestDir string) []string {
	if len(m.Package.SourceDirs) == 0 {
		return []string{manifestDir}
	}
	roots := make([]string, len(m.Package.SourceDirs))
	for i, d := range m.Package.SourceDirs {
		roots[i] = filepath.Join(manifestDir, d)
	}
	return roots
}

// ResolveContentPath returns the source root directory where contentPath
// exists. It iterates SourceRoots in order and returns the first match.
func (m *Manifest) ResolveContentPath(manifestDir, contentPath string) (string, error) {
	for _, root := range m.SourceRoots(manifestDir) {
		p := filepath.Join(root, contentPath)
		if _, err := os.Stat(p); err == nil {
			return root, nil
		}
	}
	return "", fmt.Errorf("content path %q not found in any source root", contentPath)
}

// ContentItems returns content items, libraries first so dependencies are
// copied before the items that depend on them. When includeDev is false,
// items marked as dev are excluded.
func (m *Manifest) ContentItems(includeDev ...bool) []ContentItem {
	dev := len(includeDev) > 0 && includeDev[0]
	var items []ContentItem
	for _, group := range [][]ContentItem{m.Libraries, m.Tools, m.Telemetry, m.Functions, m.Mixes, m.Widgets, m.Sounds, m.Images, m.Files} {
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

// NewTemplate returns the content for a new edgetx.yml with the given package name.
func NewTemplate(name string) []byte {
	return fmt.Appendf(nil, "package:\n  name: %s\n  description: \"\"\n  license: \"\"\n", name)
}

// Init creates a new edgetx.yml in the given directory. It returns an error if
// the file already exists.
func Init(dir, name string) error {
	ymlPath := filepath.Join(dir, FileName)
	if _, err := os.Stat(ymlPath); err == nil {
		return fmt.Errorf("%s already exists in %s", FileName, dir)
	}
	if err := os.WriteFile(ymlPath, NewTemplate(name), 0o644); err != nil {
		return fmt.Errorf("writing manifest: %w", err)
	}
	return nil
}
