use regex::Regex;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use thiserror::Error;

use crate::manifest::{self, ContentItem};

#[derive(Error, Debug)]
pub enum ScaffoldError {
    #[error("{0}")]
    Validation(String),
    #[error(transparent)]
    Manifest(#[from] crate::manifest::ManifestError),
    #[error("{context}: {source}")]
    Io {
        context: String,
        source: std::io::Error,
    },
}

static NAME_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z][A-Za-z0-9_]*$").unwrap());

// Lua template contents embedded at compile time
const TOOL_TEMPLATE: &str = include_str!("../../templates/tool.lua.tmpl");
const WIDGET_MAIN_TEMPLATE: &str = include_str!("../../templates/widget_main.lua.tmpl");
const WIDGET_LOADABLE_TEMPLATE: &str = include_str!("../../templates/widget_loadable.lua.tmpl");
const TELEMETRY_TEMPLATE: &str = include_str!("../../templates/telemetry.lua.tmpl");
const FUNCTION_TEMPLATE: &str = include_str!("../../templates/function.lua.tmpl");
const MIX_TEMPLATE: &str = include_str!("../../templates/mix.lua.tmpl");
const LIBRARY_TEMPLATE: &str = include_str!("../../templates/library.lua.tmpl");

pub struct TemplateFile {
    #[allow(dead_code)]
    pub template: &'static str,
    pub filename: &'static str, // empty string means loose file
    pub content: &'static str,
}

pub struct ScriptSpec {
    pub dir_prefix: &'static str,
    pub templates: Vec<TemplateFile>,
    pub max_name_len: usize, // 0 = no limit
}

impl ScriptSpec {
    pub fn dir_based(&self) -> bool {
        !self.templates[0].filename.is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptType {
    Tool,
    Telemetry,
    Function,
    Mix,
    Widget,
    Library,
}

impl ScriptType {
    pub fn yaml_key(&self) -> &'static str {
        match self {
            Self::Tool => "tools",
            Self::Telemetry => "telemetry",
            Self::Function => "functions",
            Self::Mix => "mixes",
            Self::Widget => "widgets",
            Self::Library => "libraries",
        }
    }

    pub fn spec(&self) -> ScriptSpec {
        match self {
            Self::Tool => ScriptSpec {
                dir_prefix: "SCRIPTS/TOOLS",
                templates: vec![TemplateFile {
                    template: "tool.lua.tmpl",
                    filename: "main.lua",
                    content: TOOL_TEMPLATE,
                }],
                max_name_len: 0,
            },
            Self::Telemetry => ScriptSpec {
                dir_prefix: "SCRIPTS/TELEMETRY",
                templates: vec![TemplateFile {
                    template: "telemetry.lua.tmpl",
                    filename: "",
                    content: TELEMETRY_TEMPLATE,
                }],
                max_name_len: 6,
            },
            Self::Function => ScriptSpec {
                dir_prefix: "SCRIPTS/FUNCTIONS",
                templates: vec![TemplateFile {
                    template: "function.lua.tmpl",
                    filename: "",
                    content: FUNCTION_TEMPLATE,
                }],
                max_name_len: 6,
            },
            Self::Mix => ScriptSpec {
                dir_prefix: "SCRIPTS/MIXES",
                templates: vec![TemplateFile {
                    template: "mix.lua.tmpl",
                    filename: "",
                    content: MIX_TEMPLATE,
                }],
                max_name_len: 6,
            },
            Self::Widget => ScriptSpec {
                dir_prefix: "WIDGETS",
                templates: vec![
                    TemplateFile {
                        template: "widget_main.lua.tmpl",
                        filename: "main.lua",
                        content: WIDGET_MAIN_TEMPLATE,
                    },
                    TemplateFile {
                        template: "widget_loadable.lua.tmpl",
                        filename: "loadable.lua",
                        content: WIDGET_LOADABLE_TEMPLATE,
                    },
                ],
                max_name_len: 8,
            },
            Self::Library => ScriptSpec {
                dir_prefix: "SCRIPTS",
                templates: vec![TemplateFile {
                    template: "library.lua.tmpl",
                    filename: "main.lua",
                    content: LIBRARY_TEMPLATE,
                }],
                max_name_len: 0,
            },
        }
    }
}

impl std::fmt::Display for ScriptType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Tool => write!(f, "tool"),
            Self::Telemetry => write!(f, "telemetry"),
            Self::Function => write!(f, "function"),
            Self::Mix => write!(f, "mix"),
            Self::Widget => write!(f, "widget"),
            Self::Library => write!(f, "library"),
        }
    }
}

impl std::str::FromStr for ScriptType {
    type Err = ScaffoldError;

    fn from_str(s: &str) -> Result<Self, ScaffoldError> {
        match s {
            "tool" => Ok(Self::Tool),
            "telemetry" => Ok(Self::Telemetry),
            "function" => Ok(Self::Function),
            "mix" => Ok(Self::Mix),
            "widget" => Ok(Self::Widget),
            "library" => Ok(Self::Library),
            _ => Err(ScaffoldError::Validation(format!(
                "unknown script type {:?} (valid types: tool, telemetry, function, mix, widget, library)",
                s
            ))),
        }
    }
}

pub struct Options {
    pub script_type: ScriptType,
    pub name: String,
    pub depends: Vec<String>,
    pub src_dir: PathBuf,
    pub dev: bool,
}

pub struct ScaffoldResult {
    pub files: Vec<PathBuf>,
    #[allow(dead_code)]
    pub content_path: String,
}

pub fn run(opts: Options) -> Result<ScaffoldResult, ScaffoldError> {
    let st = opts.script_type.spec();

    let m = manifest::load(&opts.src_dir)?;

    if !NAME_PATTERN.is_match(&opts.name) {
        return Err(ScaffoldError::Validation(format!(
            "invalid name {:?}: must match {}",
            opts.name,
            NAME_PATTERN.as_str()
        )));
    }

    if st.max_name_len > 0 && opts.name.len() > st.max_name_len {
        return Err(ScaffoldError::Validation(format!(
            "name {:?} is too long for {} scripts (max {} characters)",
            opts.name, opts.script_type, st.max_name_len
        )));
    }

    let yaml_key = opts.script_type.yaml_key();

    // Check duplicates
    check_duplicate(&m, yaml_key, &opts.name)?;

    // Validate dependencies
    validate_depends(&m, &opts.depends)?;

    // Determine paths
    let (content_path, base_dir) = if st.dir_based() {
        let cp = format!("{}/{}", st.dir_prefix, opts.name);
        let bd = opts.src_dir.join(&cp);
        (cp, bd)
    } else {
        let cp = format!("{}/{}.lua", st.dir_prefix, opts.name);
        let bd = opts.src_dir.join(st.dir_prefix);
        (cp, bd)
    };

    std::fs::create_dir_all(&base_dir).map_err(|e| ScaffoldError::Io {
        context: "creating directory".into(),
        source: e,
    })?;

    let mut result = ScaffoldResult {
        files: Vec::new(),
        content_path: content_path.clone(),
    };

    for tf in &st.templates {
        let file_path = if st.dir_based() {
            base_dir.join(tf.filename)
        } else {
            opts.src_dir.join(&content_path)
        };

        // Simple template rendering: replace {{ .Name }} with actual name
        let content = tf.content.replace("{{ .Name }}", &opts.name);

        std::fs::write(&file_path, &content).map_err(|e| ScaffoldError::Io {
            context: format!("creating {}", file_path.display()),
            source: e,
        })?;

        result.files.push(file_path);
    }

    // Update manifest
    append_to_manifest(
        &opts.src_dir,
        yaml_key,
        &opts.name,
        &content_path,
        &opts.depends,
        opts.dev,
    )?;

    Ok(result)
}

fn check_duplicate(
    m: &manifest::Manifest,
    yaml_key: &str,
    name: &str,
) -> Result<(), ScaffoldError> {
    let items: &[ContentItem] = match yaml_key {
        "tools" => &m.tools,
        "telemetry" => &m.telemetry,
        "functions" => &m.functions,
        "mixes" => &m.mixes,
        "widgets" => &m.widgets,
        "libraries" => &m.libraries,
        "sounds" => &m.sounds,
        "images" => &m.images,
        "files" => &m.files,
        _ => &[],
    };

    for item in items {
        if item.name == name {
            return Err(ScaffoldError::Validation(format!(
                "name {:?} already exists in {}",
                name, yaml_key
            )));
        }
    }
    Ok(())
}

fn validate_depends(m: &manifest::Manifest, depends: &[String]) -> Result<(), ScaffoldError> {
    if depends.is_empty() {
        return Ok(());
    }

    let libs: std::collections::HashSet<&str> =
        m.libraries.iter().map(|l| l.name.as_str()).collect();

    let unresolved: Vec<&String> = depends
        .iter()
        .filter(|d| !libs.contains(d.as_str()))
        .collect();

    if !unresolved.is_empty() {
        return Err(ScaffoldError::Validation(format!(
            "unresolved dependencies: {:?} (must reference libraries entries)",
            unresolved
        )));
    }
    Ok(())
}

fn append_to_manifest(
    src_dir: &Path,
    yaml_key: &str,
    name: &str,
    path: &str,
    depends: &[String],
    dev: bool,
) -> Result<(), ScaffoldError> {
    let manifest_path = src_dir.join(manifest::FILE_NAME);

    let data = std::fs::read_to_string(&manifest_path).map_err(|e| ScaffoldError::Io {
        context: "reading manifest for append".into(),
        source: e,
    })?;

    let mut raw: serde_yml::Value = serde_yml::from_str(&data)
        .map_err(|e| ScaffoldError::Validation(format!("parsing manifest: {e}")))?;

    let mut entry = serde_yml::Mapping::new();
    entry.insert(
        serde_yml::Value::String("name".into()),
        serde_yml::Value::String(name.into()),
    );
    entry.insert(
        serde_yml::Value::String("path".into()),
        serde_yml::Value::String(path.into()),
    );
    if !depends.is_empty() {
        let deps: Vec<serde_yml::Value> = depends
            .iter()
            .map(|d| serde_yml::Value::String(d.clone()))
            .collect();
        entry.insert(
            serde_yml::Value::String("depends".into()),
            serde_yml::Value::Sequence(deps),
        );
    }
    if dev {
        entry.insert(
            serde_yml::Value::String("dev".into()),
            serde_yml::Value::Bool(true),
        );
    }

    if let serde_yml::Value::Mapping(map) = &mut raw {
        let key = serde_yml::Value::String(yaml_key.into());
        let existing = map
            .entry(key.clone())
            .or_insert_with(|| serde_yml::Value::Sequence(Vec::new()));

        if let serde_yml::Value::Sequence(seq) = existing {
            seq.push(serde_yml::Value::Mapping(entry));
        }
    }

    let out = serde_yml::to_string(&raw)
        .map_err(|e| ScaffoldError::Validation(format!("marshaling manifest: {e}")))?;
    std::fs::write(&manifest_path, out).map_err(|e| ScaffoldError::Io {
        context: "writing manifest".into(),
        source: e,
    })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_scaffold_dir() -> TempDir {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("edgetx.yml"),
            "package:\n  id: example.com/test/test\n  description: \"Test package\"\n",
        )
        .unwrap();
        dir
    }

    #[test]
    fn test_scaffold_tool() {
        let dir = setup_scaffold_dir();
        let result = run(Options {
            script_type: ScriptType::Tool,
            name: "MyTool".into(),
            depends: vec![],
            src_dir: dir.path().to_path_buf(),
            dev: false,
        })
        .unwrap();

        assert_eq!(result.content_path, "SCRIPTS/TOOLS/MyTool");
        assert_eq!(result.files.len(), 1);
        assert!(dir.path().join("SCRIPTS/TOOLS/MyTool/main.lua").exists());
    }

    #[test]
    fn test_scaffold_widget() {
        let dir = setup_scaffold_dir();
        let result = run(Options {
            script_type: ScriptType::Widget,
            name: "MyWdgt".into(),
            depends: vec![],
            src_dir: dir.path().to_path_buf(),
            dev: false,
        })
        .unwrap();

        assert_eq!(result.files.len(), 2);
        assert!(dir.path().join("WIDGETS/MyWdgt/main.lua").exists());
        assert!(dir.path().join("WIDGETS/MyWdgt/loadable.lua").exists());
    }

    #[test]
    fn test_scaffold_telemetry() {
        let dir = setup_scaffold_dir();
        let result = run(Options {
            script_type: ScriptType::Telemetry,
            name: "MyTlm".into(),
            depends: vec![],
            src_dir: dir.path().to_path_buf(),
            dev: false,
        })
        .unwrap();

        assert_eq!(result.content_path, "SCRIPTS/TELEMETRY/MyTlm.lua");
        assert!(dir.path().join("SCRIPTS/TELEMETRY/MyTlm.lua").exists());
    }

    #[test]
    fn test_scaffold_name_too_long() {
        let dir = setup_scaffold_dir();
        let result = run(Options {
            script_type: ScriptType::Telemetry,
            name: "TooLongName".into(),
            depends: vec![],
            src_dir: dir.path().to_path_buf(),
            dev: false,
        });
        assert!(result.is_err());
    }

    #[test]
    fn test_scaffold_invalid_name() {
        let dir = setup_scaffold_dir();
        let result = run(Options {
            script_type: ScriptType::Tool,
            name: "bad name!".into(),
            depends: vec![],
            src_dir: dir.path().to_path_buf(),
            dev: false,
        });
        assert!(result.is_err());
    }
}
