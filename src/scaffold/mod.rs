use anyhow::{Context, Result, bail};
use regex::Regex;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use crate::manifest::{self, ContentItem};

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

pub struct ScriptType {
    pub yaml_key: &'static str,
    pub dir_prefix: &'static str,
    pub templates: Vec<TemplateFile>,
    pub max_name_len: usize, // 0 = no limit
}

impl ScriptType {
    pub fn dir_based(&self) -> bool {
        !self.templates[0].filename.is_empty()
    }
}

pub static TYPES: LazyLock<BTreeMap<&'static str, ScriptType>> = LazyLock::new(|| {
    let mut m = BTreeMap::new();
    m.insert(
        "tool",
        ScriptType {
            yaml_key: "tools",
            dir_prefix: "SCRIPTS/TOOLS",
            templates: vec![TemplateFile {
                template: "tool.lua.tmpl",
                filename: "main.lua",
                content: TOOL_TEMPLATE,
            }],
            max_name_len: 0,
        },
    );
    m.insert(
        "telemetry",
        ScriptType {
            yaml_key: "telemetry",
            dir_prefix: "SCRIPTS/TELEMETRY",
            templates: vec![TemplateFile {
                template: "telemetry.lua.tmpl",
                filename: "",
                content: TELEMETRY_TEMPLATE,
            }],
            max_name_len: 6,
        },
    );
    m.insert(
        "function",
        ScriptType {
            yaml_key: "functions",
            dir_prefix: "SCRIPTS/FUNCTIONS",
            templates: vec![TemplateFile {
                template: "function.lua.tmpl",
                filename: "",
                content: FUNCTION_TEMPLATE,
            }],
            max_name_len: 6,
        },
    );
    m.insert(
        "mix",
        ScriptType {
            yaml_key: "mixes",
            dir_prefix: "SCRIPTS/MIXES",
            templates: vec![TemplateFile {
                template: "mix.lua.tmpl",
                filename: "",
                content: MIX_TEMPLATE,
            }],
            max_name_len: 6,
        },
    );
    m.insert(
        "widget",
        ScriptType {
            yaml_key: "widgets",
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
    );
    m.insert(
        "library",
        ScriptType {
            yaml_key: "libraries",
            dir_prefix: "SCRIPTS",
            templates: vec![TemplateFile {
                template: "library.lua.tmpl",
                filename: "main.lua",
                content: LIBRARY_TEMPLATE,
            }],
            max_name_len: 0,
        },
    );
    m
});

pub struct Options {
    pub script_type: String,
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

pub fn run(opts: Options) -> Result<ScaffoldResult> {
    let st = TYPES.get(opts.script_type.as_str()).ok_or_else(|| {
        let valid: Vec<&str> = TYPES.keys().copied().collect();
        anyhow::anyhow!(
            "unknown script type {:?} (valid types: {})",
            opts.script_type,
            valid.join(", ")
        )
    })?;

    let m = manifest::load(&opts.src_dir).context("loading manifest")?;

    if !NAME_PATTERN.is_match(&opts.name) {
        bail!(
            "invalid name {:?}: must match {}",
            opts.name,
            NAME_PATTERN.as_str()
        );
    }

    if st.max_name_len > 0 && opts.name.len() > st.max_name_len {
        bail!(
            "name {:?} is too long for {} scripts (max {} characters)",
            opts.name,
            opts.script_type,
            st.max_name_len
        );
    }

    // Check duplicates
    check_duplicate(&m, st.yaml_key, &opts.name)?;

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

    std::fs::create_dir_all(&base_dir).context("creating directory")?;

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

        std::fs::write(&file_path, &content)
            .with_context(|| format!("creating {}", file_path.display()))?;

        result.files.push(file_path);
    }

    // Update manifest
    append_to_manifest(
        &opts.src_dir,
        st.yaml_key,
        &opts.name,
        &content_path,
        &opts.depends,
        opts.dev,
    )?;

    Ok(result)
}

fn check_duplicate(m: &manifest::Manifest, yaml_key: &str, name: &str) -> Result<()> {
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
            bail!("name {:?} already exists in {}", name, yaml_key);
        }
    }
    Ok(())
}

fn validate_depends(m: &manifest::Manifest, depends: &[String]) -> Result<()> {
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
        bail!(
            "unresolved dependencies: {:?} (must reference libraries entries)",
            unresolved
        );
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
) -> Result<()> {
    let manifest_path = src_dir.join(manifest::FILE_NAME);

    let data = std::fs::read_to_string(&manifest_path).context("reading manifest for append")?;

    let mut raw: serde_yml::Value =
        serde_yml::from_str(&data).context("parsing manifest for append")?;

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

    let out = serde_yml::to_string(&raw).context("marshaling manifest")?;
    std::fs::write(&manifest_path, out).context("writing manifest")?;

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
            "package:\n  name: test\n  description: \"\"\n",
        )
        .unwrap();
        dir
    }

    #[test]
    fn test_scaffold_tool() {
        let dir = setup_scaffold_dir();
        let result = run(Options {
            script_type: "tool".into(),
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
            script_type: "widget".into(),
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
            script_type: "telemetry".into(),
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
            script_type: "telemetry".into(),
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
            script_type: "tool".into(),
            name: "bad name!".into(),
            depends: vec![],
            src_dir: dir.path().to_path_buf(),
            dev: false,
        });
        assert!(result.is_err());
    }

    #[test]
    fn test_scaffold_unknown_type() {
        let dir = setup_scaffold_dir();
        let result = run(Options {
            script_type: "unknown".into(),
            name: "Test".into(),
            depends: vec![],
            src_dir: dir.path().to_path_buf(),
            dev: false,
        });
        assert!(result.is_err());
    }
}
