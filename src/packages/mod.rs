pub mod file_list;
pub mod info;
pub mod install;
pub mod outdated;
pub mod path;
pub mod remove;
pub mod store;
mod transfer;
pub mod update;

use thiserror::Error;

pub use store::StoreError;

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
    #[error("no matching variant for this radio -- use --path to select a variant manually")]
    NoMatchingVariant,
    #[error("could not detect radio model -- use --path to select a variant manually")]
    UnknownRadio,
    #[error("resolving content path {path}: {source}")]
    ContentResolve {
        path: PackagePath,
        source: ManifestError,
    },
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Source(#[from] SourceError),
    #[error(transparent)]
    Radio(#[from] RadioError),
    #[error(transparent)]
    Copy(#[from] CopyError),
}
