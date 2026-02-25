package manifest

import (
	"fmt"
	"os"
	"path/filepath"

	toml "github.com/pelletier/go-toml/v2"
)

const FileName = "edgetx.toml"

type Package struct {
	Name        string `toml:"name"`
	Version     string `toml:"version"`
	Description string `toml:"description"`
}

type ContentItem struct {
	Name    string   `toml:"name"`
	Path    string   `toml:"path"`
	Depends []string `toml:"depends,omitempty"`
	Exclude []string `toml:"exclude,omitempty"`
}

type Manifest struct {
	Package   Package       `toml:"package"`
	Scripts   []ContentItem `toml:"scripts"`
	Tools     []ContentItem `toml:"tools"`
	Widgets   []ContentItem `toml:"widgets"`
	Libraries []ContentItem `toml:"libraries"`
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

	if err := m.Validate(); err != nil {
		return nil, fmt.Errorf("invalid manifest %s: %w", path, err)
	}

	return &m, nil
}

// Validate checks that all depends references resolve to a [[libraries]] entry.
func (m *Manifest) Validate() error {
	libs := make(map[string]bool, len(m.Libraries))
	for _, lib := range m.Libraries {
		libs[lib.Name] = true
	}

	var unresolved []string
	for _, items := range [][]ContentItem{m.Scripts, m.Tools, m.Widgets} {
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
	return nil
}

// AllPaths returns every content path, libraries first so dependencies are
// copied before the items that depend on them.
func (m *Manifest) AllPaths() []string {
	var paths []string
	for _, groups := range [][]ContentItem{m.Libraries, m.Scripts, m.Tools, m.Widgets} {
		for _, item := range groups {
			paths = append(paths, item.Path)
		}
	}
	return paths
}
