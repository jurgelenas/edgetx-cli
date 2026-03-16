use anyhow::{Context, Result};

/// Save an RGBA framebuffer as a PNG screenshot.
pub fn save_screenshot(path: &str, rgba: &[u8], w: u32, h: u32) -> Result<()> {
    let img = image::RgbaImage::from_raw(w, h, rgba.to_vec())
        .context("creating image from framebuffer")?;
    img.save(path)
        .with_context(|| format!("saving screenshot to {path}"))?;
    Ok(())
}
