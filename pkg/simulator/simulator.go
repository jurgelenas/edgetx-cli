package simulator

import (
	"bytes"
	"context"
	"fmt"
	"runtime"
	"sync"
	"time"

	"github.com/jurgelenas/edgetx-cli/pkg/logging"
	"github.com/jurgelenas/edgetx-cli/pkg/manifest"
	pkgsync "github.com/jurgelenas/edgetx-cli/pkg/sync"
	"github.com/veandco/go-sdl2/sdl"
)

// Options configures the simulator.
type Options struct {
	Radio       *RadioDef
	WASMPath    string
	SDCardDir   string
	SettingsDir string

	// Package watching.
	WatchDir string
	Manifest *manifest.Manifest

	// Testability.
	Headless       bool
	Timeout        time.Duration
	ScreenshotPath string
	ScriptPath     string
}

// Simulator ties together the WASM runtime, UI, audio, and input.
type Simulator struct {
	opts    Options
	runtime *Runtime
	audio   *AudioPlayer
	ui      *UI

	// LCD buffer reused across frames (owned by WASM thread).
	lcdBuf  []byte
	lcdSize int
}

// New creates a simulator from the given options.
func New(opts Options) (*Simulator, error) {
	return &Simulator{
		opts:    opts,
		lcdSize: LCDBufferSize(opts.Radio.Display),
		lcdBuf:  make([]byte, LCDBufferSize(opts.Radio.Display)),
	}, nil
}

// Run executes the simulator. Blocks until the window is closed or context is cancelled.
func (s *Simulator) Run(ctx context.Context) error {
	// Main goroutine stays on its OS thread (required by SDL/OpenGL).
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()

	trace := &traceCapture{}

	if s.opts.Timeout > 0 {
		var cancel context.CancelFunc
		ctx, cancel = context.WithTimeout(ctx, s.opts.Timeout)
		defer cancel()
	}

	// Headless/script: no UI needed, run WASM on main thread.
	if s.opts.Headless || s.opts.ScriptPath != "" {
		if err := sdl.Init(sdl.INIT_AUDIO); err != nil {
			logging.WithError(err).Debug("SDL audio init failed in headless mode")
		} else {
			defer sdl.Quit()
		}
		return s.runDirect(ctx, trace)
	}

	// Windowed: UI on main thread, WASM on a dedicated OS thread.
	return s.runWindowed(ctx, trace)
}

// runDirect runs everything on the current (main) thread for headless/script mode.
func (s *Simulator) runDirect(ctx context.Context, trace *traceCapture) error {
	if err := s.initRuntime(trace); err != nil {
		return err
	}
	defer s.runtime.Stop()
	defer s.runtime.Close()

	if s.opts.WatchDir != "" && s.opts.Manifest != nil {
		go s.watchPackage(ctx)
	}

	if s.opts.ScriptPath != "" {
		return s.runScript(ctx)
	}

	return s.runHeadless(ctx, trace)
}

// runWindowed runs everything on the main thread: both WASM and UI.
// WAMR's exec_env is bound to the creating thread, and running WASM
// from a different goroutine (even with LockOSThread) crashes.
// The UI render callback polls WASM at 60fps (faster than the web
// simulator's 100ms timer). Inputs are queued via channel and
// processed in the same callback.
func (s *Simulator) runWindowed(ctx context.Context, trace *traceCapture) error {
	// Init WASM on main thread.
	if err := s.initRuntime(trace); err != nil {
		return err
	}
	defer s.runtime.Stop()
	defer s.runtime.Close()

	// Wait for firmware to finish booting before creating the SDL window.
	// WAMR's wasi-threads and SDL/OpenGL initialization conflict if they
	// run concurrently (the firmware spawns threads during lsScripts/luaInit).
	logging.Info("waiting for firmware boot...")
	select {
	case <-s.runtime.LCDReady():
		logging.Info("firmware booted")
	case <-ctx.Done():
		return ctx.Err()
	}

	// Consume the first frame so the firmware can produce the next one.
	s.runtime.CopyLCD(s.lcdBuf)
	s.runtime.LCDFlushed()
	logging.Info("first frame consumed")

	inputCh := make(chan inputCmd, 64)

	// UI setup on main thread — after firmware boot.
	logging.Info("creating UI...")
	s.ui = NewUI(UIOptions{
		Radio:   s.opts.Radio,
		InputCh: inputCh,
	})
	logging.Info("initializing SDL window...")
	if err := s.ui.Init(); err != nil {
		return fmt.Errorf("initializing UI: %w", err)
	}
	defer s.ui.Close()
	logging.Info("SDL window ready")

	if err := sdl.InitSubSystem(sdl.INIT_AUDIO); err != nil {
		logging.WithError(err).Warn("SDL audio subsystem init failed")
	}

	var err error
	s.audio, err = NewAudioPlayer(s.runtime.AudioQueue())
	if err != nil {
		logging.WithError(err).Warn("audio initialization failed, continuing without audio")
	} else {
		defer s.audio.Close()
	}

	if s.opts.WatchDir != "" && s.opts.Manifest != nil {
		go s.watchPackage(ctx)
	}

	// Close window when context is cancelled.
	go func() {
		<-ctx.Done()
		s.ui.SetShouldClose()
	}()

	// Start poll goroutine: all WASM calls happen here (separate from SDL thread).
	frameCh := make(chan []byte, 2)
	go s.pollLoop(ctx, inputCh, frameCh)

	// UI render loop — no WASM calls here, only channel reads.
	s.ui.Run(func() {
		for {
			select {
			case frame := <-frameCh:
				s.ui.UpdateLCD(frame)
			default:
				goto done
			}
		}
	done:

		if lines := trace.Drain(); len(lines) > 0 {
			for _, line := range lines {
				s.ui.AddTraceLine(line)
			}
		}
	})

	// Take screenshot at exit.
	if s.opts.ScreenshotPath != "" {
		return s.takeScreenshot()
	}
	return nil
}

// initRuntime creates and initializes the WASM runtime.
// Must be called from the thread that will make all subsequent WASM calls.
func (s *Simulator) initRuntime(trace *traceCapture) error {
	logging.Info("loading WASM module...")
	var err error
	s.runtime, err = NewRuntime(s.opts.WASMPath, s.opts.SDCardDir, s.opts.SettingsDir, trace)
	if err != nil {
		return fmt.Errorf("creating runtime: %w", err)
	}

	logging.Info("initializing simulator...")
	if err := s.runtime.Init(); err != nil {
		s.runtime.Close()
		return fmt.Errorf("simuInit: %w", err)
	}

	if err := s.runtime.SetFatfsPaths(s.opts.SDCardDir, s.opts.SettingsDir); err != nil {
		s.runtime.Close()
		return fmt.Errorf("simuFatfsSetPaths: %w", err)
	}

	logging.Info("creating defaults...")
	if err := s.runtime.CreateDefaults(); err != nil {
		s.runtime.Close()
		return fmt.Errorf("simuCreateDefaults: %w", err)
	}

	logging.Info("starting firmware...")
	if err := s.runtime.Start(); err != nil {
		s.runtime.Close()
		return fmt.Errorf("simuStart: %w", err)
	}

	logging.Info("firmware started")

	if err := s.runtime.AllocLCDBuffer(uint32(s.lcdSize)); err != nil {
		s.runtime.Stop()
		s.runtime.Close()
		return fmt.Errorf("allocating LCD buffer: %w", err)
	}

	return nil
}

// pollLoop runs on its own OS thread with a dedicated ExecEnv.
// Processes inputs and polls LCD, sending frames to the UI via frameCh.
func (s *Simulator) pollLoop(ctx context.Context, inputCh <-chan inputCmd, frameCh chan<- []byte) {
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()

	if err := s.runtime.CreatePollEnv(); err != nil {
		logging.WithError(err).Error("failed to create poll exec_env")
		return
	}

	ticker := time.NewTicker(100 * time.Millisecond)
	defer ticker.Stop()

	lcdReady := s.runtime.LCDReady()

	for {
		select {
		case <-ctx.Done():
			return
		case <-lcdReady:
			s.drainInputs(inputCh)
			s.copyAndSendLCD(frameCh)
		case <-ticker.C:
			s.drainInputs(inputCh)
			s.copyAndSendLCD(frameCh)
		case cmd := <-inputCh:
			s.processInput(cmd)
			s.drainInputs(inputCh)
		}
	}
}

func (s *Simulator) copyAndSendLCD(frameCh chan<- []byte) {
	n := s.runtime.CopyLCD(s.lcdBuf)
	if n > 0 {
		frame := make([]byte, n)
		copy(frame, s.lcdBuf[:n])
		select {
		case frameCh <- frame:
		default:
		}
		s.runtime.LCDFlushed()
	}
}

func (s *Simulator) drainInputs(inputCh <-chan inputCmd) {
	for {
		select {
		case cmd := <-inputCh:
			s.processInput(cmd)
		default:
			return
		}
	}
}

func (s *Simulator) runHeadless(ctx context.Context, trace *traceCapture) error {
	lcdReady := s.runtime.LCDReady()

	for {
		select {
		case <-ctx.Done():
			if s.opts.ScreenshotPath != "" {
				return s.takeScreenshot()
			}
			return nil
		case <-lcdReady:
			s.runtime.CopyLCD(s.lcdBuf)
			s.runtime.LCDFlushed()
			if lines := trace.Drain(); len(lines) > 0 {
				for _, line := range lines {
					logging.Debugf("[trace] %s", line)
				}
			}
		}
	}
}

func (s *Simulator) runScript(ctx context.Context) error {
	commands, err := ParseScript(s.opts.ScriptPath)
	if err != nil {
		return fmt.Errorf("parsing script: %w", err)
	}

	// Wait for firmware to boot (first LCD frame).
	select {
	case <-s.runtime.LCDReady():
	case <-ctx.Done():
		return ctx.Err()
	}

	getLCD := func() []byte {
		s.runtime.CopyLCD(s.lcdBuf)
		s.runtime.LCDFlushed()
		return s.lcdBuf
	}

	return ExecuteScript(ctx, commands, s.runtime, getLCD, s.opts.Radio.Display)
}

func (s *Simulator) processInput(cmd inputCmd) {
	switch cmd.kind {
	case "key":
		s.runtime.SetKey(cmd.index, cmd.pressed)
	case "switch":
		s.runtime.SetSwitch(cmd.index, cmd.state)
	case "trim":
		s.runtime.SetTrim(cmd.index, cmd.pressed)
	case "touch":
		s.runtime.TouchDown(cmd.x, cmd.y)
	case "touchup":
		s.runtime.TouchUp()
	case "rotary":
		s.runtime.RotaryEncoder(cmd.state)
	case "analog":
		s.runtime.SetAnalog(cmd.index, cmd.value)
	}
}

func (s *Simulator) takeScreenshot() error {
	s.runtime.CopyLCD(s.lcdBuf)
	d := s.opts.Radio.Display
	rgba := DecodeFramebuffer(s.lcdBuf, d)
	return saveScreenshot(s.opts.ScreenshotPath, rgba, d.W, d.H)
}

func (s *Simulator) watchPackage(ctx context.Context) {
	items := s.opts.Manifest.ContentItems(true)
	opts := pkgsync.Options{
		Manifest:    s.opts.Manifest,
		ManifestDir: s.opts.WatchDir,
		TargetDir:   s.opts.SDCardDir,
		Items:       items,
		Callbacks: pkgsync.Callbacks{
			OnSyncEvent: func(e pkgsync.Event) {
				logging.Infof("[sync] %s: %s", e.Op, e.RelPath)
			},
			OnError: func(err error) {
				logging.WithError(err).Warn("sync error")
			},
		},
	}

	if err := pkgsync.Watch(ctx, opts); err != nil {
		logging.WithError(err).Warn("file watching stopped")
	}
}

// traceCapture is a thread-safe io.Writer that buffers trace lines.
type traceCapture struct {
	mu    sync.Mutex
	buf   bytes.Buffer
	lines []string
}

func (tc *traceCapture) Write(p []byte) (int, error) {
	tc.mu.Lock()
	defer tc.mu.Unlock()
	tc.buf.Write(p)
	// Split on newlines.
	for {
		line, err := tc.buf.ReadString('\n')
		if err != nil {
			// Put back incomplete line.
			tc.buf.WriteString(line)
			break
		}
		tc.lines = append(tc.lines, line[:len(line)-1]) // strip newline
	}
	return len(p), nil
}

func (tc *traceCapture) Drain() []string {
	tc.mu.Lock()
	defer tc.mu.Unlock()
	if len(tc.lines) == 0 {
		return nil
	}
	lines := tc.lines
	tc.lines = nil
	return lines
}
