use crate::radio::RadioError;

/// Returns the base media directory for the current user.
pub fn default_media_dir() -> Result<String, RadioError> {
    #[cfg(target_os = "linux")]
    {
        let username = std::env::var("USER")
            .or_else(|_| std::env::var("LOGNAME"))
            .unwrap_or_else(|_| "nobody".to_string());
        Ok(format!("/media/{username}"))
    }

    #[cfg(target_os = "macos")]
    {
        Ok("/Volumes".to_string())
    }

    #[cfg(target_os = "windows")]
    {
        Ok(String::new())
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        Err(RadioError::Detection(
            "unsupported platform for radio detection".into(),
        ))
    }
}
