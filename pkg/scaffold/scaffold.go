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

type TemplateFile struct {
	Template string // e.g. "widget_main.lua.tmpl"
	Filename string // e.g. "main.lua"
}

type ScriptType struct {
	TOMLKey    string
	DirPrefix  string
	Templates  []TemplateFile
	MaxNameLen int
}

// DirBased returns true when the script type produces a directory of files
// (i.e. the templates have explicit filenames) rather than a single loose .lua file.
func (st ScriptType) DirBased() bool {
	return st.Templates[0].Filename != ""
}

var Types = map[string]ScriptType{
	"tool": {
		TOMLKey:   "tools",
		DirPrefix: "SCRIPTS/TOOLS",
		Templates: []TemplateFile{
			{Template: "tool.lua.tmpl", Filename: "main.lua"},
		},
	},
	"telemetry": {
		TOMLKey:   "telemetry",
		DirPrefix: "SCRIPTS/TELEMETRY",
		Templates: []TemplateFile{
			{Template: "telemetry.lua.tmpl"},
		},
		MaxNameLen: 6,
	},
	"function": {
		TOMLKey:   "functions",
		DirPrefix: "SCRIPTS/FUNCTIONS",
		Templates: []TemplateFile{
			{Template: "function.lua.tmpl"},
		},
		MaxNameLen: 6,
	},
	"mix": {
		TOMLKey:   "mixes",
		DirPrefix: "SCRIPTS/MIXES",
		Templates: []TemplateFile{
			{Template: "mix.lua.tmpl"},
		},
		MaxNameLen: 6,
	},
	"widget": {
		TOMLKey:   "widgets",
		DirPrefix: "WIDGETS",
		Templates: []TemplateFile{
			{Template: "widget_main.lua.tmpl", Filename: "main.lua"},
			{Template: "widget_loadable.lua.tmpl", Filename: "loadable.lua"},
		},
		MaxNameLen: 8,
	},
	"library": {
		TOMLKey:   "libraries",
		DirPrefix: "SCRIPTS",
		Templates: []TemplateFile{
			{Template: "library.lua.tmpl", Filename: "main.lua"},
		},
	},
}

type Options struct {
	Type    string
	Name    string
	Depends []string
	SrcDir  string
}

type Result struct {
	Files       []string
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
	var baseDir string
	if st.DirBased() {
		contentPath = st.DirPrefix + "/" + opts.Name
		baseDir = filepath.Join(opts.SrcDir, contentPath)
	} else {
		contentPath = st.DirPrefix + "/" + opts.Name + ".lua"
		baseDir = filepath.Join(opts.SrcDir, st.DirPrefix)
	}

	if err := os.MkdirAll(baseDir, 0o755); err != nil {
		return nil, fmt.Errorf("creating directory: %w", err)
	}

	data := map[string]string{"Name": opts.Name}

	result := &Result{
		ContentPath: contentPath,
	}

	for _, tf := range st.Templates {
		filePath, err := renderTemplate(tf, st.DirBased(), baseDir, opts.SrcDir, contentPath, data)
		if err != nil {
			return nil, err
		}
		result.Files = append(result.Files, filePath)
	}

	if err := appendToManifest(opts.SrcDir, st.TOMLKey, opts.Name, contentPath, opts.Depends); err != nil {
		return nil, fmt.Errorf("updating manifest: %w", err)
	}

	return result, nil
}

func renderTemplate(tf TemplateFile, dirBased bool, baseDir, srcDir, contentPath string, data map[string]string) (string, error) {
	var filePath string
	if dirBased {
		filePath = filepath.Join(baseDir, tf.Filename)
	} else {
		filePath = filepath.Join(srcDir, contentPath)
	}

	tmpl, err := template.ParseFS(templates, "templates/"+tf.Template)
	if err != nil {
		return "", fmt.Errorf("parsing template: %w", err)
	}

	f, err := os.Create(filePath)
	if err != nil {
		return "", fmt.Errorf("creating file: %w", err)
	}
	defer f.Close()

	if err := tmpl.Execute(f, data); err != nil {
		return "", fmt.Errorf("executing template: %w", err)
	}

	return filePath, nil
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
	case "libraries":
		items = m.Libraries
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
	fmt.Fprintf(&sb, "\n[[%s]]\n", tomlKey)
	fmt.Fprintf(&sb, "name = %q\n", name)
	fmt.Fprintf(&sb, "path = %q\n", path)
	if len(depends) > 0 {
		quoted := make([]string, len(depends))
		for i, d := range depends {
			quoted[i] = fmt.Sprintf("%q", d)
		}
		fmt.Fprintf(&sb, "depends = [%s]\n", strings.Join(quoted, ", "))
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
