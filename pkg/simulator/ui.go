package simulator

/*
#cgo pkg-config: sdl2
#include <SDL.h>
*/
import "C"

import (
	"fmt"
	"image"
	"strings"

	"github.com/AllenDang/cimgui-go/backend"
	"github.com/AllenDang/cimgui-go/backend/sdlbackend"
	"github.com/AllenDang/cimgui-go/imgui"
)

// inputCmd represents a queued input action for the WASM runtime.
type inputCmd struct {
	kind    string // "key", "switch", "trim", "touch", "touchup", "rotary", "analog"
	index   int
	state   int
	x, y    int
	value   uint16
	pressed bool
}

// UI manages the ImGui-based simulator window.
type UI struct {
	radio   *RadioDef
	backend *sdlbackend.SDLBackend

	// LCD state.
	lcdTexture    imgui.TextureRef
	lcdHasTexture bool
	lcdScale      float32
	lcdRGBA       *image.RGBA

	// Control state.
	switchStates map[string]int32
	potValues    map[string]int32
	lcdHovered   bool

	// Input command queue — UI enqueues, WASM goroutine processes.
	inputCh chan inputCmd

	// Trace output.
	traceLines []string
}

// UIOptions configures the UI.
type UIOptions struct {
	Radio   *RadioDef
	InputCh chan inputCmd
}

// NewUI creates the simulator UI.
func NewUI(opts UIOptions) *UI {
	ui := &UI{
		radio:        opts.Radio,
		lcdScale:     1.0,
		switchStates: make(map[string]int32),
		potValues:    make(map[string]int32),
		inputCh:      opts.InputCh,
	}

	// Initialize switch states from defaults.
	for _, sw := range ui.radio.Switches {
		if sw.Default == "NONE" {
			continue
		}
		switch sw.Type {
		case "2POS":
			ui.switchStates[sw.Name] = 0
		case "3POS":
			ui.switchStates[sw.Name] = 1 // center
		}
	}

	// Initialize pot values from defaults.
	for _, inp := range ui.radio.Inputs {
		if inp.Type != "FLEX" {
			continue
		}
		switch inp.Default {
		case "POT", "POT_CENTER", "SLIDER":
			ui.potValues[inp.Name] = 2048
		case "MULTIPOS":
			ui.potValues[inp.Name] = 0
		}
	}

	return ui
}

// Init creates the SDL window and ImGui context. Call before Run().
func (ui *UI) Init() error {
	// Prevent SDL from capturing SIGINT so Go's signal handling works.
	hint := C.CString("SDL_NO_SIGNAL_HANDLERS")
	val := C.CString("1")
	C.SDL_SetHint(hint, val)

	sdlBackend := sdlbackend.NewSDLBackend()
	// CreateBackend sets the global currentBackend used by C→Go callback dispatch.
	backend.CreateBackend(sdlBackend)
	ui.backend = sdlBackend

	d := ui.radio.Display
	winW := int(float32(d.W)*ui.lcdScale) + 200
	winH := int(float32(d.H)*ui.lcdScale) + 350

	title := fmt.Sprintf("EdgeTX Simulator - %s", ui.radio.Name)
	ui.backend.CreateWindow(title, winW, winH)

	// Disable multi-viewport so ImGui renders inside the SDL window, not as a separate OS window.
	// Must be done after CreateWindow since the C++ code sets ViewportsEnable during window creation.
	io := imgui.CurrentIO()
	io.SetConfigFlags(io.ConfigFlags() &^ imgui.ConfigFlagsViewportsEnable)
	ui.backend.SetTargetFPS(60)

	// Set background color based on display type.
	if d.Depth == 16 {
		ui.backend.SetBgColor(imgui.Vec4{X: 0.1, Y: 0.1, Z: 0.1, W: 1.0})
	} else {
		ui.backend.SetBgColor(imgui.Vec4{X: 0.15, Y: 0.15, Z: 0.2, W: 1.0})
	}

	// Create initial LCD texture (black).
	ui.lcdRGBA = image.NewRGBA(image.Rect(0, 0, d.W, d.H))
	ui.lcdTexture = ui.backend.CreateTextureRgba(ui.lcdRGBA, d.W, d.H)
	ui.lcdHasTexture = true

	return nil
}

// UpdateLCD updates the LCD texture with new framebuffer data.
// Called before renderFrame so the texture is ready when ImGui renders it.
func (ui *UI) UpdateLCD(rawLCD []byte) {
	if ui.backend == nil {
		return
	}

	d := ui.radio.Display
	rgba := DecodeFramebuffer(rawLCD, d)

	if ui.lcdRGBA == nil {
		ui.lcdRGBA = image.NewRGBA(image.Rect(0, 0, d.W, d.H))
	}
	copy(ui.lcdRGBA.Pix, rgba)

	if ui.lcdHasTexture {
		updateTexture(ui.lcdTexture, d.W, d.H, ui.lcdRGBA.Pix)
	}
}

// AddTraceLine appends a line to the trace output.
func (ui *UI) AddTraceLine(line string) {
	ui.traceLines = append(ui.traceLines, line)
	if len(ui.traceLines) > 200 {
		ui.traceLines = ui.traceLines[len(ui.traceLines)-200:]
	}
}

// Run starts the ImGui render loop. Blocks until window is closed.
// onFrame is called before renderFrame so LCD data is ready for rendering.
func (ui *UI) Run(onFrame func()) {
	ui.backend.Run(func() {
		if onFrame != nil {
			onFrame()
		}
		ui.renderFrame()
	})
}

// SetShouldClose signals the window to close.
func (ui *UI) SetShouldClose() {
	if ui.backend != nil {
		ui.backend.SetShouldClose(true)
	}
}

// Close releases UI resources.
func (ui *UI) Close() {
	if ui.backend != nil && ui.lcdHasTexture {
		ui.backend.DeleteTexture(ui.lcdTexture)
		ui.lcdHasTexture = false
	}
}

func (ui *UI) renderFrame() {
	ui.pollKeyboard()

	isLandscape := ui.radio.Display.H <= ui.radio.Display.W

	imgui.SetNextWindowPosV(imgui.Vec2{X: 0, Y: 0}, imgui.CondAlways, imgui.Vec2{})
	w, h := ui.backend.DisplaySize()
	imgui.SetNextWindowSizeV(imgui.Vec2{X: float32(w), Y: float32(h)}, imgui.CondAlways)

	flags := imgui.WindowFlagsNoMove | imgui.WindowFlagsNoResize |
		imgui.WindowFlagsNoCollapse | imgui.WindowFlagsNoTitleBar
	imgui.BeginV("Simulator", nil, flags)

	if isLandscape {
		ui.renderLandscapeLayout()
	} else {
		ui.renderPortraitLayout()
	}

	// Trace output section.
	imgui.Separator()
	if imgui.CollapsingHeaderTreeNodeFlagsV("Trace Output", imgui.TreeNodeFlagsDefaultOpen) {
		imgui.BeginChildStrV("trace", imgui.Vec2{X: 0, Y: 150}, imgui.ChildFlagsFrameStyle, imgui.WindowFlagsHorizontalScrollbar)
		for _, line := range ui.traceLines {
			imgui.Text(line)
		}
		if imgui.ScrollY() >= imgui.ScrollMaxY() {
			imgui.SetScrollHereYV(1.0)
		}
		imgui.EndChild()
	}

	imgui.End()
}

func (ui *UI) renderLandscapeLayout() {
	d := ui.radio.Display
	lcdW := float32(d.W) * ui.lcdScale
	lcdH := float32(d.H) * ui.lcdScale

	leftKeys, rightKeys := ui.splitKeys()

	// Top row: left keys | LCD | right keys.
	imgui.BeginGroup()

	imgui.BeginGroup()
	ui.renderKeyColumn(leftKeys)
	imgui.EndGroup()

	imgui.SameLine()
	ui.renderLCD(lcdW, lcdH)
	imgui.SameLine()

	imgui.BeginGroup()
	ui.renderKeyColumn(rightKeys)
	imgui.EndGroup()

	imgui.EndGroup()

	ui.renderPotsRow()

	imgui.Separator()
	ui.renderSwitchesRow()

	ui.renderTrimsRow()
}

func (ui *UI) renderPortraitLayout() {
	d := ui.radio.Display
	lcdW := float32(d.W) * ui.lcdScale
	lcdH := float32(d.H) * ui.lcdScale

	leftSwitches, rightSwitches := ui.splitSwitches()

	imgui.BeginGroup()

	imgui.BeginGroup()
	ui.renderSwitchColumn(leftSwitches)
	imgui.EndGroup()

	imgui.SameLine()
	ui.renderLCD(lcdW, lcdH)
	imgui.SameLine()

	imgui.BeginGroup()
	ui.renderSwitchColumn(rightSwitches)
	imgui.EndGroup()

	imgui.EndGroup()

	ui.renderKeysRow()
	ui.renderPotsRow()
	ui.renderTrimsRow()
}

func (ui *UI) renderLCD(w, h float32) {
	imgui.BeginGroup()

	cursorPos := imgui.CursorScreenPos()
	imgui.ImageV(ui.lcdTexture, imgui.Vec2{X: w, Y: h},
		imgui.Vec2{X: 0, Y: 0}, imgui.Vec2{X: 1, Y: 1})

	// Handle touch input on LCD.
	ui.lcdHovered = imgui.IsItemHovered()
	if ui.lcdHovered {
		mousePos := imgui.MousePos()
		lcdX := int((mousePos.X - cursorPos.X) / ui.lcdScale)
		lcdY := int((mousePos.Y - cursorPos.Y) / ui.lcdScale)

		if lcdX >= 0 && lcdX < ui.radio.Display.W && lcdY >= 0 && lcdY < ui.radio.Display.H {
			if imgui.IsMouseClickedBool(imgui.MouseButtonLeft) {
				ui.queueInput(inputCmd{kind: "touch", x: lcdX, y: lcdY})
			}
			if imgui.IsMouseReleased(imgui.MouseButtonLeft) {
				ui.queueInput(inputCmd{kind: "touchup"})
			}

			wheel := imgui.CurrentIO().MouseWheel()
			if wheel != 0 {
				ui.queueInput(inputCmd{kind: "rotary", state: int(wheel)})
			}
		}
	}

	imgui.EndGroup()
}

func (ui *UI) splitKeys() (left, right []KeyDef) {
	for _, k := range ui.radio.Keys {
		if strings.ToUpper(k.Side) == "R" {
			right = append(right, k)
		} else {
			left = append(left, k)
		}
	}
	return
}

func (ui *UI) renderKeyColumn(keys []KeyDef) {
	for _, k := range keys {
		label := k.Label
		if label == "" {
			label = k.Key
		}
		buttonID := fmt.Sprintf("%s##key_%s", label, k.Key)

		imgui.ButtonV(buttonID, imgui.Vec2{X: 70, Y: 30})

		keyName := strings.TrimPrefix(strings.ToUpper(k.Key), "KEY_")
		idx, ok := ScriptKeyIndex(keyName)
		if ok {
			if imgui.IsItemActivated() {
				ui.queueInput(inputCmd{kind: "key", index: idx, pressed: true})
			}
			if imgui.IsItemDeactivated() {
				ui.queueInput(inputCmd{kind: "key", index: idx, pressed: false})
			}
		}
	}
}

func (ui *UI) renderKeysRow() {
	for _, k := range ui.radio.Keys {
		label := k.Label
		if label == "" {
			label = k.Key
		}
		buttonID := fmt.Sprintf("%s##keyrow_%s", label, k.Key)
		imgui.ButtonV(buttonID, imgui.Vec2{X: 60, Y: 25})

		keyName := strings.TrimPrefix(strings.ToUpper(k.Key), "KEY_")
		idx, ok := ScriptKeyIndex(keyName)
		if ok {
			if imgui.IsItemActivated() {
				ui.queueInput(inputCmd{kind: "key", index: idx, pressed: true})
			}
			if imgui.IsItemDeactivated() {
				ui.queueInput(inputCmd{kind: "key", index: idx, pressed: false})
			}
		}
		imgui.SameLine()
	}
	imgui.NewLine()
}

func (ui *UI) splitSwitches() (left, right []SwitchDef) {
	visible := ui.visibleSwitches()
	mid := len(visible) / 2
	return visible[:mid], visible[mid:]
}

func (ui *UI) visibleSwitches() []SwitchDef {
	var visible []SwitchDef
	for _, sw := range ui.radio.Switches {
		if sw.Default == "NONE" || strings.HasPrefix(sw.Name, "SW") {
			continue
		}
		visible = append(visible, sw)
	}
	return visible
}

func (ui *UI) renderSwitchesRow() {
	visible := ui.visibleSwitches()
	if len(visible) == 0 {
		return
	}

	left, right := ui.splitSwitches()

	imgui.BeginGroup()

	imgui.BeginGroup()
	ui.renderSwitchColumn(left)
	imgui.EndGroup()

	imgui.SameLine()

	imgui.BeginGroup()
	ui.renderStickPlaceholder("Left Stick")
	imgui.SameLine()
	ui.renderStickPlaceholder("Right Stick")
	imgui.EndGroup()

	imgui.SameLine()

	imgui.BeginGroup()
	ui.renderSwitchColumn(right)
	imgui.EndGroup()

	imgui.EndGroup()
}

func (ui *UI) renderSwitchColumn(switches []SwitchDef) {
	for _, sw := range switches {
		ui.renderSwitch(sw)
	}
}

func (ui *UI) renderSwitch(sw SwitchDef) {
	val := ui.switchStates[sw.Name]

	var labels []string
	switch sw.Type {
	case "2POS":
		labels = []string{"UP", "DN"}
	case "3POS":
		labels = []string{"UP", "MID", "DN"}
	default:
		return
	}

	imgui.Text(sw.Name)
	imgui.SameLine()
	for i, l := range labels {
		id := fmt.Sprintf("%s##sw_%s_%d", l, sw.Name, i)
		if val == int32(i) {
			imgui.PushStyleColorVec4(imgui.ColButton, imgui.Vec4{X: 0.2, Y: 0.6, Z: 0.2, W: 1})
		}
		if imgui.ButtonV(id, imgui.Vec2{X: 30, Y: 20}) {
			ui.switchStates[sw.Name] = int32(i)
			ui.updateSwitch(sw.Name, int32(i))
		}
		if val == int32(i) {
			imgui.PopStyleColor()
		}
		if i < len(labels)-1 {
			imgui.SameLine()
		}
	}
}

func (ui *UI) updateSwitch(name string, state int32) {
	for i, sw := range ui.radio.Switches {
		if sw.Name == name {
			var simState int
			switch sw.Type {
			case "2POS":
				if state == 0 {
					simState = -1024
				} else {
					simState = 1024
				}
			case "3POS":
				switch state {
				case 0:
					simState = -1024
				case 1:
					simState = 0
				case 2:
					simState = 1024
				}
			}
			ui.queueInput(inputCmd{kind: "switch", index: i, state: simState})
			return
		}
	}
}

func (ui *UI) renderPotsRow() {
	var pots []InputDef
	var customSwitches []SwitchDef

	for _, inp := range ui.radio.Inputs {
		if inp.Type == "FLEX" && inp.Default != "NONE" {
			pots = append(pots, inp)
		}
	}

	for _, sw := range ui.radio.Switches {
		if strings.HasPrefix(sw.Name, "SW") && sw.Default != "NONE" {
			customSwitches = append(customSwitches, sw)
		}
	}

	if len(pots) == 0 && len(customSwitches) == 0 {
		return
	}

	imgui.Separator()

	for _, pot := range pots {
		ui.renderPot(pot)
		imgui.SameLine()
	}

	for _, sw := range customSwitches {
		label := fmt.Sprintf("%s##csw", sw.Name)
		imgui.ButtonV(label, imgui.Vec2{X: 50, Y: 25})
		if imgui.IsItemActivated() {
			for i, s := range ui.radio.Switches {
				if s.Name == sw.Name {
					ui.queueInput(inputCmd{kind: "switch", index: i, state: 1024})
					break
				}
			}
		}
		if imgui.IsItemDeactivated() {
			for i, s := range ui.radio.Switches {
				if s.Name == sw.Name {
					ui.queueInput(inputCmd{kind: "switch", index: i, state: -1024})
					break
				}
			}
		}
		imgui.SameLine()
	}

	imgui.NewLine()
}

func (ui *UI) renderPot(pot InputDef) {
	val := ui.potValues[pot.Name]
	label := pot.Label
	if label == "" {
		label = pot.Name
	}

	switch pot.Default {
	case "POT", "POT_CENTER":
		id := fmt.Sprintf("##pot_%s", pot.Name)
		imgui.Text(label)
		imgui.SameLine()
		if imgui.SliderIntV(id, &val, 0, 4096, "%d", 0) {
			ui.potValues[pot.Name] = val
			ui.updateAnalogInput(pot.Name, val)
		}

	case "SLIDER":
		id := fmt.Sprintf("##slider_%s", pot.Name)
		imgui.Text(label)
		if imgui.VSliderIntV(id, imgui.Vec2{X: 30, Y: 100}, &val, 0, 4096, "%d", 0) {
			ui.potValues[pot.Name] = val
			ui.updateAnalogInput(pot.Name, val)
		}

	case "MULTIPOS":
		imgui.Text(label)
		imgui.SameLine()
		for i := range 6 {
			id := fmt.Sprintf("%d##mp_%s_%d", i+1, pot.Name, i)
			if val == int32(i) {
				imgui.PushStyleColorVec4(imgui.ColButton, imgui.Vec4{X: 0.2, Y: 0.6, Z: 0.2, W: 1})
			}
			if imgui.ButtonV(id, imgui.Vec2{X: 25, Y: 25}) {
				ui.potValues[pot.Name] = int32(i)
				ui.updateAnalogInput(pot.Name, int32(i)*4096/5)
			}
			if val == int32(i) {
				imgui.PopStyleColor()
			}
			if i < 5 {
				imgui.SameLine()
			}
		}
	}
}

func (ui *UI) updateAnalogInput(name string, value int32) {
	for i, inp := range ui.radio.Inputs {
		if inp.Name == name {
			ui.queueInput(inputCmd{kind: "analog", index: i, value: uint16(value)})
			return
		}
	}
}

func (ui *UI) renderStickPlaceholder(label string) {
	const stickSize float32 = 120

	imgui.BeginGroup()
	imgui.Text(label)

	pos := imgui.CursorScreenPos()
	drawList := imgui.WindowDrawList()

	centerX := pos.X + stickSize/2
	centerY := pos.Y + stickSize/2

	// Background.
	drawList.AddRectFilledV(
		imgui.Vec2{X: pos.X, Y: pos.Y},
		imgui.Vec2{X: pos.X + stickSize, Y: pos.Y + stickSize},
		imgui.ColorU32Vec4(imgui.Vec4{X: 0.2, Y: 0.2, Z: 0.2, W: 1}),
		4, 0,
	)

	// Crosshair.
	gray := imgui.ColorU32Vec4(imgui.Vec4{X: 0.5, Y: 0.5, Z: 0.5, W: 1})
	drawList.AddLineV(imgui.Vec2{X: pos.X, Y: centerY}, imgui.Vec2{X: pos.X + stickSize, Y: centerY}, gray, 1)
	drawList.AddLineV(imgui.Vec2{X: centerX, Y: pos.Y}, imgui.Vec2{X: centerX, Y: pos.Y + stickSize}, gray, 1)

	// Center dot.
	red := imgui.ColorU32Vec4(imgui.Vec4{X: 0.8, Y: 0.2, Z: 0.2, W: 1})
	drawList.AddCircleFilledV(imgui.Vec2{X: centerX, Y: centerY}, 6, red, 12)

	imgui.Dummy(imgui.Vec2{X: stickSize, Y: stickSize})
	imgui.EndGroup()
}

func (ui *UI) renderTrimsRow() {
	if len(ui.radio.Trims) == 0 {
		return
	}

	imgui.Separator()
	imgui.Text("Trims")

	for i, trim := range ui.radio.Trims {
		minusID := fmt.Sprintf("-##trim_%s", trim.Name)
		plusID := fmt.Sprintf("+##trim_%s", trim.Name)

		imgui.Text(trim.Name)
		imgui.SameLine()

		imgui.ButtonV(minusID, imgui.Vec2{X: 25, Y: 25})
		if imgui.IsItemActivated() {
			ui.queueInput(inputCmd{kind: "trim", index: i * 2, pressed: true})
		}
		if imgui.IsItemDeactivated() {
			ui.queueInput(inputCmd{kind: "trim", index: i * 2, pressed: false})
		}

		imgui.SameLine()

		imgui.ButtonV(plusID, imgui.Vec2{X: 25, Y: 25})
		if imgui.IsItemActivated() {
			ui.queueInput(inputCmd{kind: "trim", index: i*2 + 1, pressed: true})
		}
		if imgui.IsItemDeactivated() {
			ui.queueInput(inputCmd{kind: "trim", index: i*2 + 1, pressed: false})
		}

		imgui.SameLine()
	}
	imgui.NewLine()
}

// imguiKeyMapping maps ImGui keys to simulator key indices.
var imguiKeyMapping = []struct {
	key   imgui.Key
	index int
}{
	{imgui.KeyS, 13},          // SYS
	{imgui.KeyM, 11},          // MODEL
	{imgui.KeyT, 12},          // TELE
	{imgui.KeyPageUp, 3},      // PAGE UP
	{imgui.KeyPageDown, 4},    // PAGE DN
	{imgui.KeyUpArrow, 5},     // UP
	{imgui.KeyDownArrow, 6},   // DOWN
	{imgui.KeyLeftArrow, 7},   // LEFT
	{imgui.KeyRightArrow, 8},  // RIGHT
	{imgui.KeyKeypadAdd, 9},   // PLUS
	{imgui.KeyMinus, 10},      // MINUS
	{imgui.KeyEnter, 2},       // ENTER
	{imgui.KeyEscape, 1},      // EXIT
	{imgui.KeyEqual, 0},       // MENU
}

// pollKeyboard reads ImGui key states and queues input commands.
func (ui *UI) pollKeyboard() {
	for _, km := range imguiKeyMapping {
		if imgui.IsKeyPressedBool(km.key) {
			ui.queueInput(inputCmd{kind: "key", index: km.index, pressed: true})
		}
		if imgui.IsKeyReleased(km.key) {
			ui.queueInput(inputCmd{kind: "key", index: km.index, pressed: false})
		}
	}

	if imgui.IsKeyPressedBool(imgui.KeyComma) {
		ui.queueInput(inputCmd{kind: "rotary", state: 1})
	}
	if imgui.IsKeyPressedBool(imgui.KeyPeriod) {
		ui.queueInput(inputCmd{kind: "rotary", state: -1})
	}

	wheel := imgui.CurrentIO().MouseWheel()
	if wheel != 0 && !ui.lcdHovered {
		ui.queueInput(inputCmd{kind: "rotary", state: int(wheel)})
	}
}

func (ui *UI) queueInput(cmd inputCmd) {
	select {
	case ui.inputCh <- cmd:
	default:
	}
}

