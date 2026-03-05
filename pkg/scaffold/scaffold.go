package scaffold

import (
	"embed"
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strings"
	"text/template"

	"github.com/edgetx/cli/pkg/manifest"
)

//go:embed templates/*.lua.tmpl
var templates embed.FS

var namePattern = regexp.MustCompile(`^[A-Za-z][A-Za-z0-9_]*$`)

type ScriptType struct {
	TOMLKey   string
	DirPrefix string
	Template  string
	MaxNameLen int
	DirBased  bool
}

var Types = map[string]ScriptType{
	"tool": {
		TOMLKey:   "tools",
		DirPrefix: "SCRIPTS/TOOLS",
		Template:  "tool.lua.tmpl",
		DirBased:  true,
	},
	"telemetry": {
		TOMLKey:    "telemetry",
		DirPrefix:  "SCRIPTS/TELEMETRY",
		Template:   "telemetry.lua.tmpl",
		MaxNameLen: 6,
	},
	"function": {
		TOMLKey:    "functions",
		DirPrefix:  "SCRIPTS/FUNCTIONS",
		Template:   "function.lua.tmpl",
		MaxNameLen: 6,
	},
	"mix": {
		TOMLKey:    "mixes",
		DirPrefix:  "SCRIPTS/MIXES",
		Template:   "mix.lua.tmpl",
		MaxNameLen: 6,
	},
	"widget": {
		TOMLKey:    "widgets",
		DirPrefix:  "WIDGETS",
		Template:   "widget.lua.tmpl",
		MaxNameLen: 8,
		DirBased:   true,
	},
}

type Options struct {
	Type    string
	Name    string
	Depends []string
	SrcDir  string
}

type Result struct {
	FilePath    string
	ContentPath string
}

func validTypeNames() string {
	names := make([]string, 0, len(Types))
	for k := range Types {
		names = append(names, k)
	}
	sort.Strings(names)
	return strings.Join(names, ", ")
}

func Run(opts Options) (*Result, error) {
	st, ok := Types[opts.Type]
	if !ok {
		return nil, fmt.Errorf("unknown script type %q (valid types: %s)", opts.Type, validTypeNames())
	}

	m, err := manifest.Load(opts.SrcDir)
	if err != nil {
		return nil, fmt.Errorf("loading manifest: %w", err)
	}

	if !namePattern.MatchString(opts.Name) {
		return nil, fmt.Errorf("invalid name %q: must match %s", opts.Name, namePattern.String())
	}

	if st.MaxNameLen > 0 && len(opts.Name) > st.MaxNameLen {
		return nil, fmt.Errorf("name %q is too long for %s scripts (max %d characters)", opts.Name, opts.Type, st.MaxNameLen)
	}

	if err := checkDuplicate(m, st.TOMLKey, opts.Name); err != nil {
		return nil, err
	}

	if err := validateDepends(m, opts.Depends); err != nil {
		return nil, err
	}

	var contentPath string
	var filePath string
	if st.DirBased {
		contentPath = st.DirPrefix + "/" + opts.Name
		filePath = filepath.Join(opts.SrcDir, contentPath, "main.lua")
	} else {
		contentPath = st.DirPrefix + "/" + opts.Name + ".lua"
		filePath = filepath.Join(opts.SrcDir, contentPath)
	}

	if err := os.MkdirAll(filepath.Dir(filePath), 0o755); err != nil {
		return nil, fmt.Errorf("creating directory: %w", err)
	}

	tmpl, err := template.ParseFS(templates, "templates/"+st.Template)
	if err != nil {
		return nil, fmt.Errorf("parsing template: %w", err)
	}

	f, err := os.Create(filePath)
	if err != nil {
		return nil, fmt.Errorf("creating file: %w", err)
	}
	defer f.Close()

	data := map[string]string{"Name": opts.Name}
	if err := tmpl.Execute(f, data); err != nil {
		return nil, fmt.Errorf("executing template: %w", err)
	}

	if err := appendToManifest(opts.SrcDir, st.TOMLKey, opts.Name, contentPath, opts.Depends); err != nil {
		return nil, fmt.Errorf("updating manifest: %w", err)
	}

	return &Result{
		FilePath:    filePath,
		ContentPath: contentPath,
	}, nil
}

func checkDuplicate(m *manifest.Manifest, tomlKey, name string) error {
	var items []manifest.ContentItem
	switch tomlKey {
	case "tools":
		items = m.Tools
	case "telemetry":
		items = m.Telemetry
	case "functions":
		items = m.Functions
	case "mixes":
		items = m.Mixes
	case "widgets":
		items = m.Widgets
	}
	for _, item := range items {
		if item.Name == name {
			return fmt.Errorf("name %q already exists in [[%s]]", name, tomlKey)
		}
	}
	return nil
}

func validateDepends(m *manifest.Manifest, depends []string) error {
	if len(depends) == 0 {
		return nil
	}
	libs := make(map[string]bool, len(m.Libraries))
	for _, lib := range m.Libraries {
		libs[lib.Name] = true
	}
	var unresolved []string
	for _, dep := range depends {
		if !libs[dep] {
			unresolved = append(unresolved, dep)
		}
	}
	if len(unresolved) > 0 {
		return fmt.Errorf("unresolved dependencies: %v (must reference [[libraries]] entries)", unresolved)
	}
	return nil
}

func appendToManifest(srcDir, tomlKey, name, path string, depends []string) error {
	manifestPath := filepath.Join(srcDir, manifest.FileName)

	var sb strings.Builder
	sb.WriteString(fmt.Sprintf("\n[[%s]]\n", tomlKey))
	sb.WriteString(fmt.Sprintf("name = %q\n", name))
	sb.WriteString(fmt.Sprintf("path = %q\n", path))
	if len(depends) > 0 {
		quoted := make([]string, len(depends))
		for i, d := range depends {
			quoted[i] = fmt.Sprintf("%q", d)
		}
		sb.WriteString(fmt.Sprintf("depends = [%s]\n", strings.Join(quoted, ", ")))
	}

	f, err := os.OpenFile(manifestPath, os.O_APPEND|os.O_WRONLY, 0o644)
	if err != nil {
		return fmt.Errorf("opening manifest for append: %w", err)
	}
	defer f.Close()

	if _, err := f.WriteString(sb.String()); err != nil {
		return fmt.Errorf("appending to manifest: %w", err)
	}

	return nil
}
