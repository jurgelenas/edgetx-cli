// Content item type helpers and source directory resolution utilities.

use super::ContentItem;

/// Returns the content items for a given YAML key.
pub fn items_for_key<'a>(
    manifest: &'a super::Manifest,
    yaml_key: &str,
) -> &'a Vec<ContentItem> {
    match yaml_key {
        "libraries" => &manifest.libraries,
        "tools" => &manifest.tools,
        "telemetry" => &manifest.telemetry,
        "functions" => &manifest.functions,
        "mixes" => &manifest.mixes,
        "widgets" => &manifest.widgets,
        "sounds" => &manifest.sounds,
        "images" => &manifest.images,
        "files" => &manifest.files,
        _ => &manifest.files, // fallback
    }
}
