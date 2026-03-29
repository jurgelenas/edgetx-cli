use serde::{Deserialize, Serialize};
use std::fmt;

/// A forward-slash-separated relative path on the SD card (e.g. `SCRIPTS/TOOLS/MyTool`).
///
/// Unlike `PathBuf`, this always uses `/` as the separator regardless of platform,
/// because it represents a path on a FAT32 SD card. This ensures state files
/// (`packages.yml`, file lists) are portable across operating systems.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PackagePath(String);

impl PackagePath {
    pub fn new(s: impl Into<String>) -> Self {
        let s = s.into().replace('\\', "/");
        debug_assert!(!s.is_empty(), "PackagePath must not be empty");
        debug_assert!(
            !s.starts_with('/'),
            "PackagePath must be relative, got: {s}"
        );
        Self(s)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }
}

impl fmt::Display for PackagePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for PackagePath {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl From<String> for PackagePath {
    fn from(s: String) -> Self {
        Self::new(s)
    }
}

impl From<&str> for PackagePath {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

impl PartialEq<&str> for PackagePath {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

impl PartialEq<str> for PackagePath {
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_preserves_forward_slashes() {
        let p = PackagePath::new("SCRIPTS/TOOLS/MyTool");
        assert_eq!(p.as_str(), "SCRIPTS/TOOLS/MyTool");
    }

    #[test]
    fn new_normalizes_backslashes() {
        let p = PackagePath::new("SCRIPTS\\TOOLS\\MyTool");
        assert_eq!(p.as_str(), "SCRIPTS/TOOLS/MyTool");
    }

    #[test]
    fn display_shows_path() {
        let p = PackagePath::new("SCRIPTS/TOOLS/MyTool");
        assert_eq!(format!("{p}"), "SCRIPTS/TOOLS/MyTool");
    }

    #[test]
    fn partial_eq_str() {
        let p = PackagePath::new("SCRIPTS/TOOLS/MyTool");
        assert_eq!(p, "SCRIPTS/TOOLS/MyTool");
    }

    #[test]
    fn from_string() {
        let p: PackagePath = String::from("SCRIPTS/TOOLS/MyTool").into();
        assert_eq!(p, "SCRIPTS/TOOLS/MyTool");
    }

    #[test]
    fn from_str() {
        let p: PackagePath = "SCRIPTS/TOOLS/MyTool".into();
        assert_eq!(p, "SCRIPTS/TOOLS/MyTool");
    }

    #[test]
    fn serde_roundtrip() {
        let p = PackagePath::new("SCRIPTS/TOOLS/MyTool");
        let json = serde_json::to_string(&p).unwrap();
        assert_eq!(json, "\"SCRIPTS/TOOLS/MyTool\"");
        let p2: PackagePath = serde_json::from_str(&json).unwrap();
        assert_eq!(p, p2);
    }

    #[test]
    fn trailing_slash_allowed() {
        let p = PackagePath::new("SCRIPTS/TOOLS/MyTool/");
        assert_eq!(p.as_str(), "SCRIPTS/TOOLS/MyTool/");
    }
}
