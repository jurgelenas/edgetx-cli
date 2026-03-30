use serde::Deserialize;
use std::path::Path;

use super::RadioError;

/// RadioInfo holds metadata read from RADIO/radio.yml on the SD card.
#[derive(Debug, Deserialize)]
pub struct RadioInfo {
    pub semver: String,
    #[allow(dead_code)]
    pub board: String,
}

/// Load RADIO/radio.yml from the given SD card root.
/// Returns None if the file does not exist.
pub fn load_radio_info(sd_root: &Path) -> Result<Option<RadioInfo>, RadioError> {
    let path = sd_root.join("RADIO").join("radio.yml");

    match std::fs::read_to_string(&path) {
        Ok(data) => {
            let info: RadioInfo = serde_yml::from_str(&data).map_err(|e| RadioError::Io {
                context: format!("parsing {}", path.display()),
                source: std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()),
            })?;
            Ok(Some(info))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(RadioError::Io {
            context: format!("reading {}", path.display()),
            source: e,
        }),
    }
}
