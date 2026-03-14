package simulator

import (
	"github.com/veandco/go-sdl2/sdl"
)

// KeyMapping maps an SDL keycode to a simulator key index.
type KeyMapping struct {
	SDLKey   sdl.Keycode
	KeyIndex int
	Label    string
}

// Keyboard shortcuts matching Companion's simulateduiwidget.cpp.
var KeyboardShortcuts = []KeyMapping{
	{sdl.K_s, 13, "SYS"},
	{sdl.K_m, 11, "MODEL"},
	{sdl.K_t, 12, "TELE"},
	{sdl.K_PAGEUP, 3, "PAGE UP"},
	{sdl.K_PAGEDOWN, 4, "PAGE DN"},
	{sdl.K_UP, 5, "UP"},
	{sdl.K_DOWN, 6, "DOWN"},
	{sdl.K_LEFT, 7, "LEFT"},
	{sdl.K_RIGHT, 8, "RIGHT"},
	{sdl.K_PLUS, 9, "PLUS"},
	{sdl.K_MINUS, 10, "MINUS"},
	{sdl.K_RETURN, 2, "ENTER"},
	{sdl.K_ESCAPE, 1, "EXIT"},
	{sdl.K_EQUALS, 0, "MENU"},
}

// RotaryKeys maps SDL keycodes to rotary encoder direction.
var RotaryUpKey sdl.Keycode = sdl.K_COMMA
var RotaryDownKey sdl.Keycode = sdl.K_PERIOD

// InputMapper handles keyboard/mouse -> simulator input translation.
type InputMapper struct {
	runtime *Runtime
	keyMap  map[sdl.Keycode]int
}

// NewInputMapper creates an input mapper connected to a runtime.
func NewInputMapper(rt *Runtime) *InputMapper {
	km := make(map[sdl.Keycode]int, len(KeyboardShortcuts))
	for _, ks := range KeyboardShortcuts {
		km[ks.SDLKey] = ks.KeyIndex
	}
	return &InputMapper{
		runtime: rt,
		keyMap:  km,
	}
}

// HandleKeyEvent processes an SDL keyboard event.
func (im *InputMapper) HandleKeyEvent(event *sdl.KeyboardEvent) {
	pressed := event.Type == sdl.KEYDOWN

	// Rotary encoder keys.
	if event.Type == sdl.KEYDOWN {
		switch sdl.Keycode(event.Keysym.Sym) {
		case RotaryUpKey:
			im.runtime.RotaryEncoder(1)
			return
		case RotaryDownKey:
			im.runtime.RotaryEncoder(-1)
			return
		}
	}

	if idx, ok := im.keyMap[sdl.Keycode(event.Keysym.Sym)]; ok {
		im.runtime.SetKey(idx, pressed)
	}
}

// HandleMouseWheel processes mouse wheel events for rotary encoder.
func (im *InputMapper) HandleMouseWheel(event *sdl.MouseWheelEvent) {
	if event.Y != 0 {
		im.runtime.RotaryEncoder(int(event.Y))
	}
}

// HandleLCDTouch processes mouse clicks within the LCD area.
func (im *InputMapper) HandleLCDTouch(mouseDown bool, lcdX, lcdY int) {
	if mouseDown {
		im.runtime.TouchDown(lcdX, lcdY)
	} else {
		im.runtime.TouchUp()
	}
}

// PrintKeyboardShortcuts returns a formatted string of keyboard shortcuts.
func PrintKeyboardShortcuts() string {
	lines := "Keyboard shortcuts:\n"
	for _, ks := range KeyboardShortcuts {
		lines += "  " + sdl.GetKeyName(ks.SDLKey) + " → " + ks.Label + "\n"
	}
	lines += "  , → Rotary encoder up\n"
	lines += "  . → Rotary encoder down\n"
	lines += "  Mouse wheel → Rotary encoder\n"
	lines += "  Mouse click on LCD → Touch\n"
	return lines
}
