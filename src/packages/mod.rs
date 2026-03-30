pub mod conflict;
pub mod install;
pub mod path;
pub mod remove;
pub mod state;
pub mod update;

use thiserror::Error;

pub use state::StateError;

use crate::manifest::ManifestError;
use crate::packages::path::PackagePath;
use crate::radio::RadioError;
use crate::radio::copy::CopyError;
use crate::source::SourceError;

#[derive(Error, Debug)]
pub enum PackageError {
    #[error("package {0:?} not found")]
    NotFound(String),
    #[error("ambiguous package name {name:?} matches multiple sources: {sources:?}")]
    Ambiguous { name: String, sources: Vec<String> },
    #[error("path conflicts:\n  {0}")]
    Conflicts(String),
    #[error("resolving content path {path}: {source}")]
    ContentResolve {
        path: PackagePath,
        source: ManifestError,
    },
    #[error(transparent)]
    State(#[from] StateError),
    #[error(transparent)]
    Source(#[from] SourceError),
    #[error(transparent)]
    Radio(#[from] RadioError),
    #[error(transparent)]
    Copy(#[from] CopyError),
}
