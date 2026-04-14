use crate::manifest::{self, Manifest};
use crate::source::{PackageRef, resolve};

use super::PackageError;

/// InfoResult holds the resolved package information.
pub struct InfoResult {
    pub manifest: Manifest,
    pub version: String,
    pub repository_url: Option<String>,
}

/// Resolve a package and return its manifest and metadata.
pub fn fetch_info(pkg_ref: &PackageRef) -> Result<InfoResult, PackageError> {
    if let PackageRef::Local { path, sub_path } = pkg_ref {
        let (m, _) = manifest::load_with_sub_path(path, sub_path)
            .map_err(|e| PackageError::Source(e.into()))?;
        return Ok(InfoResult {
            manifest: m,
            version: String::new(),
            repository_url: None,
        });
    }

    let result = resolve::resolve_package(pkg_ref)?;
    Ok(InfoResult {
        manifest: result.manifest,
        version: result.resolved.version,
        repository_url: pkg_ref.clone_url(),
    })
}
