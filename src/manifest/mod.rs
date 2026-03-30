use crate::packages::path::PackagePath;
use regex::Regex;
use serde::{Deserialize, Deserializer, Serialize};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ManifestError {
    #[error("reading manifest {path}: {source}")]
    Read {
        path: String,
        source: std::io::Error,
    },
    #[error("parsing manifest {path}: {source}")]
    Parse {
        path: String,
        source: serde_yml::Error,
    },
    #[error("invalid manifest {path}: {message}")]
    Validation { path: String, message: String },
    #[error("content path {path:?} not found in any source root")]
    ContentPathNotFound { path: String },
}

pub const FILE_NAME: &str = "edgetx.yml";

static VALID_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-zA-Z0-9][a-zA-Z0-9_-]*$").unwrap());

/// A YAML field that accepts either a single string or a list of strings.
#[derive(Debug, Clone, Default, Serialize)]
pub struct StringOrSlice(pub Vec<String>);

impl<'de> Deserialize<'de> for StringOrSlice {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de;

        struct Visitor;
        impl<'de> de::Visitor<'de> for Visitor {
            type Value = StringOrSlice;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a string or a list of strings")
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<StringOrSlice, E> {
                Ok(StringOrSlice(vec![v.to_string()]))
            }

            fn visit_seq<A: de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> Result<StringOrSlice, A::Error> {
                let mut v = Vec::new();
                while let Some(s) = seq.next_element::<String>()? {
                    v.push(s);
                }
                Ok(StringOrSlice(v))
            }
        }

        deserializer.deserialize_any(Visitor)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Package {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub license: String,
    #[serde(default, skip_serializing_if = "is_empty_sos")]
    pub source_dir: StringOrSlice,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub binary: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub min_edgetx_version: String,
}

fn is_empty_sos(s: &StringOrSlice) -> bool {
    s.0.is_empty()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentItem {
    pub name: String,
    pub path: PackagePath,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub depends: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude: Vec<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub dev: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Manifest {
    pub package: Package,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub libraries: Vec<ContentItem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ContentItem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub telemetry: Vec<ContentItem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub functions: Vec<ContentItem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mixes: Vec<ContentItem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub widgets: Vec<ContentItem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sounds: Vec<ContentItem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<ContentItem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<ContentItem>,
}

/// Load reads and parses edgetx.yml from the given directory.
pub fn load(dir: &Path) -> Result<Manifest, ManifestError> {
    load_file(&dir.join(FILE_NAME))
}

/// Load reads and parses a manifest from the given file path.
pub fn load_file(path: &Path) -> Result<Manifest, ManifestError> {
    let data = std::fs::read_to_string(path).map_err(|e| ManifestError::Read {
        path: path.display().to_string(),
        source: e,
    })?;

    let m: Manifest = serde_yml::from_str(&data).map_err(|e| ManifestError::Parse {
        path: path.display().to_string(),
        source: e,
    })?;

    let manifest_dir = path.parent().unwrap_or(Path::new("."));
    m.validate(manifest_dir)?;

    Ok(m)
}

/// Load a manifest from a directory with an optional sub_path.
/// If sub_path is empty, loads from dir. If it ends in .yml/.yaml, loads that file.
/// Otherwise, treats sub_path as a subdirectory containing edgetx.yml.
pub fn load_with_sub_path(
    dir: &Path,
    sub_path: &str,
) -> Result<(Manifest, PathBuf), ManifestError> {
    if sub_path.is_empty() {
        let m = load(dir)?;
        Ok((m, dir.to_path_buf()))
    } else if sub_path.ends_with(".yml") || sub_path.ends_with(".yaml") {
        let path = dir.join(sub_path);
        let m = load_file(&path)?;
        let mdir = path.parent().unwrap_or(dir).to_path_buf();
        Ok((m, mdir))
    } else {
        let m = load(&dir.join(sub_path))?;
        Ok((m, dir.join(sub_path)))
    }
}

impl Manifest {
    /// Validate checks manifest integrity: name, dependencies, source dirs, content paths.
    pub fn validate(&self, manifest_dir: &Path) -> Result<(), ManifestError> {
        let path_str = manifest_dir.display().to_string();

        if self.package.name.is_empty() {
            return Err(ManifestError::Validation {
                path: path_str,
                message: "package name is required".into(),
            });
        }
        if !VALID_NAME.is_match(&self.package.name) {
            return Err(ManifestError::Validation {
                path: path_str,
                message: format!(
                    "package name {:?} must contain only alphanumeric characters, dashes, and underscores",
                    self.package.name
                ),
            });
        }

        if !self.package.min_edgetx_version.is_empty() {
            let v = &self.package.min_edgetx_version;
            let sv = if v.starts_with('v') {
                v.to_string()
            } else {
                format!("v{v}")
            };
            if semver::Version::parse(sv.trim_start_matches('v')).is_err() {
                return Err(ManifestError::Validation {
                    path: path_str,
                    message: format!("min_edgetx_version {v:?} is not a valid semver version"),
                });
            }
        }

        // Build library lookup
        let mut libs = std::collections::HashSet::new();
        let mut dev_libs = std::collections::HashSet::new();
        for lib in &self.libraries {
            libs.insert(lib.name.as_str());
            if lib.dev {
                dev_libs.insert(lib.name.as_str());
            }
        }

        let mut unresolved = Vec::new();
        let mut dev_errors = Vec::new();

        for group in self.all_content_groups() {
            for item in group {
                for dep in &item.depends {
                    if !libs.contains(dep.as_str()) {
                        unresolved.push(format!("{} depends on {dep:?}", item.name));
                    } else if !item.dev && dev_libs.contains(dep.as_str()) {
                        dev_errors.push(format!("{} depends on dev library {dep:?}", item.name));
                    }
                }
            }
        }

        if !unresolved.is_empty() {
            return Err(ManifestError::Validation {
                path: path_str,
                message: format!("unresolved library dependencies: {:?}", unresolved),
            });
        }
        if !dev_errors.is_empty() {
            return Err(ManifestError::Validation {
                path: path_str,
                message: format!("non-dev items depend on dev libraries: {:?}", dev_errors),
            });
        }

        // Check source directories exist
        for root in self.source_roots(manifest_dir) {
            if !root.is_dir() {
                return Err(ManifestError::Validation {
                    path: path_str,
                    message: format!("source directory {:?} does not exist", root.display()),
                });
            }
        }

        // Check content paths exist
        let mut missing: Vec<PackagePath> = Vec::new();
        for item in self.content_items(true) {
            if self.resolve_content_path(manifest_dir, &item.path).is_err() {
                missing.push(item.path.clone());
            }
        }
        if !missing.is_empty() {
            return Err(ManifestError::Validation {
                path: path_str,
                message: format!("content paths not found: {:?}", missing),
            });
        }

        Ok(())
    }

    /// Returns absolute paths to all source directories.
    pub fn source_roots(&self, manifest_dir: &Path) -> Vec<PathBuf> {
        if self.package.source_dir.0.is_empty() {
            vec![manifest_dir.to_path_buf()]
        } else {
            self.package
                .source_dir
                .0
                .iter()
                .map(|d| manifest_dir.join(d))
                .collect()
        }
    }

    /// Returns the source root directory where content_path exists.
    pub fn resolve_content_path(
        &self,
        manifest_dir: &Path,
        content_path: &PackagePath,
    ) -> Result<PathBuf, ManifestError> {
        for root in self.source_roots(manifest_dir) {
            let p = root.join(content_path.as_str());
            if p.exists() {
                return Ok(root);
            }
        }
        Err(ManifestError::ContentPathNotFound {
            path: content_path.to_string(),
        })
    }

    /// Returns content items, libraries first. When include_dev is false, dev items are excluded.
    pub fn content_items(&self, include_dev: bool) -> Vec<ContentItem> {
        let mut items = Vec::new();
        for group in self.all_groups_with_libraries() {
            for item in group {
                if !include_dev && item.dev {
                    continue;
                }
                items.push(item.clone());
            }
        }
        items
    }

    /// Returns all content paths, libraries first.
    pub fn all_paths(&self, include_dev: bool) -> Vec<PackagePath> {
        self.content_items(include_dev)
            .iter()
            .map(|item| item.path.clone())
            .collect()
    }

    fn all_content_groups(&self) -> Vec<&Vec<ContentItem>> {
        vec![
            &self.tools,
            &self.telemetry,
            &self.functions,
            &self.mixes,
            &self.widgets,
            &self.sounds,
            &self.images,
            &self.files,
        ]
    }

    fn all_groups_with_libraries(&self) -> Vec<&Vec<ContentItem>> {
        vec![
            &self.libraries,
            &self.tools,
            &self.telemetry,
            &self.functions,
            &self.mixes,
            &self.widgets,
            &self.sounds,
            &self.images,
            &self.files,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_manifest_dir(yml: &str, content_paths: &[&str]) -> TempDir {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join(FILE_NAME), yml).unwrap();
        for path in content_paths {
            let p = dir.path().join(path);
            fs::create_dir_all(&p).unwrap();
        }
        dir
    }

    #[test]
    fn test_load_valid_manifest() {
        let dir = create_manifest_dir(
            r#"
package:
  name: test-pkg
  description: "A test package"
tools:
  - name: MyTool
    path: SCRIPTS/TOOLS/MyTool
"#,
            &["SCRIPTS/TOOLS/MyTool"],
        );

        let m = load(dir.path()).unwrap();
        assert_eq!(m.package.name, "test-pkg");
        assert_eq!(m.tools.len(), 1);
        assert_eq!(m.tools[0].name, "MyTool");
    }

    #[test]
    fn test_name_required() {
        let dir = create_manifest_dir(
            r#"
package:
  name: ""
"#,
            &[],
        );
        let result = load(dir.path());
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_name() {
        let dir = create_manifest_dir(
            r#"
package:
  name: "bad name!"
"#,
            &[],
        );
        let result = load(dir.path());
        assert!(result.is_err());
    }

    #[test]
    fn test_unresolved_dependency() {
        let dir = create_manifest_dir(
            r#"
package:
  name: test
tools:
  - name: Tool
    path: SCRIPTS/TOOLS/Tool
    depends:
      - NonExistent
"#,
            &["SCRIPTS/TOOLS/Tool"],
        );
        let result = load(dir.path());
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("unresolved library dependencies")
        );
    }

    #[test]
    fn test_dev_dependency_error() {
        let dir = create_manifest_dir(
            r#"
package:
  name: test
libraries:
  - name: DevLib
    path: SCRIPTS/DevLib
    dev: true
tools:
  - name: Tool
    path: SCRIPTS/TOOLS/Tool
    depends:
      - DevLib
"#,
            &["SCRIPTS/DevLib", "SCRIPTS/TOOLS/Tool"],
        );
        let result = load(dir.path());
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("non-dev items depend on dev libraries")
        );
    }

    #[test]
    fn test_source_dir_single() {
        let dir = create_manifest_dir(
            r#"
package:
  name: test
  source_dir: src
tools:
  - name: Tool
    path: SCRIPTS/TOOLS/Tool
"#,
            &[],
        );
        // Create the source dir and content
        fs::create_dir_all(dir.path().join("src/SCRIPTS/TOOLS/Tool")).unwrap();

        let m = load(dir.path()).unwrap();
        let roots = m.source_roots(dir.path());
        assert_eq!(roots.len(), 1);
        assert!(roots[0].ends_with("src"));
    }

    #[test]
    fn test_source_dir_multiple() {
        let dir = create_manifest_dir(
            r#"
package:
  name: test
  source_dir: [src, lib]
libraries:
  - name: Lib
    path: SCRIPTS/Lib
tools:
  - name: Tool
    path: SCRIPTS/TOOLS/Tool
"#,
            &[],
        );
        fs::create_dir_all(dir.path().join("src/SCRIPTS/TOOLS/Tool")).unwrap();
        fs::create_dir_all(dir.path().join("lib/SCRIPTS/Lib")).unwrap();

        let m = load(dir.path()).unwrap();
        let roots = m.source_roots(dir.path());
        assert_eq!(roots.len(), 2);
    }

    #[test]
    fn test_content_items_dev_filtering() {
        let dir = create_manifest_dir(
            r#"
package:
  name: test
libraries:
  - name: Lib
    path: SCRIPTS/Lib
  - name: DevLib
    path: SCRIPTS/DevLib
    dev: true
tools:
  - name: Tool
    path: SCRIPTS/TOOLS/Tool
  - name: DevTool
    path: SCRIPTS/TOOLS/DevTool
    dev: true
"#,
            &[
                "SCRIPTS/Lib",
                "SCRIPTS/DevLib",
                "SCRIPTS/TOOLS/Tool",
                "SCRIPTS/TOOLS/DevTool",
            ],
        );

        let m = load(dir.path()).unwrap();

        let all = m.content_items(true);
        assert_eq!(all.len(), 4);

        let no_dev = m.content_items(false);
        assert_eq!(no_dev.len(), 2);
        assert!(no_dev.iter().all(|i| !i.dev));
    }

    #[test]
    fn test_all_paths() {
        let dir = create_manifest_dir(
            r#"
package:
  name: test
libraries:
  - name: ELRS
    path: SCRIPTS/ELRS
tools:
  - name: ExpressLRS
    path: SCRIPTS/TOOLS/ExpressLRS
"#,
            &["SCRIPTS/ELRS", "SCRIPTS/TOOLS/ExpressLRS"],
        );

        let m = load(dir.path()).unwrap();
        let paths = m.all_paths(true);
        assert_eq!(
            paths,
            vec![
                PackagePath::from("SCRIPTS/ELRS"),
                PackagePath::from("SCRIPTS/TOOLS/ExpressLRS"),
            ]
        );
    }

    #[test]
    fn test_min_edgetx_version_valid() {
        let dir = create_manifest_dir(
            r#"
package:
  name: test
  min_edgetx_version: "2.11.0"
"#,
            &[],
        );
        let m = load(dir.path()).unwrap();
        assert_eq!(m.package.min_edgetx_version, "2.11.0");
    }

    #[test]
    fn test_min_edgetx_version_invalid() {
        let dir = create_manifest_dir(
            r#"
package:
  name: test
  min_edgetx_version: "not-semver"
"#,
            &[],
        );
        let result = load(dir.path());
        assert!(result.is_err());
    }
}
