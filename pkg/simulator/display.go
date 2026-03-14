package simulator

// LCD backlight color for monochrome/grayscale displays (blue tint).
var backlightR, backlightG, backlightB uint8 = 0x2F, 0x7B, 0xE3

// DecodeMono converts a 1-bit column-major LCD framebuffer to RGBA.
// Layout: byte at (y/8)*w + x, bit y&7. Set pixel = backlight color, unset = black.
func DecodeMono(src []byte, w, h int) []byte {
	rgba := make([]byte, w*h*4)
	cols := (h + 7) / 8

	for x := 0; x < w; x++ {
		for y := 0; y < h; y++ {
			byteIdx := (y/8)*w + x
			if byteIdx >= len(src) {
				continue
			}
			bit := src[byteIdx] & (1 << uint(y&7))
			px := (y*w + x) * 4
			if bit != 0 {
				rgba[px] = backlightR
				rgba[px+1] = backlightG
				rgba[px+2] = backlightB
			}
			// else black (0,0,0)
			rgba[px+3] = 0xFF
		}
	}
	_ = cols
	return rgba
}

// DecodeGray4 converts a 4-bit grayscale framebuffer to RGBA.
// High nibble first. Intensity interpolates between black and backlight color.
func DecodeGray4(src []byte, w, h int) []byte {
	rgba := make([]byte, w*h*4)

	for y := 0; y < h; y++ {
		for x := 0; x < w; x++ {
			pixIdx := y*w + x
			byteIdx := pixIdx / 2
			if byteIdx >= len(src) {
				continue
			}

			var nibble uint8
			if pixIdx%2 == 0 {
				nibble = (src[byteIdx] >> 4) & 0x0F
			} else {
				nibble = src[byteIdx] & 0x0F
			}

			// Scale 0-15 to 0-255 for interpolation.
			intensity := nibble * 17 // 0->0, 15->255

			px := (y*w + x) * 4
			rgba[px] = uint8(uint16(backlightR) * uint16(intensity) / 255)
			rgba[px+1] = uint8(uint16(backlightG) * uint16(intensity) / 255)
			rgba[px+2] = uint8(uint16(backlightB) * uint16(intensity) / 255)
			rgba[px+3] = 0xFF
		}
	}
	return rgba
}

// DecodeRGB565 converts a 16-bit RGB565 little-endian framebuffer to RGBA.
func DecodeRGB565(src []byte, w, h int) []byte {
	rgba := make([]byte, w*h*4)

	for y := 0; y < h; y++ {
		for x := 0; x < w; x++ {
			byteIdx := (y*w + x) * 2
			if byteIdx+1 >= len(src) {
				continue
			}

			v := uint16(src[byteIdx]) | uint16(src[byteIdx+1])<<8

			r := uint8((v >> 11) << 3)
			g := uint8(((v >> 5) & 0x3F) << 2)
			b := uint8((v & 0x1F) << 3)

			px := (y*w + x) * 4
			rgba[px] = r
			rgba[px+1] = g
			rgba[px+2] = b
			rgba[px+3] = 0xFF
		}
	}
	return rgba
}

// LCDBufferSize returns the expected source buffer size for the given display.
func LCDBufferSize(d DisplayDef) int {
	switch d.Depth {
	case 1:
		return d.W * ((d.H + 7) / 8)
	case 4:
		return d.W * d.H / 2
	case 16:
		return d.W * d.H * 2
	default:
		return d.W * d.H * 2
	}
}

// DecodeFramebuffer dispatches to the correct decoder based on depth.
func DecodeFramebuffer(src []byte, d DisplayDef) []byte {
	switch d.Depth {
	case 1:
		return DecodeMono(src, d.W, d.H)
	case 4:
		return DecodeGray4(src, d.W, d.H)
	case 16:
		return DecodeRGB565(src, d.W, d.H)
	default:
		return DecodeRGB565(src, d.W, d.H)
	}
}
