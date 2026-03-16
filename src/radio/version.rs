use crate::error::RadioError;

/// Check that radio_version meets min_version. Returns Ok if compatible.
pub fn check_version_compatibility(
    radio_version: &str,
    min_version: &str,
) -> Result<(), RadioError> {
    if min_version.is_empty() {
        return Ok(());
    }

    let rv = normalize_version(radio_version);
    let mv = normalize_version(min_version);

    let rv_parsed = semver::Version::parse(&rv).map_err(|_| {
        RadioError::Other(format!("invalid radio firmware version {radio_version:?}"))
    })?;

    let mv_parsed = semver::Version::parse(&mv)
        .map_err(|_| RadioError::Other(format!("invalid minimum version {min_version:?}")))?;

    if rv_parsed < mv_parsed {
        return Err(RadioError::VersionMismatch {
            installed: radio_version.to_string(),
            required: min_version.to_string(),
        });
    }

    Ok(())
}

fn normalize_version(v: &str) -> String {
    v.strip_prefix('v').unwrap_or(v).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compatible() {
        assert!(check_version_compatibility("2.12.0", "2.11.0").is_ok());
    }

    #[test]
    fn test_exact_match() {
        assert!(check_version_compatibility("2.11.0", "2.11.0").is_ok());
    }

    #[test]
    fn test_incompatible() {
        let result = check_version_compatibility("2.10.0", "2.11.0");
        assert!(result.is_err());
    }

    #[test]
    fn test_with_v_prefix() {
        assert!(check_version_compatibility("v2.12.0", "v2.11.0").is_ok());
    }

    #[test]
    fn test_empty_min() {
        assert!(check_version_compatibility("2.12.0", "").is_ok());
    }
}
