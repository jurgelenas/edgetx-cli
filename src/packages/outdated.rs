use std::path::PathBuf;

use crate::source::version::Channel;
use crate::source::{PackageRef, resolve};

use super::PackageError;
use super::store::PackageStore;

/// OutdatedOptions configures an outdated check.
pub struct OutdatedOptions {
    pub sd_root: PathBuf,
}

/// Describes one package that has an update available.
pub struct OutdatedPackage {
    pub id: String,
    pub current_version: String,
    pub latest_version: String,
    pub channel: Channel,
}

/// Check installed packages for available updates.
pub fn check_outdated(opts: OutdatedOptions) -> Result<Vec<OutdatedPackage>, PackageError> {
    let store = PackageStore::load(opts.sd_root)?;
    let mut outdated = Vec::new();

    for pkg in store.packages() {
        if pkg.channel.is_pinned() || pkg.channel.is_local() {
            continue;
        }

        // Fetch from origin (fork) if set, else from id
        let fetch_str = pkg.origin.as_deref().unwrap_or(pkg.id.as_str());
        let pkg_ref: PackageRef = match fetch_str.parse() {
            Ok(r) => r,
            Err(_) => continue,
        };

        let result = match resolve::resolve_package(&pkg_ref) {
            Ok(r) => r,
            Err(e) => {
                log::warn!("failed to check {}: {e}", pkg.id);
                continue;
            }
        };

        if result.resolved.hash != pkg.commit {
            outdated.push(OutdatedPackage {
                id: pkg.id.clone(),
                current_version: pkg.version.clone(),
                latest_version: result.resolved.version,
                channel: pkg.channel,
            });
        }
    }

    Ok(outdated)
}
