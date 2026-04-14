use crate::packages::path::PackagePath;
use regex::Regex;
use serde::{Deserialize, Deserializer, Serialize};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ManifestError {
    #[error("reading manifest {}: {source}", path.display())]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("parsing manifest {}: {source}", path.display())]
    Parse {
        path: PathBuf,
        source: serde_yml::Error,
    },
    #[error("invalid manifest {}: {message}", path.display())]
    Validation { path: PathBuf, message: String },
    #[error("content path {path:?} not found in any source root")]
    ContentPathNotFound { path: PackagePath },
}

pub const FILE_NAME: &str = "edgetx.yml";

static VALID_ID: LazyLock<Regex> =
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

/// Author of a package.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Author {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
}

/// A named URL entry (e.g. homepage, repository, discord).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UrlEntry {
    pub name: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Package {
    pub id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub authors: Vec<Author>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub urls: Vec<UrlEntry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub screenshots: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keywords: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub license: String,
    #[serde(default, skip_serializing_if = "is_empty_sos")]
    pub source_dir: StringOrSlice,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub binary: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub min_edgetx_version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<RadioCapabilitiesFilter>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variants: Vec<Variant>,
}

impl Package {
    /// Returns the human-friendly display name, falling back to `id` if `name` is empty.
    pub fn display_name(&self) -> &str {
        if self.name.is_empty() {
            &self.id
        } else {
            &self.name
        }
    }
}

/// Display type: black-and-white or color LCD.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DisplayType {
    Bw,
    #[serde(rename = "colorlcd")]
    ColorLcd,
}

impl std::fmt::Display for DisplayType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DisplayType::Bw => write!(f, "bw"),
            DisplayType::ColorLcd => write!(f, "colorlcd"),
        }
    }
}

/// Display resolution, e.g. 480x272.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
    pub width: u32,
    pub height: u32,
}

impl std::fmt::Display for Resolution {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}x{}", self.width, self.height)
    }
}

impl std::str::FromStr for Resolution {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = s.split('x').collect();
        if parts.len() != 2 {
            return Err(format!("resolution {s:?} must be in WIDTHxHEIGHT format"));
        }
        let width = parts[0]
            .parse::<u32>()
            .map_err(|_| format!("invalid width in resolution {s:?}"))?;
        let height = parts[1]
            .parse::<u32>()
            .map_err(|_| format!("invalid height in resolution {s:?}"))?;
        Ok(Resolution { width, height })
    }
}

impl Serialize for Resolution {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Resolution {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

/// Display compatibility filter. All fields are optional — omit any to mean "any".
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DisplayFilter {
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "type")]
    pub display_type: Option<DisplayType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<Resolution>,
    /// Whether a touchscreen is required.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub touch: Option<bool>,
}

/// Describes a radio's display hardware.
#[derive(Debug, Clone)]
pub struct DisplayCapabilities {
    pub width: u32,
    pub height: u32,
    pub color: bool,
    pub touch: Option<bool>,
}

/// Describes a radio's full hardware capabilities, built from catalog + SD card info.
#[derive(Debug, Clone)]
pub struct RadioCapabilities {
    pub display: DisplayCapabilities,
}

/// Filter that a package or variant declares to describe what radio capabilities it requires.
/// Deserialized from `capabilities:` in the manifest YAML.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RadioCapabilitiesFilter {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display: Option<DisplayFilter>,
}

impl RadioCapabilitiesFilter {
    /// Returns true if all sub-filters match the given radio capabilities.
    pub fn matches(&self, radio: &RadioCapabilities) -> bool {
        if let Some(ref df) = self.display
            && !df.matches(&radio.display)
        {
            return false;
        }
        true
    }

    /// Returns a specificity score (higher = more specific match).
    pub fn specificity(&self) -> u8 {
        self.display.as_ref().map_or(0, |df| df.specificity())
    }
}

impl DisplayFilter {
    /// Returns true if the filter matches the given display capabilities.
    pub fn matches(&self, display: &DisplayCapabilities) -> bool {
        if let Some(ref dt) = self.display_type {
            let want_color = *dt == DisplayType::ColorLcd;
            if want_color != display.color {
                return false;
            }
        }
        if let Some(ref res) = self.resolution
            && (res.width != display.width || res.height != display.height)
        {
            return false;
        }
        if let Some(true) = self.touch {
            match display.touch {
                Some(true) => {}
                Some(false) => return false,
                None => {} // unknown touch capability, don't reject
            }
        }
        true
    }

    /// Returns a specificity score (higher = more specific match).
    fn specificity(&self) -> u8 {
        let mut s = 0;
        if self.display_type.is_some() {
            s += 1;
        }
        if self.resolution.is_some() {
            s += 2;
        }
        if self.touch.is_some() {
            s += 1;
        }
        s
    }
}

impl std::fmt::Display for DisplayFilter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut parts: Vec<String> = Vec::new();
        if let Some(ref dt) = self.display_type {
            parts.push(dt.to_string());
        }
        if let Some(ref res) = self.resolution {
            parts.push(res.to_string());
        }
        if self.touch == Some(true) {
            parts.push("touch".into());
        }
        write!(f, "{}", parts.join(", "))
    }
}

/// A variant pointing to another manifest file for specific radio capabilities.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Variant {
    /// Relative path to the variant manifest file (e.g. "edgetx.bw128x64.yml").
    pub path: String,
    /// Capabilities filter that this variant targets.
    pub capabilities: RadioCapabilitiesFilter,
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
        path: path.to_path_buf(),
        source: e,
    })?;

    let m: Manifest = serde_yml::from_str(&data).map_err(|e| ManifestError::Parse {
        path: path.to_path_buf(),
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
    /// Returns true if this manifest declares display variants.
    pub fn has_variants(&self) -> bool {
        !self.package.variants.is_empty()
    }

    /// Select the best matching variant for the given radio capabilities.
    /// Returns the variant, or None if no variants match.
    /// Prefers the most specific match (exact resolution > type-only).
    pub fn select_variant(&self, radio: &RadioCapabilities) -> Option<&Variant> {
        self.package
            .variants
            .iter()
            .filter(|v| v.capabilities.matches(radio))
            .max_by_key(|v| v.capabilities.specificity())
    }

    /// Validate checks manifest integrity: id, description, dependencies, source dirs, content paths.
    pub fn validate(&self, manifest_dir: &Path) -> Result<(), ManifestError> {
        let path = manifest_dir.to_path_buf();

        if self.package.id.is_empty() {
            return Err(ManifestError::Validation {
                path: path.clone(),
                message: "package id is required".into(),
            });
        }
        if !VALID_ID.is_match(&self.package.id) {
            return Err(ManifestError::Validation {
                path: path.clone(),
                message: format!(
                    "package id {:?} must contain only alphanumeric characters, dashes, and underscores",
                    self.package.id
                ),
            });
        }

        if self.package.description.is_empty() {
            return Err(ManifestError::Validation {
                path: path.clone(),
                message: "package description is required".into(),
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
                    path: path.clone(),
                    message: format!("min_edgetx_version {v:?} is not a valid semver version"),
                });
            }
        }

        // Validate author emails
        for author in &self.package.authors {
            if let Some(ref email) = author.email
                && !email_address::EmailAddress::is_valid(email)
            {
                return Err(ManifestError::Validation {
                    path: path.clone(),
                    message: format!("invalid email {:?} for author {:?}", email, author.name),
                });
            }
        }

        // Validate license (SPDX expression)
        if !self.package.license.is_empty()
            && spdx::Expression::parse(&self.package.license).is_err()
        {
            return Err(ManifestError::Validation {
                path: path.clone(),
                message: format!(
                    "license {:?} is not a valid SPDX expression (e.g. \"MIT\", \"GPL-3.0\", \"MIT OR Apache-2.0\")",
                    self.package.license
                ),
            });
        }

        // Validate URLs
        for entry in &self.package.urls {
            if url::Url::parse(&entry.url).is_err() {
                return Err(ManifestError::Validation {
                    path: path.clone(),
                    message: format!(
                        "url {:?} for {:?} is not a valid URL",
                        entry.url, entry.name
                    ),
                });
            }
        }

        // Validate screenshot paths exist
        for screenshot in &self.package.screenshots {
            let screenshot_path = manifest_dir.join(screenshot);
            if !screenshot_path.exists() {
                return Err(ManifestError::Validation {
                    path: path.clone(),
                    message: format!("screenshot {:?} not found", screenshot),
                });
            }
        }

        // Validate variant paths
        for variant in &self.package.variants {
            if !variant.path.ends_with(".yml") && !variant.path.ends_with(".yaml") {
                return Err(ManifestError::Validation {
                    path: path.clone(),
                    message: format!("variant path {:?} must end in .yml or .yaml", variant.path),
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
                path: path.clone(),
                message: format!("unresolved library dependencies: {:?}", unresolved),
            });
        }
        if !dev_errors.is_empty() {
            return Err(ManifestError::Validation {
                path: path.clone(),
                message: format!("non-dev items depend on dev libraries: {:?}", dev_errors),
            });
        }

        // Check source directories exist
        for root in self.source_roots(manifest_dir) {
            if !root.is_dir() {
                return Err(ManifestError::Validation {
                    path: path.clone(),
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
                path: path.clone(),
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
            path: content_path.clone(),
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
  id: test-pkg
  description: "A test package"
tools:
  - name: MyTool
    path: SCRIPTS/TOOLS/MyTool
"#,
            &["SCRIPTS/TOOLS/MyTool"],
        );

        let m = load(dir.path()).unwrap();
        assert_eq!(m.package.id, "test-pkg");
        assert_eq!(m.tools.len(), 1);
        assert_eq!(m.tools[0].name, "MyTool");
    }

    #[test]
    fn test_id_required() {
        let dir = create_manifest_dir(
            r#"
package:
  id: ""
  description: "Test"
"#,
            &[],
        );
        let result = load(dir.path());
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_id() {
        let dir = create_manifest_dir(
            r#"
package:
  id: "bad name!"
  description: "Test"
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
  id: test
  description: "Test package"
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
  id: test
  description: "Test package"
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
  id: test
  description: "Test package"
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
  id: test
  description: "Test package"
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
  id: test
  description: "Test package"
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
  id: test
  description: "Test package"
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
  id: test
  description: "Test package"
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
  id: test
  description: "Test package"
  min_edgetx_version: "not-semver"
"#,
            &[],
        );
        let result = load(dir.path());
        assert!(result.is_err());
    }

    #[test]
    fn test_metadata_fields() {
        let dir = create_manifest_dir(
            r#"
package:
  id: test-pkg
  name: "Test Package"
  description: "A test package"
  authors:
    - name: Alice
      email: alice@example.com
    - name: Bob
  urls:
    - name: Homepage
      url: "https://example.com"
  keywords: ["telemetry", "test"]
  license: MIT
"#,
            &[],
        );

        let m = load(dir.path()).unwrap();
        assert_eq!(m.package.id, "test-pkg");
        assert_eq!(m.package.name, "Test Package");
        assert_eq!(m.package.display_name(), "Test Package");
        assert_eq!(m.package.authors.len(), 2);
        assert_eq!(m.package.authors[0].name, "Alice");
        assert_eq!(
            m.package.authors[0].email,
            Some("alice@example.com".to_string())
        );
        assert_eq!(m.package.authors[1].name, "Bob");
        assert!(m.package.authors[1].email.is_none());
        assert_eq!(m.package.urls.len(), 1);
        assert_eq!(m.package.urls[0].name, "Homepage");
        assert_eq!(m.package.urls[0].url, "https://example.com");
        assert_eq!(m.package.keywords, vec!["telemetry", "test"]);
    }

    #[test]
    fn test_author_with_email() {
        let dir = create_manifest_dir(
            r#"
package:
  id: test-pkg
  description: "Test package"
  authors:
    - name: Alice
      email: alice@example.com
    - name: Bob
"#,
            &[],
        );
        let m = load(dir.path()).unwrap();
        assert_eq!(m.package.authors.len(), 2);
        assert_eq!(
            m.package.authors[0].email,
            Some("alice@example.com".to_string())
        );
        assert!(m.package.authors[1].email.is_none());
    }

    #[test]
    fn test_author_invalid_email() {
        let dir = create_manifest_dir(
            r#"
package:
  id: test-pkg
  description: "Test package"
  authors:
    - name: Alice
      email: not-an-email
"#,
            &[],
        );
        let result = load(dir.path());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("invalid email"));
    }

    #[test]
    fn test_license_valid_spdx() {
        let dir = create_manifest_dir(
            r#"
package:
  id: test-pkg
  description: "Test package"
  license: "MIT OR Apache-2.0"
"#,
            &[],
        );
        assert!(load(dir.path()).is_ok());
    }

    #[test]
    fn test_license_invalid_spdx() {
        let dir = create_manifest_dir(
            r#"
package:
  id: test-pkg
  description: "Test package"
  license: "NOT-A-LICENSE"
"#,
            &[],
        );
        let result = load(dir.path());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("SPDX"));
    }

    #[test]
    fn test_url_valid() {
        let dir = create_manifest_dir(
            r#"
package:
  id: test-pkg
  description: "Test package"
  urls:
    - name: Homepage
      url: "https://example.com/project"
"#,
            &[],
        );
        assert!(load(dir.path()).is_ok());
    }

    #[test]
    fn test_url_invalid() {
        let dir = create_manifest_dir(
            r#"
package:
  id: test-pkg
  description: "Test package"
  urls:
    - name: Homepage
      url: "not a url"
"#,
            &[],
        );
        let result = load(dir.path());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not a valid URL"));
    }

    #[test]
    fn test_metadata_fields_optional() {
        let dir = create_manifest_dir(
            r#"
package:
  id: test-pkg
  description: "Test package"
"#,
            &[],
        );

        let m = load(dir.path()).unwrap();
        assert_eq!(m.package.name, "");
        assert_eq!(m.package.display_name(), "test-pkg"); // falls back to id
        assert!(m.package.authors.is_empty());
        assert!(m.package.urls.is_empty());
        assert!(m.package.screenshots.is_empty());
        assert!(m.package.keywords.is_empty());
        assert!(m.package.capabilities.is_none());
        assert!(m.package.variants.is_empty());
    }

    #[test]
    fn test_display_filter() {
        let dir = create_manifest_dir(
            r#"
package:
  id: test-pkg
  description: "Test package"
  capabilities:
    display:
      type: colorlcd
      resolution: 480x272
      touch: true
"#,
            &[],
        );

        let m = load(dir.path()).unwrap();
        let display = m.package.capabilities.unwrap().display.unwrap();
        assert_eq!(display.display_type, Some(DisplayType::ColorLcd));
        assert_eq!(
            display.resolution,
            Some(Resolution {
                width: 480,
                height: 272
            })
        );
        assert_eq!(display.touch, Some(true));
    }

    #[test]
    fn test_display_filter_partial() {
        let dir = create_manifest_dir(
            r#"
package:
  id: test-pkg
  description: "Test package"
  capabilities:
    display:
      type: bw
"#,
            &[],
        );

        let m = load(dir.path()).unwrap();
        let display = m.package.capabilities.unwrap().display.unwrap();
        assert_eq!(display.display_type, Some(DisplayType::Bw));
        assert!(display.resolution.is_none());
        assert!(display.touch.is_none());
    }

    #[test]
    fn test_variants() {
        let dir = create_manifest_dir(
            r#"
package:
  id: test-pkg
  description: "Multi-variant package"
  variants:
    - path: edgetx.bw128x64.yml
      capabilities:
        display:
          type: bw
          resolution: 128x64
    - path: edgetx.color.yml
      capabilities:
        display:
          type: colorlcd
    - path: edgetx.color-touch.yml
      capabilities:
        display:
          type: colorlcd
          touch: true
"#,
            &[],
        );

        let m = load(dir.path()).unwrap();
        assert_eq!(m.package.variants.len(), 3);

        let v0_display = m.package.variants[0].capabilities.display.as_ref().unwrap();
        assert_eq!(m.package.variants[0].path, "edgetx.bw128x64.yml");
        assert_eq!(v0_display.display_type, Some(DisplayType::Bw));
        assert_eq!(
            v0_display.resolution,
            Some(Resolution {
                width: 128,
                height: 64
            })
        );

        let v1_display = m.package.variants[1].capabilities.display.as_ref().unwrap();
        assert_eq!(m.package.variants[1].path, "edgetx.color.yml");
        assert_eq!(v1_display.display_type, Some(DisplayType::ColorLcd));
        assert!(v1_display.resolution.is_none());

        let v2_display = m.package.variants[2].capabilities.display.as_ref().unwrap();
        assert_eq!(m.package.variants[2].path, "edgetx.color-touch.yml");
        assert_eq!(v2_display.touch, Some(true));
    }

    #[test]
    fn test_has_variants() {
        let dir = create_manifest_dir(
            r#"
package:
  id: test-pkg
  description: "Test package"
  variants:
    - path: edgetx.bw.yml
      capabilities:
        display:
          type: bw
"#,
            &[],
        );

        let m = load(dir.path()).unwrap();
        assert!(m.has_variants());
    }

    #[test]
    fn test_invalid_display_type() {
        let dir = create_manifest_dir(
            r#"
package:
  id: test-pkg
  description: "Test package"
  capabilities:
    display:
      type: invalid
"#,
            &[],
        );

        let result = load(dir.path());
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_variant_path() {
        let dir = create_manifest_dir(
            r#"
package:
  id: test-pkg
  description: "Test package"
  variants:
    - path: edgetx.bw.txt
      capabilities:
        display:
          type: bw
"#,
            &[],
        );

        let result = load(dir.path());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("must end in .yml"));
    }

    #[test]
    fn test_invalid_variant_display_type() {
        let dir = create_manifest_dir(
            r#"
package:
  id: test-pkg
  description: "Test package"
  variants:
    - path: edgetx.bw.yml
      capabilities:
        display:
          type: invalid
"#,
            &[],
        );

        let result = load(dir.path());
        assert!(result.is_err());
    }

    #[test]
    fn test_display_filter_matches_color() {
        let filter = DisplayFilter {
            display_type: Some(DisplayType::ColorLcd),
            resolution: None,
            touch: None,
        };
        let radio = DisplayCapabilities {
            width: 480,
            height: 272,
            color: true,
            touch: Some(true),
        };
        assert!(filter.matches(&radio));

        let bw_radio = DisplayCapabilities {
            width: 128,
            height: 64,
            color: false,
            touch: Some(false),
        };
        assert!(!filter.matches(&bw_radio));
    }

    #[test]
    fn test_display_filter_matches_resolution() {
        let filter = DisplayFilter {
            display_type: Some(DisplayType::Bw),
            resolution: Some(Resolution {
                width: 128,
                height: 64,
            }),
            touch: None,
        };
        let radio_128 = DisplayCapabilities {
            width: 128,
            height: 64,
            color: false,
            touch: None,
        };
        assert!(filter.matches(&radio_128));

        let radio_212 = DisplayCapabilities {
            width: 212,
            height: 64,
            color: false,
            touch: None,
        };
        assert!(!filter.matches(&radio_212));
    }

    #[test]
    fn test_display_filter_matches_touch() {
        let filter = DisplayFilter {
            display_type: Some(DisplayType::ColorLcd),
            resolution: None,
            touch: Some(true),
        };
        let touch_radio = DisplayCapabilities {
            width: 480,
            height: 272,
            color: true,
            touch: Some(true),
        };
        assert!(filter.matches(&touch_radio));

        let no_touch = DisplayCapabilities {
            width: 480,
            height: 272,
            color: true,
            touch: Some(false),
        };
        assert!(!filter.matches(&no_touch));
    }

    #[test]
    fn test_select_variant_prefers_specific() {
        let dir = create_manifest_dir(
            r#"
package:
  id: test-pkg
  description: "Test package"
  variants:
    - path: edgetx.bw128x64.yml
      capabilities:
        display:
          type: bw
          resolution: 128x64
    - path: edgetx.bw.yml
      capabilities:
        display:
          type: bw
    - path: edgetx.color.yml
      capabilities:
        display:
          type: colorlcd
"#,
            &[],
        );

        let m = load(dir.path()).unwrap();

        // BW 128x64 radio should match the specific variant
        let radio = RadioCapabilities {
            display: DisplayCapabilities {
                width: 128,
                height: 64,
                color: false,
                touch: None,
            },
        };
        let selected = m.select_variant(&radio).unwrap();
        assert_eq!(selected.path, "edgetx.bw128x64.yml");

        // BW 212x64 radio should fall back to the generic BW variant
        let radio_212 = RadioCapabilities {
            display: DisplayCapabilities {
                width: 212,
                height: 64,
                color: false,
                touch: None,
            },
        };
        let selected = m.select_variant(&radio_212).unwrap();
        assert_eq!(selected.path, "edgetx.bw.yml");

        // Color radio should match the color variant
        let color_radio = RadioCapabilities {
            display: DisplayCapabilities {
                width: 480,
                height: 272,
                color: true,
                touch: None,
            },
        };
        let selected = m.select_variant(&color_radio).unwrap();
        assert_eq!(selected.path, "edgetx.color.yml");
    }

    #[test]
    fn test_select_variant_no_match() {
        let dir = create_manifest_dir(
            r#"
package:
  id: test-pkg
  description: "Test package"
  variants:
    - path: edgetx.color.yml
      capabilities:
        display:
          type: colorlcd
"#,
            &[],
        );

        let m = load(dir.path()).unwrap();
        let bw_radio = RadioCapabilities {
            display: DisplayCapabilities {
                width: 128,
                height: 64,
                color: false,
                touch: None,
            },
        };
        assert!(m.select_variant(&bw_radio).is_none());
    }
}
