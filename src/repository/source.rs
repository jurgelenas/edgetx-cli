/// Source represents a parsed package source string.
/// Source strings use "::" to separate an optional subpath and "@" to separate an optional version.
#[derive(Debug, Clone, Default)]
pub struct Source {
    /// "owner/repo", "host.com/org/repo", or "/abs/path"
    pub base: String,
    /// "" or "edgetx.c480x272.yml"
    pub sub_path: String,
    /// "" or "v1.0" or "main"
    pub version: String,
    /// true for local paths and "local::" prefix
    pub is_local: bool,
}

/// Parse a raw source/query string into a Source.
pub fn parse(raw: &str) -> Source {
    if raw.is_empty() {
        return Source::default();
    }

    // "local::" prefix means a stored local source.
    if let Some(remainder) = raw.strip_prefix("local::") {
        let (base, sub_path) = split_first(remainder, "::");
        return Source {
            base: base.to_string(),
            sub_path: sub_path.to_string(),
            version: String::new(),
            is_local: true,
        };
    }

    // Paths starting with . / ~ are local — never split on @.
    if raw.starts_with('.') || raw.starts_with('/') || raw.starts_with('~') {
        let (base, sub_path) = split_first(raw, "::");
        return Source {
            base: base.to_string(),
            sub_path: sub_path.to_string(),
            version: String::new(),
            is_local: true,
        };
    }

    // Remote: split last @ for version, then first :: for subpath.
    let (base_with_sub, version) = split_last(raw, "@");
    let (base, sub_path) = split_first(base_with_sub, "::");
    Source {
        base: base.to_string(),
        sub_path: sub_path.to_string(),
        version: version.to_string(),
        is_local: false,
    }
}

impl Source {
    /// Canonical returns the source identifier without version.
    pub fn canonical(&self) -> String {
        let mut s = String::new();
        if self.is_local {
            s.push_str("local::");
        }
        s.push_str(&self.base);
        if !self.sub_path.is_empty() {
            s.push_str("::");
            s.push_str(&self.sub_path);
        }
        s
    }

    /// Full returns the canonical form plus "@version" if a version is set.
    pub fn full(&self) -> String {
        let c = self.canonical();
        if !self.version.is_empty() {
            format!("{c}@{}", self.version)
        } else {
            c
        }
    }

    /// Returns a copy with the subpath set. Non-empty argument overrides existing.
    pub fn with_sub_path(mut self, p: &str) -> Self {
        if !p.is_empty() {
            self.sub_path = p.to_string();
        }
        self
    }

    /// Returns a copy with the version set.
    pub fn with_version(mut self, v: &str) -> Self {
        self.version = v.to_string();
        self
    }
}

/// Split on the first occurrence of sep. If not found, returns (s, "").
fn split_first<'a>(s: &'a str, sep: &str) -> (&'a str, &'a str) {
    match s.find(sep) {
        Some(idx) => (&s[..idx], &s[idx + sep.len()..]),
        None => (s, ""),
    }
}

/// Split on the last occurrence of sep. If not found or only at position 0, returns (s, "").
fn split_last<'a>(s: &'a str, sep: &str) -> (&'a str, &'a str) {
    match s.rfind(sep) {
        Some(idx) if idx > 0 => (&s[..idx], &s[idx + sep.len()..]),
        _ => (s, ""),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_empty() {
        let s = parse("");
        assert!(s.base.is_empty());
    }

    #[test]
    fn test_parse_simple_remote() {
        let s = parse("ExpressLRS/Lua-Scripts");
        assert_eq!(s.base, "ExpressLRS/Lua-Scripts");
        assert!(s.sub_path.is_empty());
        assert!(s.version.is_empty());
        assert!(!s.is_local);
    }

    #[test]
    fn test_parse_remote_with_version() {
        let s = parse("ExpressLRS/Lua-Scripts@v1.6.0");
        assert_eq!(s.base, "ExpressLRS/Lua-Scripts");
        assert_eq!(s.version, "v1.6.0");
    }

    #[test]
    fn test_parse_remote_with_subpath_and_version() {
        let s = parse("Org/Repo::edgetx.c480x272.yml@branch");
        assert_eq!(s.base, "Org/Repo");
        assert_eq!(s.sub_path, "edgetx.c480x272.yml");
        assert_eq!(s.version, "branch");
    }

    #[test]
    fn test_parse_local_dot() {
        let s = parse(".");
        assert_eq!(s.base, ".");
        assert!(s.is_local);
    }

    #[test]
    fn test_parse_local_absolute() {
        let s = parse("/abs/path");
        assert_eq!(s.base, "/abs/path");
        assert!(s.is_local);
    }

    #[test]
    fn test_parse_local_tilde() {
        let s = parse("~/dir");
        assert_eq!(s.base, "~/dir");
        assert!(s.is_local);
    }

    #[test]
    fn test_parse_stored_local() {
        let s = parse("local::/abs/path::sub");
        assert_eq!(s.base, "/abs/path");
        assert_eq!(s.sub_path, "sub");
        assert!(s.is_local);
    }

    #[test]
    fn test_canonical() {
        let s = parse("Org/Repo::sub@v1.0");
        assert_eq!(s.canonical(), "Org/Repo::sub");
    }

    #[test]
    fn test_full() {
        let s = parse("Org/Repo::sub@v1.0");
        assert_eq!(s.full(), "Org/Repo::sub@v1.0");
    }

    #[test]
    fn test_canonical_local() {
        let s = parse("local::/path::sub");
        assert_eq!(s.canonical(), "local::/path::sub");
    }

    #[test]
    fn test_local_no_at_split() {
        // Local paths should not split on @
        let s = parse("./path@with-at");
        assert_eq!(s.base, "./path@with-at");
        assert!(s.version.is_empty());
        assert!(s.is_local);
    }

    #[test]
    fn test_with_sub_path() {
        let s = parse("Org/Repo");
        let s2 = s.with_sub_path("edgetx.yml");
        assert_eq!(s2.sub_path, "edgetx.yml");
    }

    #[test]
    fn test_with_sub_path_empty_preserves() {
        let s = parse("Org/Repo::existing");
        let s2 = s.with_sub_path("");
        assert_eq!(s2.sub_path, "existing");
    }
}
