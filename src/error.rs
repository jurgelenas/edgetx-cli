use thiserror::Error;

#[derive(Error, Debug)]
pub enum ManifestError {
    #[error("reading manifest {path}: {source}")]
    Read {
        path: String,
        source: std::io::Error,
    },
    #[error("parsing manifest {path}: {source}")]
    Parse {
        path: String,
        source: serde_yml::Error,
    },
    #[error("invalid manifest {path}: {message}")]
    Validation { path: String, message: String },
    #[error("{0}")]
    Other(String),
}

#[derive(Error, Debug)]
pub enum PackageError {
    #[error("package {0:?} not found")]
    NotFound(String),
    #[error("ambiguous package name {name:?} matches multiple sources: {sources:?}")]
    Ambiguous { name: String, sources: Vec<String> },
    #[error("path conflicts:\n  {0}")]
    Conflicts(String),
    #[error("{0}")]
    State(String),
}

#[derive(Error, Debug)]
pub enum RadioError {
    #[error(
        "no EdgeTX SD card detected under {0} -- make sure the radio is connected in USB Storage mode"
    )]
    NoDevice(String),
    #[error("multiple EdgeTX SD cards detected: {0} -- disconnect extra devices")]
    MultipleDevices(String),
    #[error("radio firmware version {installed} does not meet minimum required version {required}")]
    VersionMismatch { installed: String, required: String },
    #[error("{0}")]
    Eject(String),
    #[error("{0}")]
    Other(String),
}

#[derive(Error, Debug)]
pub enum SourceError {
    #[error("empty package reference")]
    EmptyRef,
    #[error("invalid package reference {raw:?}: {reason}")]
    InvalidRef { raw: String, reason: String },
    #[error("cloning {url}: {reason}")]
    Clone { url: String, reason: String },
    #[error("repository does not contain a valid manifest: {0}")]
    NoManifest(String),
    #[error("{0}")]
    Other(String),
}
