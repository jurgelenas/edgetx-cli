use super::SourceError;
use serde::{Deserialize, Serialize};
use std::fmt;

/// The channel through which a package version was resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    Tag,
    Branch,
    Commit,
    Local,
}

impl fmt::Display for Channel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Channel::Tag => write!(f, "tag"),
            Channel::Branch => write!(f, "branch"),
            Channel::Commit => write!(f, "commit"),
            Channel::Local => write!(f, "local"),
        }
    }
}

impl Channel {
    pub fn is_pinned(&self) -> bool {
        matches!(self, Channel::Commit)
    }

    pub fn is_local(&self) -> bool {
        matches!(self, Channel::Local)
    }
}

/// ResolvedVersion holds the result of version resolution.
#[derive(Debug, Clone)]
pub struct ResolvedVersion {
    pub channel: Channel,
    /// Tag name or branch name (empty for commit)
    pub version: String,
    /// Full commit hash
    pub hash: String,
}

/// Sort tags in descending semver order. Non-semver tags are filtered out.
pub fn sort_semver_tags(tags: &[String]) -> Vec<String> {
    let mut valid: Vec<(semver::Version, String)> = tags
        .iter()
        .filter_map(|tag| {
            let normalized = tag.strip_prefix('v').unwrap_or(tag);
            semver::Version::parse(normalized)
                .ok()
                .map(|v| (v, tag.clone()))
        })
        .collect();

    valid.sort_by(|a, b| b.0.cmp(&a.0));
    valid.into_iter().map(|(_, tag)| tag).collect()
}

/// Resolve a version string against a list of tags and branches.
/// This is a simplified version that works with tag/branch/commit lists
/// rather than a live git repository.
pub fn resolve_version(
    tags: &[String],
    branches: &[String],
    default_branch: &str,
    head_commit: &str,
    version: &str,
) -> Result<ResolvedVersion, SourceError> {
    if version.is_empty() {
        return resolve_latest(tags, default_branch, head_commit);
    }

    // Try exact tag match
    if tags.contains(&version.to_string()) {
        return Ok(ResolvedVersion {
            channel: Channel::Tag,
            version: version.into(),
            hash: String::new(), // Caller fills in from checkout
        });
    }

    // Try branch
    if branches.contains(&version.to_string()) {
        return Ok(ResolvedVersion {
            channel: Channel::Branch,
            version: version.into(),
            hash: String::new(),
        });
    }

    // Treat as commit SHA
    Ok(ResolvedVersion {
        channel: Channel::Commit,
        version: String::new(),
        hash: version.into(),
    })
}

fn resolve_latest(
    tags: &[String],
    default_branch: &str,
    head_commit: &str,
) -> Result<ResolvedVersion, SourceError> {
    let sorted = sort_semver_tags(tags);
    if let Some(tag) = sorted.first() {
        return Ok(ResolvedVersion {
            channel: Channel::Tag,
            version: tag.clone(),
            hash: String::new(),
        });
    }

    // Fall back to default branch HEAD
    Ok(ResolvedVersion {
        channel: Channel::Branch,
        version: default_branch.into(),
        hash: head_commit.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sort_semver_tags() {
        let tags = vec![
            "v1.0.0".into(),
            "v2.1.0".into(),
            "v1.5.0".into(),
            "v2.0.0".into(),
        ];
        let sorted = sort_semver_tags(&tags);
        assert_eq!(sorted, vec!["v2.1.0", "v2.0.0", "v1.5.0", "v1.0.0"]);
    }

    #[test]
    fn test_sort_semver_tags_without_v() {
        let tags = vec!["1.0.0".into(), "2.0.0".into()];
        let sorted = sort_semver_tags(&tags);
        assert_eq!(sorted, vec!["2.0.0", "1.0.0"]);
    }

    #[test]
    fn test_sort_semver_tags_mixed() {
        let tags = vec![
            "v1.0.0".into(),
            "nightly".into(),
            "v2.0.0".into(),
            "beta".into(),
        ];
        let sorted = sort_semver_tags(&tags);
        assert_eq!(sorted, vec!["v2.0.0", "v1.0.0"]);
    }

    #[test]
    fn test_resolve_latest_with_tags() {
        let tags = vec!["v1.0.0".into(), "v2.0.0".into(), "v1.5.0".into()];
        let result = resolve_version(&tags, &[], "main", "abc123", "").unwrap();
        assert_eq!(result.channel, Channel::Tag);
        assert_eq!(result.version, "v2.0.0");
    }

    #[test]
    fn test_resolve_latest_no_tags() {
        let result = resolve_version(&[], &["main".into()], "main", "abc123", "").unwrap();
        assert_eq!(result.channel, Channel::Branch);
        assert_eq!(result.version, "main");
    }

    #[test]
    fn test_resolve_explicit_tag() {
        let tags = vec!["v1.0.0".into(), "v2.0.0".into()];
        let result = resolve_version(&tags, &[], "main", "", "v1.0.0").unwrap();
        assert_eq!(result.channel, Channel::Tag);
        assert_eq!(result.version, "v1.0.0");
    }

    #[test]
    fn test_resolve_explicit_branch() {
        let branches = vec!["main".into(), "develop".into()];
        let result = resolve_version(&[], &branches, "main", "", "develop").unwrap();
        assert_eq!(result.channel, Channel::Branch);
        assert_eq!(result.version, "develop");
    }

    #[test]
    fn test_resolve_commit_sha() {
        let result = resolve_version(&[], &[], "main", "", "abc123def").unwrap();
        assert_eq!(result.channel, Channel::Commit);
        assert_eq!(result.hash, "abc123def");
    }
}
