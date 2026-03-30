use std::path::Path;

use super::SimulatorError;

/// Save an RGBA framebuffer as a PNG screenshot.
pub fn save_screenshot(path: &Path, rgba: &[u8], w: u32, h: u32) -> Result<(), SimulatorError> {
    let img = image::RgbaImage::from_raw(w, h, rgba.to_vec())
        .ok_or_else(|| SimulatorError::Runtime("creating image from framebuffer".into()))?;
    img.save(path).map_err(|e| {
        SimulatorError::Runtime(format!("saving screenshot to {}: {e}", path.display()))
    })?;
    Ok(())
}
