package simulator

import (
	"testing"

	"github.com/stretchr/testify/assert"
)

func TestDecodeMono_SinglePixelSet(t *testing.T) {
	// 1x8 display, first byte = 0x01 (bit 0 set = pixel at y=0).
	src := []byte{0x01}
	rgba := DecodeMono(src, 1, 8)

	// Pixel at (0,0) should be backlight color.
	assert.Equal(t, backlightR, rgba[0])
	assert.Equal(t, backlightG, rgba[1])
	assert.Equal(t, backlightB, rgba[2])
	assert.Equal(t, uint8(0xFF), rgba[3])

	// Pixel at (0,1) should be black.
	assert.Equal(t, uint8(0), rgba[4])
	assert.Equal(t, uint8(0), rgba[5])
	assert.Equal(t, uint8(0), rgba[6])
	assert.Equal(t, uint8(0xFF), rgba[7])
}

func TestDecodeMono_AllSet(t *testing.T) {
	// 2x8 display, all bits set.
	src := []byte{0xFF, 0xFF}
	rgba := DecodeMono(src, 2, 8)

	assert.Len(t, rgba, 2*8*4)
	for y := range 8 {
		for x := range 2 {
			px := (y*2 + x) * 4
			assert.Equal(t, backlightR, rgba[px], "pixel (%d,%d) R", x, y)
			assert.Equal(t, backlightG, rgba[px+1], "pixel (%d,%d) G", x, y)
			assert.Equal(t, backlightB, rgba[px+2], "pixel (%d,%d) B", x, y)
		}
	}
}

func TestDecodeMono_Empty(t *testing.T) {
	rgba := DecodeMono(nil, 0, 0)
	assert.Empty(t, rgba)
}

func TestDecodeGray4_MaxIntensity(t *testing.T) {
	// 2x1 display, both pixels at max intensity (0xFF -> high=15, low=15).
	src := []byte{0xFF}
	rgba := DecodeGray4(src, 2, 1)

	assert.Len(t, rgba, 2*1*4)

	// Both pixels should be full backlight color.
	assert.Equal(t, backlightR, rgba[0])
	assert.Equal(t, backlightG, rgba[1])
	assert.Equal(t, backlightB, rgba[2])
	assert.Equal(t, uint8(0xFF), rgba[3])
}

func TestDecodeGray4_Zero(t *testing.T) {
	// 2x1 display, both pixels at zero intensity.
	src := []byte{0x00}
	rgba := DecodeGray4(src, 2, 1)

	// Both pixels should be black.
	assert.Equal(t, uint8(0), rgba[0])
	assert.Equal(t, uint8(0), rgba[1])
	assert.Equal(t, uint8(0), rgba[2])
	assert.Equal(t, uint8(0xFF), rgba[3])
}

func TestDecodeGray4_HalfIntensity(t *testing.T) {
	// 2x1 display, high nibble=8, low nibble=0.
	src := []byte{0x80}
	rgba := DecodeGray4(src, 2, 1)

	// First pixel (high nibble=8): intensity = 8*17 = 136.
	expectedR := uint8(uint16(backlightR) * 136 / 255)
	assert.Equal(t, expectedR, rgba[0])

	// Second pixel (low nibble=0): should be black.
	assert.Equal(t, uint8(0), rgba[4])
}

func TestDecodeRGB565_Red(t *testing.T) {
	// Pure red: R=31, G=0, B=0 -> 0xF800, little-endian: 0x00, 0xF8.
	src := []byte{0x00, 0xF8}
	rgba := DecodeRGB565(src, 1, 1)

	assert.Equal(t, uint8(0xF8), rgba[0]) // R: 31<<3 = 248
	assert.Equal(t, uint8(0), rgba[1])     // G
	assert.Equal(t, uint8(0), rgba[2])     // B
	assert.Equal(t, uint8(0xFF), rgba[3])  // A
}

func TestDecodeRGB565_Green(t *testing.T) {
	// Pure green: R=0, G=63, B=0 -> 0x07E0, little-endian: 0xE0, 0x07.
	src := []byte{0xE0, 0x07}
	rgba := DecodeRGB565(src, 1, 1)

	assert.Equal(t, uint8(0), rgba[0])     // R
	assert.Equal(t, uint8(0xFC), rgba[1])  // G: 63<<2 = 252
	assert.Equal(t, uint8(0), rgba[2])     // B
}

func TestDecodeRGB565_Blue(t *testing.T) {
	// Pure blue: R=0, G=0, B=31 -> 0x001F, little-endian: 0x1F, 0x00.
	src := []byte{0x1F, 0x00}
	rgba := DecodeRGB565(src, 1, 1)

	assert.Equal(t, uint8(0), rgba[0])     // R
	assert.Equal(t, uint8(0), rgba[1])     // G
	assert.Equal(t, uint8(0xF8), rgba[2])  // B: 31<<3 = 248
}

func TestDecodeRGB565_White(t *testing.T) {
	// White: 0xFFFF, little-endian: 0xFF, 0xFF.
	src := []byte{0xFF, 0xFF}
	rgba := DecodeRGB565(src, 1, 1)

	assert.Equal(t, uint8(0xF8), rgba[0])  // R
	assert.Equal(t, uint8(0xFC), rgba[1])  // G
	assert.Equal(t, uint8(0xF8), rgba[2])  // B
}

func TestDecodeRGB565_Empty(t *testing.T) {
	rgba := DecodeRGB565(nil, 0, 0)
	assert.Empty(t, rgba)
}

func TestLCDBufferSize(t *testing.T) {
	tests := []struct {
		display  DisplayDef
		expected int
	}{
		{DisplayDef{W: 128, H: 64, Depth: 1}, 128 * 8},
		{DisplayDef{W: 212, H: 64, Depth: 4}, 212 * 64 / 2},
		{DisplayDef{W: 480, H: 272, Depth: 16}, 480 * 272 * 2},
	}
	for _, tt := range tests {
		t.Run(
			func() string {
				return string(rune(tt.display.Depth)) + "bit"
			}(),
			func(t *testing.T) {
				assert.Equal(t, tt.expected, LCDBufferSize(tt.display))
			},
		)
	}
}

func TestDecodeFramebuffer_Dispatches(t *testing.T) {
	// Verify DecodeFramebuffer dispatches to correct decoder.
	d1 := DisplayDef{W: 1, H: 8, Depth: 1}
	result1 := DecodeFramebuffer([]byte{0xFF}, d1)
	assert.Len(t, result1, 1*8*4)

	d4 := DisplayDef{W: 2, H: 1, Depth: 4}
	result4 := DecodeFramebuffer([]byte{0xFF}, d4)
	assert.Len(t, result4, 2*1*4)

	d16 := DisplayDef{W: 1, H: 1, Depth: 16}
	result16 := DecodeFramebuffer([]byte{0x00, 0x00}, d16)
	assert.Len(t, result16, 1*1*4)
}
