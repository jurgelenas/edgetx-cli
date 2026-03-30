use crate::radio::RadioError;
use std::path::PathBuf;

/// Returns the base media directory for the current user.
pub fn default_media_dir() -> Result<PathBuf, RadioError> {
    #[cfg(target_os = "linux")]
    {
        let username = std::env::var("USER")
            .or_else(|_| std::env::var("LOGNAME"))
            .unwrap_or_else(|_| "nobody".to_string());
        Ok(PathBuf::from(format!("/media/{username}")))
    }

    #[cfg(target_os = "macos")]
    {
        Ok(PathBuf::from("/Volumes"))
    }

    #[cfg(target_os = "windows")]
    {
        Ok(PathBuf::new())
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        Err(RadioError::Detection(
            "unsupported platform for radio detection".into(),
        ))
    }
}
