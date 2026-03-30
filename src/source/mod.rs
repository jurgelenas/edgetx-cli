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

/// Describes the remote origin of a package.
#[derive(Debug, Clone)]
pub enum RemoteSource {
    GitHub {
        owner: String,
        repo: String,
    },
    Hosted {
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
            RemoteSource::GitHub { owner, repo } => {
                format!("https://github.com/{owner}/{repo}.git")
            }
            RemoteSource::Hosted { host, owner, repo } => {
                format!("https://{host}/{owner}/{repo}.git")
            }
            RemoteSource::File { path } => format!("file://{}", path.display()),
        }
    }

    pub fn canonical(&self) -> String {
        match self {
            RemoteSource::GitHub { owner, repo } => format!("{owner}/{repo}"),
            RemoteSource::Hosted { host, owner, repo } => format!("{host}/{owner}/{repo}"),
            RemoteSource::File { path } => format!("file://{}", path.display()),
        }
    }
}

/// PackageRef represents a parsed package reference — either a local path or a remote repository.
#[derive(Debug, Clone)]
pub enum PackageRef {
    Local {
        path: PathBuf,
        sub_path: String,
    },
    Remote {
        source: RemoteSource,
        /// Tag, branch, commit, or "" (latest)
        version: String,
        /// Manifest file or subdirectory within the repo
        sub_path: String,
    },
}

impl PackageRef {
    /// Canonical identifier without version — used as storage key.
    pub fn canonical(&self) -> String {
        match self {
            PackageRef::Local { path, sub_path } => {
                let base = format!("local::{}", path.display());
                if sub_path.is_empty() {
                    base
                } else {
                    format!("{base}::{sub_path}")
                }
            }
            PackageRef::Remote {
                source, sub_path, ..
            } => {
                let base = source.canonical();
                if sub_path.is_empty() {
                    base
                } else {
                    format!("{base}::{sub_path}")
                }
            }
        }
    }

    /// Canonical form plus `@version` if set.
    pub fn full(&self) -> String {
        let c = self.canonical();
        let v = self.version();
        if v.is_empty() { c } else { format!("{c}@{v}") }
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

    pub fn sub_path(&self) -> &str {
        match self {
            PackageRef::Local { sub_path, .. } | PackageRef::Remote { sub_path, .. } => sub_path,
        }
    }

    pub fn set_sub_path(&mut self, p: String) {
        match self {
            PackageRef::Local { sub_path, .. } | PackageRef::Remote { sub_path, .. } => {
                *sub_path = p;
            }
        }
    }

    pub fn with_sub_path(mut self, p: &str) -> Self {
        if !p.is_empty() {
            self.set_sub_path(p.to_string());
        }
        self
    }

    #[allow(dead_code)]
    pub fn with_version(mut self, v: &str) -> Self {
        self.set_version(v.to_string());
        self
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

        // "local::" prefix — stored canonical form for local packages
        if let Some(remainder) = raw.strip_prefix("local::") {
            let (base, sub_path) = split_first(remainder, "::");
            let path = PathBuf::from(base);
            return Ok(PackageRef::Local {
                path,
                sub_path: sub_path.to_string(),
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
    let (path_str, sub_path) = split_first(raw, "::");

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
        sub_path: sub_path.to_string(),
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

    // Split sub_path on "::" before cleaning the base
    let (base_str, sub_path) = split_first(remainder, "::");

    // file:// scheme — RemoteSource::File
    if let Some(path_str) = base_str.strip_prefix("file://") {
        return Ok(PackageRef::Remote {
            source: RemoteSource::File {
                path: PathBuf::from(path_str),
            },
            version,
            sub_path: sub_path.to_string(),
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

    match parts.len() {
        2 => {
            if parts[0].is_empty() || parts[1].is_empty() {
                return Err(SourceError::InvalidRef {
                    raw: raw.to_string(),
                    reason: "empty owner or repo".into(),
                });
            }
            Ok(PackageRef::Remote {
                source: RemoteSource::GitHub {
                    owner: parts[0].to_string(),
                    repo: parts[1].to_string(),
                },
                version,
                sub_path: sub_path.to_string(),
            })
        }
        3 => {
            if parts[0].is_empty() || parts[1].is_empty() || parts[2].is_empty() {
                return Err(SourceError::InvalidRef {
                    raw: raw.to_string(),
                    reason: "empty host, owner, or repo".into(),
                });
            }
            if !parts[0].contains('.') {
                return Err(SourceError::InvalidRef {
                    raw: raw.to_string(),
                    reason: "expected host.com/org/repo or Org/Repo format".into(),
                });
            }
            Ok(PackageRef::Remote {
                source: RemoteSource::Hosted {
                    host: parts[0].to_string(),
                    owner: parts[1].to_string(),
                    repo: parts[2].to_string(),
                },
                version,
                sub_path: sub_path.to_string(),
            })
        }
        _ => Err(SourceError::InvalidRef {
            raw: raw.to_string(),
            reason: "expected Org/Repo or host.com/org/repo format".into(),
        }),
    }
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
                RemoteSource::GitHub { owner, repo } => {
                    assert_eq!(owner, "ExpressLRS");
                    assert_eq!(repo, "Lua-Scripts");
                }
                _ => panic!("expected GitHub source"),
            },
            _ => panic!("expected Remote"),
        }
        assert!(!r.is_local());
        assert_eq!(r.canonical(), "ExpressLRS/Lua-Scripts");
        assert_eq!(
            r.clone_url().unwrap(),
            "https://github.com/ExpressLRS/Lua-Scripts.git"
        );
    }

    #[test]
    fn test_github_with_version() {
        let r: PackageRef = "Org/Repo@v1.0.0".parse().unwrap();
        assert_eq!(r.version(), "v1.0.0");
        match &r {
            PackageRef::Remote { source, .. } => match source {
                RemoteSource::GitHub { owner, repo } => {
                    assert_eq!(owner, "Org");
                    assert_eq!(repo, "Repo");
                }
                _ => panic!("expected GitHub source"),
            },
            _ => panic!("expected Remote"),
        }
    }

    #[test]
    fn test_full_url() {
        let r: PackageRef = "gitea.example.com/org/repo".parse().unwrap();
        match &r {
            PackageRef::Remote { source, .. } => match source {
                RemoteSource::Hosted { host, owner, repo } => {
                    assert_eq!(host, "gitea.example.com");
                    assert_eq!(owner, "org");
                    assert_eq!(repo, "repo");
                }
                _ => panic!("expected Hosted source"),
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
                    RemoteSource::Hosted { host, .. } => assert_eq!(host, "gitea.example.com"),
                    _ => panic!("expected Hosted source"),
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
            PackageRef::Local { path, .. } => assert_eq!(path, &PathBuf::from("/tmp")),
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
                RemoteSource::GitHub { repo, .. } => assert_eq!(repo, "Repo"),
                _ => panic!("expected GitHub source"),
            },
            _ => panic!("expected Remote"),
        }
    }

    #[test]
    fn test_trailing_slash_stripped() {
        let r: PackageRef = "Org/Repo/".parse().unwrap();
        match &r {
            PackageRef::Remote { source, .. } => match source {
                RemoteSource::GitHub { repo, .. } => assert_eq!(repo, "Repo"),
                _ => panic!("expected GitHub source"),
            },
            _ => panic!("expected Remote"),
        }
    }

    #[test]
    fn test_parse_simple_remote() {
        let r: PackageRef = "ExpressLRS/Lua-Scripts".parse().unwrap();
        assert!(r.sub_path().is_empty());
        assert!(r.version().is_empty());
        assert!(!r.is_local());
    }

    #[test]
    fn test_parse_remote_with_version() {
        let r: PackageRef = "ExpressLRS/Lua-Scripts@v1.6.0".parse().unwrap();
        assert_eq!(r.version(), "v1.6.0");
        assert_eq!(r.canonical(), "ExpressLRS/Lua-Scripts");
    }

    #[test]
    fn test_parse_remote_with_subpath_and_version() {
        let r: PackageRef = "Org/Repo::edgetx.c480x272.yml@branch".parse().unwrap();
        assert_eq!(r.sub_path(), "edgetx.c480x272.yml");
        assert_eq!(r.version(), "branch");
        assert_eq!(r.canonical(), "Org/Repo::edgetx.c480x272.yml");
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
    fn test_parse_stored_local() {
        let r: PackageRef = "local::/abs/path::sub".parse().unwrap();
        match &r {
            PackageRef::Local { path, sub_path } => {
                assert_eq!(path, &PathBuf::from("/abs/path"));
                assert_eq!(sub_path, "sub");
            }
            _ => panic!("expected Local"),
        }
    }

    #[test]
    fn test_canonical_remote_with_sub() {
        let r: PackageRef = "Org/Repo::sub@v1.0".parse().unwrap();
        assert_eq!(r.canonical(), "Org/Repo::sub");
    }

    #[test]
    fn test_full_remote() {
        let r: PackageRef = "Org/Repo::sub@v1.0".parse().unwrap();
        assert_eq!(r.full(), "Org/Repo::sub@v1.0");
    }

    #[test]
    fn test_canonical_local() {
        let r: PackageRef = "local::/path::sub".parse().unwrap();
        assert_eq!(r.canonical(), "local::/path::sub");
    }

    #[test]
    fn test_with_sub_path() {
        let r: PackageRef = "Org/Repo".parse().unwrap();
        let r2 = r.with_sub_path("edgetx.yml");
        assert_eq!(r2.sub_path(), "edgetx.yml");
    }

    #[test]
    fn test_with_sub_path_empty_preserves() {
        let r: PackageRef = "Org/Repo::existing".parse().unwrap();
        let r2 = r.with_sub_path("");
        assert_eq!(r2.sub_path(), "existing");
    }

    // New tests for file:// URLs and RemoteSource

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
        match &r {
            PackageRef::Remote { source, .. } => match source {
                RemoteSource::File { path } => {
                    assert_eq!(path, &PathBuf::from("/path/to/repo"));
                }
                _ => panic!("expected File source"),
            },
            _ => panic!("expected Remote"),
        }
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
            sub_path: String::new(),
        };
        assert!(r.clone_url().is_none());
    }
}
