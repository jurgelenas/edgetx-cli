pub mod resolve;
pub mod version;

use std::path::PathBuf;
use std::str::FromStr;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum SourceError {
    #[error("invalid package reference {raw:?}: {reason}")]
    InvalidRef { raw: String, reason: String },
    #[error("resolving {url}: {reason}")]
    Resolve { url: String, reason: String },
    #[error(transparent)]
    Manifest(#[from] crate::manifest::ManifestError),
    #[error("{context}: {source}")]
    Io {
        context: String,
        source: std::io::Error,
    },
}

/// Describes the remote origin of a package. Git hosts and local file:// URLs.
#[derive(Debug, Clone)]
pub enum RemoteSource {
    Git {
        host: String,
        owner: String,
        repo: String,
    },
    File {
        path: PathBuf,
    },
}

impl RemoteSource {
    pub fn clone_url(&self) -> String {
        match self {
            RemoteSource::Git { host, owner, repo } => {
                format!("https://{host}/{owner}/{repo}.git")
            }
            RemoteSource::File { path } => format!("file://{}", path.display()),
        }
    }

    /// Canonical `host/owner/repo` form (or `file://path` for File sources). Does not include subpath.
    fn canonical_root(&self) -> String {
        match self {
            RemoteSource::Git { host, owner, repo } => format!("{host}/{owner}/{repo}"),
            RemoteSource::File { path } => format!("file://{}", path.display()),
        }
    }
}

/// PackageRef represents a parsed package reference — either a local path or a remote repository.
#[derive(Debug, Clone)]
pub enum PackageRef {
    Local {
        path: PathBuf,
        /// Manifest variant filename (e.g. `edgetx.c480x272.yml`). Not part of identity.
        variant: String,
    },
    Remote {
        source: RemoteSource,
        /// Tag, branch, commit, or "" (latest)
        version: String,
        /// Subpackage directory within the repo. Part of canonical identity.
        subpath: String,
        /// Manifest variant filename (e.g. `edgetx.c480x272.yml`). Not part of identity.
        variant: String,
    },
}

impl PackageRef {
    /// Canonical identifier — `host/owner/repo` plus optional `/subpath`. Does not include version or variant.
    pub fn canonical(&self) -> String {
        match self {
            PackageRef::Local { path, .. } => {
                // Local canonical is mostly informational — identity is taken from the manifest.
                format!("file://{}", path.display())
            }
            PackageRef::Remote {
                source, subpath, ..
            } => {
                let root = source.canonical_root();
                if subpath.is_empty() {
                    root
                } else {
                    format!("{root}/{subpath}")
                }
            }
        }
    }

    /// Clone URL for the remote source. Returns `None` for local packages.
    pub fn clone_url(&self) -> Option<String> {
        match self {
            PackageRef::Remote { source, .. } => Some(source.clone_url()),
            PackageRef::Local { .. } => None,
        }
    }

    pub fn is_local(&self) -> bool {
        matches!(self, PackageRef::Local { .. })
    }

    pub fn version(&self) -> &str {
        match self {
            PackageRef::Remote { version, .. } => version,
            PackageRef::Local { .. } => "",
        }
    }

    pub fn set_version(&mut self, v: String) {
        if let PackageRef::Remote { version, .. } = self {
            *version = v;
        }
    }

    /// Subpackage directory (part of identity). Empty for Local refs.
    pub fn subpath(&self) -> &str {
        match self {
            PackageRef::Remote { subpath, .. } => subpath,
            PackageRef::Local { .. } => "",
        }
    }

    /// Variant manifest filename (install-time selector, not part of identity).
    pub fn variant(&self) -> &str {
        match self {
            PackageRef::Local { variant, .. } | PackageRef::Remote { variant, .. } => variant,
        }
    }

    pub fn set_variant(&mut self, v: String) {
        match self {
            PackageRef::Local { variant, .. } | PackageRef::Remote { variant, .. } => {
                *variant = v;
            }
        }
    }
}

impl FromStr for PackageRef {
    type Err = SourceError;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        if raw.is_empty() {
            return Err(SourceError::InvalidRef {
                raw: String::new(),
                reason: "empty package reference".into(),
            });
        }

        // file:// URLs are remote (File source)
        if raw.starts_with("file://") {
            return parse_remote(raw);
        }

        // Paths starting with . / ~ are local
        if raw.starts_with('.') || raw.starts_with('/') || raw.starts_with('~') {
            return parse_local(raw);
        }

        // Check if it's an existing local directory
        if std::path::Path::new(raw).is_dir() {
            return parse_local(raw);
        }

        parse_remote(raw)
    }
}

fn parse_local(raw: &str) -> Result<PackageRef, SourceError> {
    // Split on "::" to extract variant selector, if any
    let (path_str, variant) = split_first(raw, "::");

    let path = if let Some(rest) = path_str.strip_prefix('~') {
        let home = dirs::home_dir().ok_or_else(|| SourceError::Io {
            context: "could not determine home directory".into(),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "home directory not found"),
        })?;
        home.join(rest.trim_start_matches('/'))
    } else {
        PathBuf::from(path_str)
    };

    let abs = std::fs::canonicalize(&path)
        .unwrap_or_else(|_| std::env::current_dir().unwrap_or_default().join(&path));

    Ok(PackageRef::Local {
        path: abs,
        variant: variant.to_string(),
    })
}

fn parse_remote(raw: &str) -> Result<PackageRef, SourceError> {
    if raw.matches('@').count() > 1 {
        return Err(SourceError::InvalidRef {
            raw: raw.to_string(),
            reason: "multiple @ symbols".into(),
        });
    }

    let (remainder, version) = if let Some(idx) = raw.rfind('@') {
        let v = &raw[idx + 1..];
        if v.is_empty() {
            return Err(SourceError::InvalidRef {
                raw: raw.to_string(),
                reason: "empty version after @".into(),
            });
        }
        (&raw[..idx], v.to_string())
    } else {
        (raw, String::new())
    };

    // Extract variant selector via "::"
    let (base_str, variant) = split_first(remainder, "::");

    // file:// scheme — RemoteSource::File
    if let Some(path_str) = base_str.strip_prefix("file://") {
        return Ok(PackageRef::Remote {
            source: RemoteSource::File {
                path: PathBuf::from(path_str),
            },
            version,
            subpath: String::new(),
            variant: variant.to_string(),
        });
    }

    // Strip scheme if present (https://, etc.)
    let clean = if let Some(idx) = base_str.find("://") {
        &base_str[idx + 3..]
    } else {
        base_str
    };

    // Remove trailing .git and /
    let clean = clean.trim_end_matches(".git").trim_end_matches('/');

    let parts: Vec<&str> = clean.split('/').collect();

    // Determine host/owner/repo + optional subpath:
    //   - 2 parts: GitHub shorthand "Owner/Repo"
    //   - 3+ parts: first segment contains '.' → explicit host/owner/repo + optional subpath
    //   - 3+ parts: first segment does NOT contain '.' → GitHub shorthand "Owner/Repo/subpath"
    if parts.iter().any(|p| p.is_empty()) {
        return Err(SourceError::InvalidRef {
            raw: raw.to_string(),
            reason: "empty segment".into(),
        });
    }

    let (host, owner, repo, subpath) = match parts.len() {
        0 | 1 => {
            return Err(SourceError::InvalidRef {
                raw: raw.to_string(),
                reason: "expected Owner/Repo or host.com/Owner/Repo[/sub/path] format".into(),
            });
        }
        2 => (
            "github.com".to_string(),
            parts[0].to_string(),
            parts[1].to_string(),
            String::new(),
        ),
        _ => {
            if parts[0].contains('.') {
                // Explicit host
                (
                    parts[0].to_string(),
                    parts[1].to_string(),
                    parts[2].to_string(),
                    parts[3..].join("/"),
                )
            } else {
                // GitHub shorthand with subpath: Owner/Repo/sub/path
                (
                    "github.com".to_string(),
                    parts[0].to_string(),
                    parts[1].to_string(),
                    parts[2..].join("/"),
                )
            }
        }
    };

    Ok(PackageRef::Remote {
        source: RemoteSource::Git { host, owner, repo },
        version,
        subpath,
        variant: variant.to_string(),
    })
}

/// Split on the first occurrence of sep. If not found, returns (s, "").
fn split_first<'a>(s: &'a str, sep: &str) -> (&'a str, &'a str) {
    match s.find(sep) {
        Some(idx) => (&s[..idx], &s[idx + sep.len()..]),
        None => (s, ""),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_github_shorthand() {
        let r: PackageRef = "ExpressLRS/Lua-Scripts".parse().unwrap();
        match &r {
            PackageRef::Remote { source, .. } => match source {
                RemoteSource::Git { host, owner, repo } => {
                    assert_eq!(host, "github.com");
                    assert_eq!(owner, "ExpressLRS");
                    assert_eq!(repo, "Lua-Scripts");
                }
                _ => panic!("expected Git source"),
            },
            _ => panic!("expected Remote"),
        }
        assert!(!r.is_local());
        assert_eq!(r.canonical(), "github.com/ExpressLRS/Lua-Scripts");
        assert_eq!(
            r.clone_url().unwrap(),
            "https://github.com/ExpressLRS/Lua-Scripts.git"
        );
    }

    #[test]
    fn test_github_with_version() {
        let r: PackageRef = "Org/Repo@v1.0.0".parse().unwrap();
        assert_eq!(r.version(), "v1.0.0");
        assert_eq!(r.canonical(), "github.com/Org/Repo");
    }

    #[test]
    fn test_hosted_three_segment() {
        let r: PackageRef = "gitea.example.com/org/repo".parse().unwrap();
        match &r {
            PackageRef::Remote { source, .. } => match source {
                RemoteSource::Git { host, owner, repo } => {
                    assert_eq!(host, "gitea.example.com");
                    assert_eq!(owner, "org");
                    assert_eq!(repo, "repo");
                }
                _ => panic!("expected Git source"),
            },
            _ => panic!("expected Remote"),
        }
        assert_eq!(r.canonical(), "gitea.example.com/org/repo");
    }

    #[test]
    fn test_full_url_with_scheme() {
        let r: PackageRef = "https://gitea.example.com/org/repo@v2.0".parse().unwrap();
        match &r {
            PackageRef::Remote {
                source, version, ..
            } => {
                match source {
                    RemoteSource::Git { host, .. } => assert_eq!(host, "gitea.example.com"),
                    _ => panic!("expected Git source"),
                }
                assert_eq!(version, "v2.0");
            }
            _ => panic!("expected Remote"),
        }
    }

    #[test]
    fn test_local_relative() {
        let r: PackageRef = ".".parse().unwrap();
        assert!(r.is_local());
    }

    #[test]
    fn test_local_absolute() {
        let r: PackageRef = "/tmp".parse().unwrap();
        assert!(r.is_local());
        match &r {
            PackageRef::Local { path, .. } => {
                assert_eq!(path, &std::fs::canonicalize("/tmp").unwrap())
            }
            _ => panic!("expected Local"),
        }
    }

    #[test]
    fn test_empty_ref() {
        assert!("".parse::<PackageRef>().is_err());
    }

    #[test]
    fn test_multiple_at() {
        assert!("a@b@c".parse::<PackageRef>().is_err());
    }

    #[test]
    fn test_empty_version() {
        assert!("Org/Repo@".parse::<PackageRef>().is_err());
    }

    #[test]
    fn test_git_suffix_stripped() {
        let r: PackageRef = "Org/Repo.git".parse().unwrap();
        match &r {
            PackageRef::Remote { source, .. } => match source {
                RemoteSource::Git { repo, .. } => assert_eq!(repo, "Repo"),
                _ => panic!("expected Git source"),
            },
            _ => panic!("expected Remote"),
        }
    }

    #[test]
    fn test_trailing_slash_stripped() {
        let r: PackageRef = "Org/Repo/".parse().unwrap();
        match &r {
            PackageRef::Remote { source, .. } => match source {
                RemoteSource::Git { repo, .. } => assert_eq!(repo, "Repo"),
                _ => panic!("expected Git source"),
            },
            _ => panic!("expected Remote"),
        }
    }

    #[test]
    fn test_parse_simple_remote() {
        let r: PackageRef = "ExpressLRS/Lua-Scripts".parse().unwrap();
        assert!(r.subpath().is_empty());
        assert!(r.variant().is_empty());
        assert!(r.version().is_empty());
        assert!(!r.is_local());
    }

    #[test]
    fn test_parse_remote_with_version() {
        let r: PackageRef = "ExpressLRS/Lua-Scripts@v1.6.0".parse().unwrap();
        assert_eq!(r.version(), "v1.6.0");
        assert_eq!(r.canonical(), "github.com/ExpressLRS/Lua-Scripts");
    }

    #[test]
    fn test_parse_remote_with_variant_and_version() {
        let r: PackageRef = "Org/Repo::edgetx.c480x272.yml@branch".parse().unwrap();
        assert_eq!(r.variant(), "edgetx.c480x272.yml");
        assert!(r.subpath().is_empty());
        assert_eq!(r.version(), "branch");
        assert_eq!(r.canonical(), "github.com/Org/Repo");
    }

    #[test]
    fn test_parse_local_dot() {
        let r: PackageRef = ".".parse().unwrap();
        assert!(r.is_local());
    }

    #[test]
    fn test_parse_local_absolute() {
        let r: PackageRef = "/abs/path".parse().unwrap();
        assert!(r.is_local());
    }

    #[test]
    fn test_parse_local_tilde() {
        let r: PackageRef = "~/dir".parse().unwrap();
        assert!(r.is_local());
    }

    #[test]
    fn test_canonical_remote_with_variant() {
        let r: PackageRef = "Org/Repo::variant.yml@v1.0".parse().unwrap();
        // Variant is not part of canonical
        assert_eq!(r.canonical(), "github.com/Org/Repo");
        assert_eq!(r.variant(), "variant.yml");
    }

    #[test]
    fn test_canonical_local() {
        let r: PackageRef = "/tmp".parse().unwrap();
        // Local canonical is informational only (file://)
        assert!(r.canonical().starts_with("file://"));
    }

    #[test]
    fn test_subpackage_shorthand() {
        let r: PackageRef = "Org/Repo/widget-a".parse().unwrap();
        assert_eq!(r.canonical(), "github.com/Org/Repo/widget-a");
        assert_eq!(r.subpath(), "widget-a");
        match &r {
            PackageRef::Remote { source, .. } => match source {
                RemoteSource::Git { host, owner, repo } => {
                    assert_eq!(host, "github.com");
                    assert_eq!(owner, "Org");
                    assert_eq!(repo, "Repo");
                }
                _ => panic!("expected Git"),
            },
            _ => panic!("expected Remote"),
        }
        assert_eq!(r.clone_url().unwrap(), "https://github.com/Org/Repo.git");
    }

    #[test]
    fn test_subpackage_explicit_host() {
        let r: PackageRef = "gitea.example.com/org/repo/sub/path".parse().unwrap();
        assert_eq!(r.canonical(), "gitea.example.com/org/repo/sub/path");
        assert_eq!(r.subpath(), "sub/path");
        assert_eq!(
            r.clone_url().unwrap(),
            "https://gitea.example.com/org/repo.git"
        );
    }

    #[test]
    fn test_subpackage_with_variant() {
        let r: PackageRef = "Org/Repo/widget-a::edgetx.c480x272.yml".parse().unwrap();
        assert_eq!(r.canonical(), "github.com/Org/Repo/widget-a");
        assert_eq!(r.subpath(), "widget-a");
        assert_eq!(r.variant(), "edgetx.c480x272.yml");
    }

    #[test]
    fn test_file_url() {
        let r: PackageRef = "file:///path/to/repo".parse().unwrap();
        match &r {
            PackageRef::Remote { source, .. } => match source {
                RemoteSource::File { path } => {
                    assert_eq!(path, &PathBuf::from("/path/to/repo"));
                }
                _ => panic!("expected File source"),
            },
            _ => panic!("expected Remote"),
        }
        assert_eq!(r.clone_url().unwrap(), "file:///path/to/repo");
    }

    #[test]
    fn test_file_url_with_version() {
        let r: PackageRef = "file:///path/to/repo@v1.0".parse().unwrap();
        assert_eq!(r.version(), "v1.0");
    }

    #[test]
    fn test_clone_url_github() {
        let r: PackageRef = "Org/Repo".parse().unwrap();
        assert_eq!(r.clone_url().unwrap(), "https://github.com/Org/Repo.git");
    }

    #[test]
    fn test_clone_url_hosted() {
        let r: PackageRef = "gitea.com/org/repo".parse().unwrap();
        assert_eq!(r.clone_url().unwrap(), "https://gitea.com/org/repo.git");
    }

    #[test]
    fn test_clone_url_file() {
        let r: PackageRef = "file:///tmp/repo".parse().unwrap();
        assert_eq!(r.clone_url().unwrap(), "file:///tmp/repo");
    }

    #[test]
    fn test_clone_url_local() {
        let r = PackageRef::Local {
            path: PathBuf::from("/tmp/local"),
            variant: String::new(),
        };
        assert!(r.clone_url().is_none());
    }
}
