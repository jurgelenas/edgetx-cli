package simulator

import (
	"fmt"
	"io"
	"os"
	"sync"
	"unsafe"

	"github.com/bytecodealliance/wasm-micro-runtime/language-bindings/go/wamr"
	"github.com/jurgelenas/edgetx-cli/pkg/logging"
)

// Runtime wraps a WAMR WASM module instance and provides the simulator interface.
// All methods that call WASM exports must be called from the same OS thread
// that created the runtime (the WASM goroutine).
type Runtime struct {
	module   *wamr.Module
	instance *wamr.Instance
	mu       sync.Mutex

	// Persistent WASM-side LCD buffer (allocated via WASM malloc, reused).
	lcdWasmOff  uint64
	lcdWasmSize uint32

	// Dedicated exec_env for calling WASM from the poll goroutine.
	pollEnv *ExecEnv

	// Host state accessed by import trampolines.
	analogValues [32]uint16
	audioQueue   chan []int16
	lcdReady     chan struct{}
	traceWriter  io.Writer
}

// activeRuntime is the current singleton Runtime.
// Only one WASM simulator instance is supported per process.
// CGO trampolines use this to find the Go Runtime.
var activeRuntime *Runtime

// lookupRuntime returns the active runtime for CGO callbacks.
// The instPtr parameter is ignored (single-instance design) but kept
// for future multi-instance support.
func lookupRuntime(_ unsafe.Pointer) *Runtime {
	return activeRuntime
}

// NewRuntime loads a WASM module and prepares it for execution.
func NewRuntime(wasmPath, sdcardDir, settingsDir string, traceWriter io.Writer) (*Runtime, error) {
	if traceWriter == nil {
		traceWriter = os.Stderr
	}

	rt := &Runtime{
		audioQueue:  make(chan []int16, 64),
		lcdReady:    make(chan struct{}, 1),
		traceWriter: traceWriter,
	}

	// Initialize the WAMR runtime singleton.
	if err := wamr.Runtime().Init(); err != nil {
		return nil, fmt.Errorf("initializing WAMR runtime: %w", err)
	}

	// Register host import functions before loading the module.
	if err := registerEnvNatives(); err != nil {
		return nil, fmt.Errorf("registering native symbols: %w", err)
	}

	// Load the WASM binary.
	wasmBytes, err := os.ReadFile(wasmPath)
	if err != nil {
		return nil, fmt.Errorf("reading WASM file: %w", err)
	}

	rt.module, err = wamr.NewModule(wasmBytes)
	if err != nil {
		return nil, fmt.Errorf("loading WASM module: %w", err)
	}

	// Configure WASI preopens for the SD card and settings directories.
	setWasiPreopens(rt.module, sdcardDir, settingsDir)

	// Instantiate with generous stack/heap.
	const stackSize = 256 * 1024
	const heapSize = 8 * 1024 * 1024
	rt.instance, err = wamr.NewInstance(rt.module, stackSize, heapSize)
	if err != nil {
		rt.module.Destroy()
		return nil, fmt.Errorf("instantiating WASM module: %w", err)
	}

	// Set as active runtime for CGO callbacks.
	activeRuntime = rt

	logging.Debug("WASM module loaded and instantiated")
	return rt, nil
}

// Init calls simuInit() in the WASM module.
func (rt *Runtime) Init() error {
	rt.mu.Lock()
	defer rt.mu.Unlock()
	return rt.callVoid("simuInit")
}

// SetFatfsPaths sets the SD card and settings paths in the WASM module.
func (rt *Runtime) SetFatfsPaths(sdcardDir, settingsDir string) error {
	rt.mu.Lock()
	defer rt.mu.Unlock()

	sdPtr, sdOff := rt.mallocString(sdcardDir)
	if sdPtr == nil {
		return fmt.Errorf("malloc for sdcard path failed")
	}
	defer rt.instance.ModuleFree(sdOff)

	setPtr, setOff := rt.mallocString(settingsDir)
	if setPtr == nil {
		return fmt.Errorf("malloc for settings path failed")
	}
	defer rt.instance.ModuleFree(setOff)

	args := []uint32{uint32(sdOff), uint32(setOff)}
	return rt.instance.CallFunc("simuFatfsSetPaths", 2, args)
}

// CreateDefaults calls simuCreateDefaults() in the WASM module.
func (rt *Runtime) CreateDefaults() error {
	rt.mu.Lock()
	defer rt.mu.Unlock()
	return rt.callVoid("simuCreateDefaults")
}

// Start calls simuStart(tests=false) in the WASM module.
func (rt *Runtime) Start() error {
	rt.mu.Lock()
	defer rt.mu.Unlock()
	args := []uint32{0} // tests = false
	return rt.instance.CallFunc("simuStart", 1, args)
}

// IsRunning calls simuIsRunning() and returns the result.
func (rt *Runtime) IsRunning() bool {
	rt.mu.Lock()
	defer rt.mu.Unlock()
	args := []uint32{0}
	if err := rt.instance.CallFunc("simuIsRunning", 0, args); err != nil {
		return false
	}
	return args[0] != 0
}

// Stop calls simuStop() in the WASM module.
func (rt *Runtime) Stop() error {
	rt.mu.Lock()
	defer rt.mu.Unlock()
	return rt.callVoid("simuStop")
}

// CreatePollEnv creates a dedicated exec_env for calling WASM from a
// poll goroutine. Must be called from the goroutine's locked OS thread.
func (rt *Runtime) CreatePollEnv() error {
	env, err := CreateExecEnv(rt.instance)
	if err != nil {
		return err
	}
	rt.pollEnv = env
	return nil
}

// Close destroys the WASM instance and module, and clears the active runtime.
func (rt *Runtime) Close() {
	if activeRuntime == rt {
		activeRuntime = nil
	}
	if rt.pollEnv != nil {
		rt.pollEnv.Destroy()
		rt.pollEnv = nil
	}
	if rt.instance != nil {
		if rt.lcdWasmOff != 0 {
			freeArgs := []uint32{uint32(rt.lcdWasmOff)}
			rt.instance.CallFunc("free", 1, freeArgs)
		}
		rt.instance.Destroy()
	}
	if rt.module != nil {
		rt.module.Destroy()
	}
	freeWasiDirs()
}

// --- Input methods ---

// SetKey sets a key state in the simulator.
func (rt *Runtime) SetKey(key int, pressed bool) {
	rt.mu.Lock()
	defer rt.mu.Unlock()
	state := uint32(0)
	if pressed {
		state = 1
	}
	args := []uint32{uint32(key), state}
	rt.callPoll("simuSetKey", 2, args)
}

// SetSwitch sets a switch state in the simulator.
func (rt *Runtime) SetSwitch(sw int, state int) {
	rt.mu.Lock()
	defer rt.mu.Unlock()
	args := []uint32{uint32(sw), uint32(state)}
	rt.callPoll("simuSetSwitch", 2, args)
}

// SetTrim sets a trim state in the simulator.
func (rt *Runtime) SetTrim(trim int, pressed bool) {
	rt.mu.Lock()
	defer rt.mu.Unlock()
	state := uint32(0)
	if pressed {
		state = 1
	}
	args := []uint32{uint32(trim), state}
	rt.callPoll("simuSetTrim", 2, args)
}

// TouchDown sends a touch-down event at (x, y).
func (rt *Runtime) TouchDown(x, y int) {
	rt.mu.Lock()
	defer rt.mu.Unlock()
	args := []uint32{uint32(x), uint32(y)}
	rt.callPoll("simuTouchDown", 2, args)
}

// TouchUp sends a touch-up event.
func (rt *Runtime) TouchUp() {
	rt.mu.Lock()
	defer rt.mu.Unlock()
	rt.callPoll("simuTouchUp", 0, nil)
}

// RotaryEncoder sends rotary encoder steps.
func (rt *Runtime) RotaryEncoder(steps int) {
	rt.mu.Lock()
	defer rt.mu.Unlock()
	args := []uint32{uint32(int32(steps))}
	rt.callPoll("simuRotaryEncoderEvent", 1, args)
}

// SetAnalog sets an analog input value (0-4096, center=2048).
func (rt *Runtime) SetAnalog(idx int, value uint16) {
	if idx >= 0 && idx < len(rt.analogValues) {
		rt.analogValues[idx] = value
	}
}

// --- LCD methods ---

// LCDDimensions returns the LCD width, height, and color depth.
func (rt *Runtime) LCDDimensions() (w, h, depth int) {
	rt.mu.Lock()
	defer rt.mu.Unlock()

	args := []uint32{0}
	rt.instance.CallFunc("simuLcdGetWidth", 0, args)
	w = int(args[0])

	args[0] = 0
	rt.instance.CallFunc("simuLcdGetHeight", 0, args)
	h = int(args[0])

	args[0] = 0
	rt.instance.CallFunc("simuLcdGetDepth", 0, args)
	depth = int(args[0])

	return
}

// LCDChanged returns whether the LCD framebuffer has changed since last check.
func (rt *Runtime) LCDChanged() bool {
	rt.mu.Lock()
	defer rt.mu.Unlock()
	args := []uint32{0}
	if err := rt.instance.CallFunc("simuLcdChanged", 0, args); err != nil {
		return false
	}
	return args[0] != 0
}

// AllocLCDBuffer pre-allocates the LCD buffer in WASM memory using the
// module's own malloc. Must be called on the WASM thread.
func (rt *Runtime) AllocLCDBuffer(size uint32) error {
	rt.mu.Lock()
	defer rt.mu.Unlock()
	mallocArgs := []uint32{size}
	if err := rt.instance.CallFunc("malloc", 1, mallocArgs); err != nil || mallocArgs[0] == 0 {
		return fmt.Errorf("WASM malloc(%d) failed", size)
	}
	rt.lcdWasmOff = uint64(mallocArgs[0])
	rt.lcdWasmSize = size
	return nil
}

// CopyLCD copies the LCD framebuffer into the provided buffer.
// Returns the number of bytes copied.
func (rt *Runtime) CopyLCD(buf []byte) int {
	rt.mu.Lock()
	defer rt.mu.Unlock()

	if rt.lcdWasmOff == 0 {
		return 0
	}

	args := []uint32{uint32(rt.lcdWasmOff), uint32(len(buf))}
	copied32, err := rt.callPoll("simuLcdCopy", 2, args)
	if err != nil {
		return 0
	}
	copied := int(copied32)

	// Copy from WASM memory to Go buffer.
	nativePtr := rt.instance.AddrAppToNative(rt.lcdWasmOff)
	if nativePtr == nil {
		return 0
	}

	src := unsafe.Slice(nativePtr, copied)
	copy(buf, src)

	return copied
}

// LCDFlushed signals to the WASM module that the LCD frame has been consumed.
func (rt *Runtime) LCDFlushed() {
	rt.mu.Lock()
	defer rt.mu.Unlock()
	rt.callPoll("simuLcdFlushed", 0, nil)
}

// LCDReady returns the channel signalled by the firmware when a new LCD frame is available.
func (rt *Runtime) LCDReady() <-chan struct{} {
	return rt.lcdReady
}

// AudioQueue returns the channel for receiving audio samples from the WASM module.
func (rt *Runtime) AudioQueue() <-chan []int16 {
	return rt.audioQueue
}

// --- Internal helpers ---

// callPoll calls a WASM function using the poll exec_env if available,
// otherwise falls back to the default exec_env (main thread only).
func (rt *Runtime) callPoll(funcName string, argc uint32, args []uint32) (int32, error) {
	if rt.pollEnv != nil {
		return rt.pollEnv.Call(funcName, argc, args)
	}
	// Fallback to Go bindings (same thread as instance creation).
	clearException(rt.instance)
	if err := rt.instance.CallFunc(funcName, argc, args); err != nil {
		clearException(rt.instance)
		return 0, err
	}
	if argc > 0 {
		return int32(args[0]), nil
	}
	return 0, nil
}

func (rt *Runtime) callVoid(funcName string) error {
	clearException(rt.instance)
	err := rt.instance.CallFunc(funcName, 0, nil)
	if err != nil {
		clearException(rt.instance)
	}
	return err
}

func (rt *Runtime) mallocString(s string) (*uint8, uint64) {
	size := uint64(len(s) + 1) // +1 for null terminator
	off, ptr := rt.instance.ModuleMalloc(size)
	if ptr == nil {
		return nil, 0
	}
	// Copy string data into WASM memory.
	dst := unsafe.Slice(ptr, size)
	copy(dst, []byte(s))
	dst[len(s)] = 0 // null terminate
	return ptr, off
}
