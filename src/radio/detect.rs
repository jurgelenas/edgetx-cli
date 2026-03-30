use super::RadioError;
use std::path::PathBuf;
use std::time::Duration;

/// Scan for a mounted EdgeTX SD card by looking for edgetx.sdcard.version.
pub fn detect_mount(media_dir: &str) -> Result<PathBuf, RadioError> {
    #[cfg(target_os = "windows")]
    {
        if media_dir.is_empty() {
            return detect_windows_drives();
        }
    }

    let entries = std::fs::read_dir(media_dir).map_err(|e| RadioError::Io {
        context: format!("scanning {media_dir}"),
        source: e,
    })?;

    let mut candidates = Vec::new();
    for entry in entries.flatten() {
        if !entry.path().is_dir() {
            continue;
        }
        let mount_point = entry.path();
        let version_file = mount_point.join("edgetx.sdcard.version");
        if version_file.exists() {
            candidates.push(mount_point);
        }
    }

    match candidates.len() {
        0 => Err(RadioError::NoDevice(media_dir.to_string())),
        1 => Ok(candidates.into_iter().next().unwrap()),
        _ => {
            let names: Vec<String> = candidates.iter().map(|c| c.display().to_string()).collect();
            Err(RadioError::Detection(format!(
                "multiple EdgeTX SD cards detected: {} -- disconnect extra devices",
                names.join(", ")
            )))
        }
    }
}

#[cfg(target_os = "windows")]
fn detect_windows_drives() -> Result<PathBuf, RadioError> {
    let mut candidates = Vec::new();
    for letter in b'D'..=b'Z' {
        let drive = format!("{}:\\", letter as char);
        let path = PathBuf::from(&drive);
        if path.is_dir() {
            let version_file = path.join("edgetx.sdcard.version");
            if version_file.exists() {
                candidates.push(path);
            }
        }
    }

    match candidates.len() {
        0 => Err(RadioError::NoDevice("Windows drives".to_string())),
        1 => Ok(candidates.into_iter().next().unwrap()),
        _ => {
            let names: Vec<String> = candidates.iter().map(|c| c.display().to_string()).collect();
            Err(RadioError::Detection(format!(
                "multiple EdgeTX SD cards detected: {} -- disconnect extra devices",
                names.join(", ")
            )))
        }
    }
}

fn is_no_device_error(err: &RadioError) -> bool {
    matches!(err, RadioError::NoDevice(_))
}

/// Poll DetectMount until a device is found or the timeout expires.
/// Non-retryable errors (e.g. multiple devices) are returned immediately.
pub fn wait_for_mount(media_dir: &str, timeout: Duration) -> Result<PathBuf, RadioError> {
    let poll_interval = Duration::from_millis(500);
    let deadline = std::time::Instant::now() + timeout;

    loop {
        match detect_mount(media_dir) {
            Ok(mount) => return Ok(mount),
            Err(e) => {
                if !is_no_device_error(&e) {
                    return Err(e);
                }
                if std::time::Instant::now() > deadline {
                    return Err(e);
                }
                std::thread::sleep(poll_interval);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_detect_mount_single() {
        let dir = TempDir::new().unwrap();
        let radio = dir.path().join("my-radio");
        std::fs::create_dir(&radio).unwrap();
        std::fs::write(radio.join("edgetx.sdcard.version"), "2.10.0").unwrap();

        let result = detect_mount(dir.path().to_str().unwrap()).unwrap();
        assert_eq!(result, radio);
    }

    #[test]
    fn test_detect_mount_none() {
        let dir = TempDir::new().unwrap();
        let result = detect_mount(dir.path().to_str().unwrap());
        assert!(result.is_err());
    }

    #[test]
    fn test_detect_mount_multiple() {
        let dir = TempDir::new().unwrap();
        for name in &["radio1", "radio2"] {
            let radio = dir.path().join(name);
            std::fs::create_dir(&radio).unwrap();
            std::fs::write(radio.join("edgetx.sdcard.version"), "2.10.0").unwrap();
        }

        let result = detect_mount(dir.path().to_str().unwrap());
        assert!(matches!(result, Err(RadioError::Detection(_))));
    }
}
