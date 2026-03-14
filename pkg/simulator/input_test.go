package simulator

import (
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/veandco/go-sdl2/sdl"
)

func TestKeyboardShortcuts_AllMapped(t *testing.T) {
	// Verify all shortcuts have unique SDL keys and valid key indices.
	seen := make(map[sdl.Keycode]bool)
	for _, ks := range KeyboardShortcuts {
		assert.False(t, seen[ks.SDLKey], "duplicate SDL key: %v for %s", ks.SDLKey, ks.Label)
		seen[ks.SDLKey] = true
		assert.GreaterOrEqual(t, ks.KeyIndex, 0)
		assert.LessOrEqual(t, ks.KeyIndex, 13)
	}
}

func TestKeyboardShortcuts_CompanionMapping(t *testing.T) {
	// Verify specific mappings match Companion's simulateduiwidget.cpp.
	tests := []struct {
		key      sdl.Keycode
		keyIndex int
		label    string
	}{
		{sdl.K_s, 13, "SYS"},
		{sdl.K_m, 11, "MODEL"},
		{sdl.K_t, 12, "TELE"},
		{sdl.K_RETURN, 2, "ENTER"},
		{sdl.K_ESCAPE, 1, "EXIT"},
		{sdl.K_UP, 5, "UP"},
		{sdl.K_DOWN, 6, "DOWN"},
		{sdl.K_LEFT, 7, "LEFT"},
		{sdl.K_RIGHT, 8, "RIGHT"},
		{sdl.K_PAGEUP, 3, "PAGE UP"},
		{sdl.K_PAGEDOWN, 4, "PAGE DN"},
	}

	keyMap := make(map[sdl.Keycode]KeyMapping)
	for _, ks := range KeyboardShortcuts {
		keyMap[ks.SDLKey] = ks
	}

	for _, tt := range tests {
		t.Run(tt.label, func(t *testing.T) {
			mapping, ok := keyMap[tt.key]
			assert.True(t, ok, "key %v should be mapped", tt.key)
			assert.Equal(t, tt.keyIndex, mapping.KeyIndex)
		})
	}
}

func TestNewInputMapper(t *testing.T) {
	// NewInputMapper should create a valid mapper (runtime can be nil for this test).
	// We can't fully test without a real runtime, but verify the keyMap is populated.
	mapper := NewInputMapper(nil)
	assert.NotNil(t, mapper)
	assert.Len(t, mapper.keyMap, len(KeyboardShortcuts))
}

func TestRotaryKeys(t *testing.T) {
	assert.Equal(t, sdl.Keycode(sdl.K_COMMA), RotaryUpKey)
	assert.Equal(t, sdl.Keycode(sdl.K_PERIOD), RotaryDownKey)
}
