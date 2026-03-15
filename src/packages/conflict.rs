use crate::error::PackageError;

use super::state::State;

/// Check if any of new_paths overlap with paths owned by already-installed packages.
/// skip_source is excluded from checks (used during update to skip the package being updated).
///
/// Overlap is determined by segment-based prefix matching (split on "/") to
/// avoid false positives like "SCRIPTS/TOOLS" vs "SCRIPTS/TOOLSET".
pub fn check_conflicts(
    state: &State,
    new_paths: &[String],
    skip_source: &str,
) -> Result<(), PackageError> {
    let mut installed: std::collections::HashMap<&str, &str> = std::collections::HashMap::new();
    for pkg in &state.packages {
        if pkg.source == skip_source {
            continue;
        }
        for p in &pkg.paths {
            installed.insert(p.as_str(), pkg.source.as_str());
        }
    }

    let mut conflicts = Vec::new();
    for np in new_paths {
        let np_segs: Vec<&str> = split_path(np);
        for (ip, owner) in &installed {
            let ip_segs: Vec<&str> = split_path(ip);
            if segment_prefix_match(&np_segs, &ip_segs) {
                conflicts.push(format!(
                    "{np:?} conflicts with {ip:?} (owned by {owner})"
                ));
            }
        }
    }

    if !conflicts.is_empty() {
        return Err(PackageError::Conflicts(conflicts.join("\n  ")));
    }

    Ok(())
}

fn split_path(p: &str) -> Vec<&str> {
    p.trim_end_matches('/').split('/').collect()
}

/// Returns true if a is a prefix of b, b is a prefix of a, or they are equal
/// — all at segment boundaries.
fn segment_prefix_match(a: &[&str], b: &[&str]) -> bool {
    let shorter = a.len().min(b.len());
    for i in 0..shorter {
        if a[i] != b[i] {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packages::state::{InstalledPackage, State};

    fn make_state(packages: Vec<(&str, Vec<&str>)>) -> State {
        let mut s = State::default();
        for (source, paths) in packages {
            s.add(InstalledPackage {
                source: source.into(),
                name: source.into(),
                channel: "tag".into(),
                version: String::new(),
                commit: String::new(),
                paths: paths.into_iter().map(String::from).collect(),
                dev: false,
            });
        }
        s
    }

    #[test]
    fn test_no_conflicts() {
        let state = make_state(vec![("pkg-a", vec!["SCRIPTS/TOOLS/A"])]);
        let result = check_conflicts(&state, &["SCRIPTS/TOOLS/B".into()], "");
        assert!(result.is_ok());
    }

    #[test]
    fn test_exact_match_conflict() {
        let state = make_state(vec![("pkg-a", vec!["SCRIPTS/TOOLS/A"])]);
        let result = check_conflicts(&state, &["SCRIPTS/TOOLS/A".into()], "");
        assert!(result.is_err());
    }

    #[test]
    fn test_prefix_overlap() {
        let state = make_state(vec![("pkg-a", vec!["SCRIPTS/TOOLS"])]);
        let result = check_conflicts(&state, &["SCRIPTS/TOOLS/B".into()], "");
        assert!(result.is_err());
    }

    #[test]
    fn test_no_false_positive() {
        // "SCRIPTS/TOOLS" and "SCRIPTS/TOOLSET" should NOT conflict
        let state = make_state(vec![("pkg-a", vec!["SCRIPTS/TOOLS"])]);
        let result = check_conflicts(&state, &["SCRIPTS/TOOLSET".into()], "");
        assert!(result.is_ok());
    }

    #[test]
    fn test_skip_source() {
        let state = make_state(vec![("pkg-a", vec!["SCRIPTS/TOOLS/A"])]);
        let result = check_conflicts(&state, &["SCRIPTS/TOOLS/A".into()], "pkg-a");
        assert!(result.is_ok());
    }

    #[test]
    fn test_multiple_conflicts() {
        let state = make_state(vec![
            ("pkg-a", vec!["SCRIPTS/TOOLS/A"]),
            ("pkg-b", vec!["WIDGETS/B"]),
        ]);
        let result = check_conflicts(
            &state,
            &["SCRIPTS/TOOLS/A".into(), "WIDGETS/B".into()],
            "",
        );
        assert!(result.is_err());
    }
}
