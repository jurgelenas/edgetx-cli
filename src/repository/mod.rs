pub mod clone;
pub mod source;
pub mod version;

use crate::error::RepositoryError;
use std::path::PathBuf;

/// PackageRef represents a parsed package reference.
#[derive(Debug, Clone)]
pub struct PackageRef {
    /// Empty for GitHub, or "gitea.example.com"
    pub host: String,
    pub owner: String,
    pub repo: String,
    /// Tag, branch, commit, or "" (latest)
    pub version: String,
    pub is_local: bool,
    pub local_path: PathBuf,
    /// Manifest file or subdirectory within the repo
    pub sub_path: String,
}

impl PackageRef {
    /// Canonical returns the canonical identifier for this package.
    pub fn canonical(&self) -> String {
        let base = if self.is_local {
            format!("local::{}", self.local_path.display())
        } else if !self.host.is_empty() {
            format!("{}/{}/{}", self.host, self.owner, self.repo)
        } else {
            format!("{}/{}", self.owner, self.repo)
        };
        if self.sub_path.is_empty() {
            base
        } else {
            format!("{base}::{}", self.sub_path)
        }
    }

    /// CloneURL returns the HTTPS clone URL for this package.
    pub fn clone_url(&self) -> String {
        let host = if self.host.is_empty() {
            "github.com"
        } else {
            &self.host
        };
        format!("https://{}/{}/{}.git", host, self.owner, self.repo)
    }
}

/// Parse a raw package reference string into a PackageRef.
pub fn parse_package_ref(raw: &str) -> Result<PackageRef, RepositoryError> {
    if raw.is_empty() {
        return Err(RepositoryError::EmptyRef);
    }

    // Local path detection: starts with ".", "/", or "~"
    if raw.starts_with('.') || raw.starts_with('/') || raw.starts_with('~') {
        return parse_local(raw);
    }

    // Check if it's an existing local directory
    if std::path::Path::new(raw).is_dir() {
        return parse_local(raw);
    }

    parse_remote(raw)
}

fn parse_local(raw: &str) -> Result<PackageRef, RepositoryError> {
    let path = if let Some(rest) = raw.strip_prefix('~') {
        let home = dirs_home().map_err(|e| RepositoryError::Other(e.to_string()))?;
        home.join(rest.trim_start_matches('/'))
    } else {
        PathBuf::from(raw)
    };

    let abs = std::fs::canonicalize(&path).unwrap_or_else(|_| {
        std::env::current_dir()
            .unwrap_or_default()
            .join(&path)
    });

    Ok(PackageRef {
        host: String::new(),
        owner: String::new(),
        repo: String::new(),
        version: String::new(),
        is_local: true,
        local_path: abs,
        sub_path: String::new(),
    })
}

fn parse_remote(raw: &str) -> Result<PackageRef, RepositoryError> {
    if raw.matches('@').count() > 1 {
        return Err(RepositoryError::InvalidRef {
            raw: raw.to_string(),
            reason: "multiple @ symbols".into(),
        });
    }

    let (remainder, version) = if let Some(idx) = raw.rfind('@') {
        let v = &raw[idx + 1..];
        if v.is_empty() {
            return Err(RepositoryError::InvalidRef {
                raw: raw.to_string(),
                reason: "empty version after @".into(),
            });
        }
        (&raw[..idx], v.to_string())
    } else {
        (raw, String::new())
    };

    // Strip scheme if present
    let clean = if let Some(idx) = remainder.find("://") {
        let after_scheme = &remainder[idx + 3..];
        after_scheme
    } else {
        remainder
    };

    // Remove trailing .git and /
    let clean = clean.trim_end_matches(".git").trim_end_matches('/');

    let parts: Vec<&str> = clean.split('/').collect();

    match parts.len() {
        2 => {
            if parts[0].is_empty() || parts[1].is_empty() {
                return Err(RepositoryError::InvalidRef {
                    raw: raw.to_string(),
                    reason: "empty owner or repo".into(),
                });
            }
            Ok(PackageRef {
                host: String::new(),
                owner: parts[0].to_string(),
                repo: parts[1].to_string(),
                version,
                is_local: false,
                local_path: PathBuf::new(),
                sub_path: String::new(),
            })
        }
        3 => {
            if parts[0].is_empty() || parts[1].is_empty() || parts[2].is_empty() {
                return Err(RepositoryError::InvalidRef {
                    raw: raw.to_string(),
                    reason: "empty host, owner, or repo".into(),
                });
            }
            if !parts[0].contains('.') {
                return Err(RepositoryError::InvalidRef {
                    raw: raw.to_string(),
                    reason: "expected host.com/org/repo or Org/Repo format".into(),
                });
            }
            Ok(PackageRef {
                host: parts[0].to_string(),
                owner: parts[1].to_string(),
                repo: parts[2].to_string(),
                version,
                is_local: false,
                local_path: PathBuf::new(),
                sub_path: String::new(),
            })
        }
        _ => Err(RepositoryError::InvalidRef {
            raw: raw.to_string(),
            reason: "expected Org/Repo or host.com/org/repo format".into(),
        }),
    }
}

fn dirs_home() -> Result<PathBuf, String> {
    dirs::home_dir().ok_or_else(|| "could not determine home directory".to_string())
}

// Re-export dirs for home dir usage
fn dirs() -> Option<PathBuf> {
    dirs::home_dir()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_github_shorthand() {
        let r = parse_package_ref("ExpressLRS/Lua-Scripts").unwrap();
        assert_eq!(r.owner, "ExpressLRS");
        assert_eq!(r.repo, "Lua-Scripts");
        assert!(r.host.is_empty());
        assert!(!r.is_local);
        assert_eq!(r.canonical(), "ExpressLRS/Lua-Scripts");
        assert_eq!(
            r.clone_url(),
            "https://github.com/ExpressLRS/Lua-Scripts.git"
        );
    }

    #[test]
    fn test_github_with_version() {
        let r = parse_package_ref("Org/Repo@v1.0.0").unwrap();
        assert_eq!(r.owner, "Org");
        assert_eq!(r.repo, "Repo");
        assert_eq!(r.version, "v1.0.0");
    }

    #[test]
    fn test_full_url() {
        let r = parse_package_ref("gitea.example.com/org/repo").unwrap();
        assert_eq!(r.host, "gitea.example.com");
        assert_eq!(r.owner, "org");
        assert_eq!(r.repo, "repo");
        assert_eq!(r.canonical(), "gitea.example.com/org/repo");
    }

    #[test]
    fn test_full_url_with_scheme() {
        let r = parse_package_ref("https://gitea.example.com/org/repo@v2.0").unwrap();
        assert_eq!(r.host, "gitea.example.com");
        assert_eq!(r.version, "v2.0");
    }

    #[test]
    fn test_local_relative() {
        let r = parse_package_ref(".").unwrap();
        assert!(r.is_local);
    }

    #[test]
    fn test_local_absolute() {
        let r = parse_package_ref("/tmp").unwrap();
        assert!(r.is_local);
        assert_eq!(r.local_path, PathBuf::from("/tmp"));
    }

    #[test]
    fn test_empty_ref() {
        assert!(parse_package_ref("").is_err());
    }

    #[test]
    fn test_multiple_at() {
        assert!(parse_package_ref("a@b@c").is_err());
    }

    #[test]
    fn test_empty_version() {
        assert!(parse_package_ref("Org/Repo@").is_err());
    }

    #[test]
    fn test_git_suffix_stripped() {
        let r = parse_package_ref("Org/Repo.git").unwrap();
        assert_eq!(r.repo, "Repo");
    }

    #[test]
    fn test_trailing_slash_stripped() {
        let r = parse_package_ref("Org/Repo/").unwrap();
        assert_eq!(r.repo, "Repo");
    }
}
