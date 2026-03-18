use crate::radio_catalog::DisplayDef;

/// LCD backlight color for monochrome/grayscale displays (classic gray-green LCD).
const BACKLIGHT_R: u8 = 0xC9;
const BACKLIGHT_G: u8 = 0xCB;
const BACKLIGHT_B: u8 = 0xBA;

/// LCD pixel "ink" color (dark, near-black).
const INK_R: u8 = 0x1A;
const INK_G: u8 = 0x1E;
const INK_B: u8 = 0x16;

/// Convert a 1-bit column-major LCD framebuffer to RGBA.
/// Bit ON = dark pixel (ink), Bit OFF = backlight visible.
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
                rgba[px] = INK_R;
                rgba[px + 1] = INK_G;
                rgba[px + 2] = INK_B;
            } else {
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
/// Two rows share each byte: even y → low nibble, odd y → high nibble.
/// Byte index = (y / 2) * w + x.
/// 0 = darkest (ink), 15 = brightest (backlight).
pub fn decode_gray4(src: &[u8], w: usize, h: usize) -> Vec<u8> {
    let mut rgba = vec![0u8; w * h * 4];

    for y in 0..h {
        for x in 0..w {
            let byte_idx = (y / 2) * w + x;
            if byte_idx >= src.len() {
                continue;
            }

            let nibble = if y & 1 == 0 {
                src[byte_idx] & 0x0F
            } else {
                (src[byte_idx] >> 4) & 0x0F
            };

            // t=0 → backlight (lightest), t=255 → ink (darkest)
            let t = nibble as u16 * 17; // 0->0, 15->255

            let px = (y * w + x) * 4;
            rgba[px] = (BACKLIGHT_R as u16 - (BACKLIGHT_R as u16 - INK_R as u16) * t / 255) as u8;
            rgba[px + 1] =
                (BACKLIGHT_G as u16 - (BACKLIGHT_G as u16 - INK_G as u16) * t / 255) as u8;
            rgba[px + 2] =
                (BACKLIGHT_B as u16 - (BACKLIGHT_B as u16 - INK_B as u16) * t / 255) as u8;
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
        1 => d.w as usize * (d.h as usize).div_ceil(8),
        4 => d.w as usize * d.h as usize / 2,
        16 => d.w as usize * d.h as usize * 2,
        _ => d.w as usize * d.h as usize * 2,
    }
}

/// Dispatch to the correct decoder based on depth.
pub fn decode(src: &[u8], d: &DisplayDef) -> Vec<u8> {
    let w = d.w as usize;
    let h = d.h as usize;
    match d.depth {
        1 => decode_mono(src, w, h),
        4 => decode_gray4(src, w, h),
        16 => decode_rgb565(src, w, h),
        _ => decode_rgb565(src, w, h),
    }
}

const GRID_ALPHA: f32 = 0.35;

const GRID_R: f32 = 0x90 as f32;
const GRID_G: f32 = 0x92 as f32;
const GRID_B: f32 = 0x88 as f32;

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn build_grid_lut(scale: usize) -> Vec<f32> {
    let mut lut = vec![0.0f32; scale * scale];
    if scale <= 2 {
        // Hard 1-pixel grid lines on right and bottom edges of each cell
        for sy in 0..scale {
            for sx in 0..scale {
                if sx == scale - 1 || sy == scale - 1 {
                    lut[sy * scale + sx] = GRID_ALPHA;
                }
            }
        }
    } else {
        let s = scale as f32;
        for sy in 0..scale {
            for sx in 0..scale {
                let cx = (sx as f32 + 0.5) / s;
                let cy = (sy as f32 + 0.5) / s;
                let dx = 0.5 - (cx - 0.5).abs();
                let dy = 0.5 - (cy - 0.5).abs();
                let grid_h = 1.0 - smoothstep(0.0, 0.3, dx);
                let grid_v = 1.0 - smoothstep(0.0, 0.3, dy);
                let grid = grid_h.max(grid_v);
                lut[sy * scale + sx] = grid * GRID_ALPHA;
            }
        }
    }
    lut
}

/// Decode a BW/grayscale framebuffer and scale up with a dot-matrix LCD effect.
/// Returns (RGBA buffer, output width, output height).
pub fn decode_lcd(src: &[u8], d: &DisplayDef, scale: usize) -> (Vec<u8>, usize, usize) {
    let w = d.w as usize;
    let h = d.h as usize;
    let native = decode(src, d);

    if scale <= 1 {
        return (native, w, h);
    }

    let out_w = w * scale;
    let out_h = h * scale;
    let lut = build_grid_lut(scale);
    let mut out = vec![0u8; out_w * out_h * 4];

    for out_y in 0..out_h {
        let src_y = out_y / scale;
        let sub_y = out_y % scale;
        for out_x in 0..out_w {
            let src_x = out_x / scale;
            let sub_x = out_x % scale;

            let src_px = (src_y * w + src_x) * 4;
            let pr = native[src_px] as f32;
            let pg = native[src_px + 1] as f32;
            let pb = native[src_px + 2] as f32;

            let luminance = (0.299 * pr + 0.587 * pg + 0.114 * pb) / 255.0;
            let darkness = 1.0 - luminance;
            let blend = lut[sub_y * scale + sub_x] * (0.25 + 0.75 * darkness);
            let keep = 1.0 - blend;

            let dst_px = (out_y * out_w + out_x) * 4;
            out[dst_px] = (pr * keep + GRID_R * blend) as u8;
            out[dst_px + 1] = (pg * keep + GRID_G * blend) as u8;
            out[dst_px + 2] = (pb * keep + GRID_B * blend) as u8;
            out[dst_px + 3] = 0xFF;
        }
    }

    (out, out_w, out_h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_mono_empty() {
        let src = vec![0u8; 128 * 8]; // 128x64 mono
        let rgba = decode_mono(&src, 128, 64);
        assert_eq!(rgba.len(), 128 * 64 * 4);
        // All pixels OFF → backlight color with alpha 0xFF
        for i in (0..rgba.len()).step_by(4) {
            assert_eq!(rgba[i], BACKLIGHT_R);
            assert_eq!(rgba[i + 1], BACKLIGHT_G);
            assert_eq!(rgba[i + 2], BACKLIGHT_B);
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
        let mono = DisplayDef {
            w: 128,
            h: 64,
            depth: 1,
        };
        assert_eq!(lcd_buffer_size(&mono), 128 * 8);

        let gray = DisplayDef {
            w: 212,
            h: 64,
            depth: 4,
        };
        assert_eq!(lcd_buffer_size(&gray), 212 * 64 / 2);

        let color = DisplayDef {
            w: 480,
            h: 272,
            depth: 16,
        };
        assert_eq!(lcd_buffer_size(&color), 480 * 272 * 2);
    }
}
