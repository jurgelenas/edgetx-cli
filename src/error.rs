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
    #[error("{0}")]
    Other(String),
}

#[derive(Error, Debug)]
pub enum RadioError {
    #[error("no EdgeTX SD card detected under {0} -- make sure the radio is connected in USB Storage mode")]
    NoDevice(String),
    #[error("no EdgeTX SD card detected -- make sure the radio is connected in USB Storage mode")]
    NoDeviceWindows,
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
pub enum RegistryError {
    #[error("empty package reference")]
    EmptyRef,
    #[error("invalid package reference {raw:?}: {reason}")]
    InvalidRef { raw: String, reason: String },
    #[error("cloning {url}: {reason}")]
    Clone { url: String, reason: String },
    #[error("could not resolve version {version:?}: not a tag, branch, or commit")]
    UnresolvableVersion { version: String },
    #[error("repository does not contain a valid manifest: {0}")]
    NoManifest(String),
    #[error("{0}")]
    Other(String),
}

#[derive(Error, Debug)]
pub enum ScaffoldError {
    #[error("unknown script type {0:?} (valid types: {1})")]
    UnknownType(String, String),
    #[error("invalid name {0:?}: must match {1}")]
    InvalidName(String, String),
    #[error("name {name:?} is too long for {script_type} scripts (max {max} characters)")]
    NameTooLong {
        name: String,
        script_type: String,
        max: usize,
    },
    #[error("name {0:?} already exists in {1}")]
    Duplicate(String, String),
    #[error("unresolved dependencies: {0:?} (must reference libraries entries)")]
    UnresolvedDeps(Vec<String>),
    #[error("{0}")]
    Other(String),
}

#[derive(Error, Debug)]
pub enum SimulatorError {
    #[error("no radio found matching {0:?}")]
    RadioNotFound(String),
    #[error("ambiguous query {query:?} matches: {matches}")]
    AmbiguousRadio { query: String, matches: String },
    #[error("{0}")]
    Wasm(String),
    #[error("{0}")]
    Other(String),
}
