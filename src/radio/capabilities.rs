use std::path::Path;

use crate::manifest::{DisplayCapabilities, RadioCapabilities};
use crate::radio_catalog;

use super::radioinfo;

/// Detect the radio's hardware capabilities from the SD card and catalog.
/// Returns None if the radio info or catalog lookup fails.
pub fn detect(sd_root: &Path) -> Option<RadioCapabilities> {
    let info = radioinfo::load_radio_info(sd_root).ok()??;
    let catalog = radio_catalog::fetch_catalog().ok()?;
    let radio = radio_catalog::find_radio(&catalog, &info.board).ok()?;

    Some(RadioCapabilities {
        display: DisplayCapabilities {
            width: radio.display.w as u32,
            height: radio.display.h as u32,
            color: radio.display.is_color(),
            touch: None, // TODO: add touch info to catalog
        },
    })
}
