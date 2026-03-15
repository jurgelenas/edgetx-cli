use super::radios::DisplayDef;

/// LCD backlight color for monochrome/grayscale displays (blue tint).
const BACKLIGHT_R: u8 = 0x2F;
const BACKLIGHT_G: u8 = 0x7B;
const BACKLIGHT_B: u8 = 0xE3;

/// Convert a 1-bit column-major LCD framebuffer to RGBA.
pub fn decode_mono(src: &[u8], w: usize, h: usize) -> Vec<u8> {
    let mut rgba = vec![0u8; w * h * 4];

    for x in 0..w {
        for y in 0..h {
            let byte_idx = (y / 8) * w + x;
            if byte_idx >= src.len() {
                continue;
            }
            let bit = src[byte_idx] & (1 << (y & 7));
            let px = (y * w + x) * 4;
            if bit != 0 {
                rgba[px] = BACKLIGHT_R;
                rgba[px + 1] = BACKLIGHT_G;
                rgba[px + 2] = BACKLIGHT_B;
            }
            rgba[px + 3] = 0xFF;
        }
    }

    rgba
}

/// Convert a 4-bit grayscale framebuffer to RGBA.
pub fn decode_gray4(src: &[u8], w: usize, h: usize) -> Vec<u8> {
    let mut rgba = vec![0u8; w * h * 4];

    for y in 0..h {
        for x in 0..w {
            let pix_idx = y * w + x;
            let byte_idx = pix_idx / 2;
            if byte_idx >= src.len() {
                continue;
            }

            let nibble = if pix_idx % 2 == 0 {
                (src[byte_idx] >> 4) & 0x0F
            } else {
                src[byte_idx] & 0x0F
            };

            let intensity = nibble * 17; // 0->0, 15->255

            let px = (y * w + x) * 4;
            rgba[px] = (BACKLIGHT_R as u16 * intensity as u16 / 255) as u8;
            rgba[px + 1] = (BACKLIGHT_G as u16 * intensity as u16 / 255) as u8;
            rgba[px + 2] = (BACKLIGHT_B as u16 * intensity as u16 / 255) as u8;
            rgba[px + 3] = 0xFF;
        }
    }

    rgba
}

/// Convert a 16-bit RGB565 little-endian framebuffer to RGBA.
pub fn decode_rgb565(src: &[u8], w: usize, h: usize) -> Vec<u8> {
    let mut rgba = vec![0u8; w * h * 4];

    for y in 0..h {
        for x in 0..w {
            let byte_idx = (y * w + x) * 2;
            if byte_idx + 1 >= src.len() {
                continue;
            }

            let v = src[byte_idx] as u16 | (src[byte_idx + 1] as u16) << 8;

            let r = ((v >> 11) << 3) as u8;
            let g = (((v >> 5) & 0x3F) << 2) as u8;
            let b = ((v & 0x1F) << 3) as u8;

            let px = (y * w + x) * 4;
            rgba[px] = r;
            rgba[px + 1] = g;
            rgba[px + 2] = b;
            rgba[px + 3] = 0xFF;
        }
    }

    rgba
}

/// Expected source buffer size for the given display.
pub fn lcd_buffer_size(d: &DisplayDef) -> usize {
    match d.depth {
        1 => d.w as usize * ((d.h as usize + 7) / 8),
        4 => d.w as usize * d.h as usize / 2,
        16 => d.w as usize * d.h as usize * 2,
        _ => d.w as usize * d.h as usize * 2,
    }
}

/// Dispatch to the correct decoder based on depth.
pub fn decode_framebuffer(src: &[u8], d: &DisplayDef) -> Vec<u8> {
    let w = d.w as usize;
    let h = d.h as usize;
    match d.depth {
        1 => decode_mono(src, w, h),
        4 => decode_gray4(src, w, h),
        16 => decode_rgb565(src, w, h),
        _ => decode_rgb565(src, w, h),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_mono_empty() {
        let src = vec![0u8; 128 * 8]; // 128x64 mono
        let rgba = decode_mono(&src, 128, 64);
        assert_eq!(rgba.len(), 128 * 64 * 4);
        // All pixels should be black (0,0,0) with alpha 0xFF
        for i in (0..rgba.len()).step_by(4) {
            assert_eq!(rgba[i], 0);
            assert_eq!(rgba[i + 1], 0);
            assert_eq!(rgba[i + 2], 0);
            assert_eq!(rgba[i + 3], 0xFF);
        }
    }

    #[test]
    fn test_decode_rgb565() {
        // White pixel: R=31, G=63, B=31 -> 0xFFFF
        let src = vec![0xFF, 0xFF];
        let rgba = decode_rgb565(&src, 1, 1);
        assert_eq!(rgba[0], 0xF8); // R = 31 << 3
        assert_eq!(rgba[1], 0xFC); // G = 63 << 2
        assert_eq!(rgba[2], 0xF8); // B = 31 << 3
        assert_eq!(rgba[3], 0xFF);
    }

    #[test]
    fn test_lcd_buffer_size() {
        let mono = DisplayDef { w: 128, h: 64, depth: 1 };
        assert_eq!(lcd_buffer_size(&mono), 128 * 8);

        let gray = DisplayDef { w: 212, h: 64, depth: 4 };
        assert_eq!(lcd_buffer_size(&gray), 212 * 64 / 2);

        let color = DisplayDef { w: 480, h: 272, depth: 16 };
        assert_eq!(lcd_buffer_size(&color), 480 * 272 * 2);
    }
}
