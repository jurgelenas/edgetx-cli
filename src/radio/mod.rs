pub mod backup;
pub mod copy;
pub mod detect;
pub mod radioinfo;
pub mod version;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum RadioError {
    #[error(
        "no EdgeTX SD card detected under {0} -- make sure the radio is connected in USB Storage mode"
    )]
    NoDevice(String),
    #[error("{0}")]
    Detection(String),
    #[error("{0}")]
    Version(String),
    #[error("{step}: {source}")]
    EjectIo {
        step: &'static str,
        source: std::io::Error,
    },
    #[error("{step}: {stderr}")]
    EjectFailed { step: &'static str, stderr: String },
    #[error("{context}: {source}")]
    Io {
        context: String,
        source: std::io::Error,
    },
}
