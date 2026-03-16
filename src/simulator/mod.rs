pub mod framebuffer;
pub mod input;
pub mod runtime;
pub mod screenshot;
pub mod script;
pub mod sdcard;

use anyhow::Result;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::radio_catalog::RadioDef;
use crate::simulator_ui::SimulatorApp;
use crate::simulator_ui::app::CustomSwitchState;

pub struct SimulatorOptions {
    pub radio: RadioDef,
    pub wasm_path: PathBuf,
    pub sdcard_dir: PathBuf,
    pub settings_dir: PathBuf,
    #[allow(dead_code)]
    pub watch_dir: Option<PathBuf>,
    pub headless: bool,
    pub timeout: Option<Duration>,
    pub screenshot_path: Option<String>,
    pub script_path: Option<PathBuf>,
}

pub struct Simulator {
    opts: SimulatorOptions,
}

impl Simulator {
    pub fn new(opts: SimulatorOptions) -> Result<Self> {
        Ok(Self { opts })
    }

    pub fn run(self) -> Result<()> {
        let wasm_bytes = std::fs::read(&self.opts.wasm_path)?;

        if self.opts.headless {
            return self.run_headless(&wasm_bytes);
        }

        self.run_windowed(&wasm_bytes)
    }

    fn run_headless(self, wasm_bytes: &[u8]) -> Result<()> {
        // Initialize audio channel; drop receiver so samples are silently discarded
        let _audio_rx = runtime::init_audio_channel();
        drop(_audio_rx);

        let mut rt = runtime::Runtime::new(
            wasm_bytes,
            &self.opts.radio,
            &self.opts.sdcard_dir,
            &self.opts.settings_dir,
        )?;

        rt.start()?;

        // Execute script if provided, otherwise run with timeout or until stopped
        if let Some(ref script_path) = self.opts.script_path {
            self.run_script(&mut rt, script_path)?;
        } else if let Some(timeout) = self.opts.timeout {
            std::thread::sleep(timeout);
        } else {
            // Wait for Ctrl+C
            let (tx, rx) = std::sync::mpsc::channel();
            ctrlc_channel(&tx);
            let _ = rx.recv();
        }

        // Take screenshot if requested (separate from script screenshots)
        if let Some(ref path) = self.opts.screenshot_path
            && let Some(lcd) = rt.get_lcd_buffer()
        {
            let rgba = framebuffer::decode(&lcd, &self.opts.radio.display);
            screenshot::save_screenshot(
                path,
                &rgba,
                self.opts.radio.display.w as u32,
                self.opts.radio.display.h as u32,
            )?;
        }

        rt.stop();
        Ok(())
    }

    fn run_script(&self, rt: &mut runtime::Runtime, script_path: &Path) -> Result<()> {
        let commands = script::parse_script(script_path)?;
        for cmd in &commands {
            match cmd {
                script::ScriptCommand::Wait(d) => std::thread::sleep(*d),
                script::ScriptCommand::KeyPress(key) => {
                    if let Some(idx) = input::script_key_index(key) {
                        rt.set_key(idx, true);
                    } else {
                        eprintln!("Warning: unknown key {:?} in script", key);
                    }
                }
                script::ScriptCommand::KeyRelease(key) => {
                    if let Some(idx) = input::script_key_index(key) {
                        rt.set_key(idx, false);
                    } else {
                        eprintln!("Warning: unknown key {:?} in script", key);
                    }
                }
                script::ScriptCommand::Screenshot(path) => {
                    if let Some(lcd) = rt.get_lcd_buffer() {
                        let rgba = framebuffer::decode(&lcd, &self.opts.radio.display);
                        screenshot::save_screenshot(
                            path,
                            &rgba,
                            self.opts.radio.display.w as u32,
                            self.opts.radio.display.h as u32,
                        )?;
                    }
                }
            }
        }
        Ok(())
    }

    fn run_windowed(self, wasm_bytes: &[u8]) -> Result<()> {
        let radio = self.opts.radio.clone();
        let sdcard_dir = self.opts.sdcard_dir.clone();
        let settings_dir = self.opts.settings_dir.clone();

        // Start WASM runtime on a separate thread
        let (lcd_tx, lcd_rx) = std::sync::mpsc::channel::<Vec<u8>>();
        let (input_tx, input_rx) = std::sync::mpsc::channel::<input::InputEvent>();
        let (cs_tx, cs_rx) = std::sync::mpsc::channel::<Vec<CustomSwitchState>>();

        // Initialize audio and trace channels before spawning WASM thread
        let audio_rx = runtime::init_audio_channel();
        let trace_rx = runtime::init_trace_channel();
        let audio_player = crate::simulator_ui::audio::AudioPlayer::new()?;

        let radio_clone = radio.clone();
        let wasm_bytes = wasm_bytes.to_vec();

        let _wasm_thread = std::thread::spawn(move || -> Result<()> {
            let mut rt =
                runtime::Runtime::new(&wasm_bytes, &radio_clone, &sdcard_dir, &settings_dir)?;

            rt.start()?;

            // Main loop: poll inputs, send LCD updates on notification or every 100ms
            use std::collections::HashMap;
            loop {
                // Drain all pending input events, deduplicating trim/key to final state
                let mut trim_finals: HashMap<i32, bool> = HashMap::new();
                let mut key_finals: HashMap<i32, bool> = HashMap::new();

                while let Ok(event) = input_rx.try_recv() {
                    match event {
                        input::InputEvent::Key { index, pressed } => {
                            key_finals.insert(index, pressed);
                        }
                        input::InputEvent::Rotary(delta) => {
                            rt.rotary_encoder(delta);
                        }
                        input::InputEvent::Touch { x, y, down } => {
                            if down {
                                rt.touch_down(x, y);
                            } else {
                                rt.touch_up();
                            }
                        }
                        input::InputEvent::Switch { index, state } => {
                            rt.set_switch(index, state);
                        }
                        input::InputEvent::Trim { index, pressed } => {
                            trim_finals.insert(index, pressed);
                        }
                        input::InputEvent::Analog { index, value } => {
                            rt.set_analog(index, value);
                        }
                        input::InputEvent::Quit => {
                            rt.stop();
                            return Ok(());
                        }
                    }
                }

                // Apply only the final state per button — one WAMR call each
                for (index, pressed) in key_finals {
                    rt.set_key(index, pressed);
                }
                for (index, pressed) in trim_finals {
                    rt.set_trim(index, pressed);
                }

                // Check for LCD update: either firmware signalled via simuLcdNotify
                // or we poll every 100ms as fallback
                if runtime::LCD_READY.swap(false, std::sync::atomic::Ordering::Relaxed)
                    && let Some(lcd) = rt.get_lcd_buffer()
                {
                    let _ = lcd_tx.send(lcd);
                }

                // Drain queued audio samples and play them
                while let Ok(samples) = audio_rx.try_recv() {
                    audio_player.play_samples(&samples, 32000);
                }

                // Poll custom switch LED states from firmware
                let num_cs = rt.get_num_custom_switches() as usize;
                if num_cs > 0 {
                    let states: Vec<CustomSwitchState> = (0..num_cs)
                        .map(|i| {
                            let active = rt.get_custom_switch_state(i as u8);
                            let rgb = if active {
                                rt.get_custom_switch_color(i as u8)
                            } else {
                                0
                            };
                            let color = if active && rgb != 0 {
                                egui::Color32::from_rgb(
                                    (rgb >> 16) as u8,
                                    (rgb >> 8) as u8,
                                    rgb as u8,
                                )
                            } else if active {
                                egui::Color32::from_rgb(56, 191, 249)
                            } else {
                                egui::Color32::from_gray(60)
                            };
                            CustomSwitchState { color }
                        })
                        .collect();
                    let _ = cs_tx.send(states);
                }

                // Short sleep to avoid busy-waiting, but responsive to notifications
                std::thread::sleep(Duration::from_millis(10));
            }
        });

        // Compute window size based on layout
        let lcd_w = radio.display.w as f32;
        let lcd_h = radio.display.h as f32;
        let lcd_scale = if radio.display.is_color() {
            1.0
        } else {
            (480.0 / lcd_w).max(1.0).floor()
        };
        let lcd_display_w = lcd_w * lcd_scale;
        let lcd_display_h = lcd_h * lcd_scale;

        // Count sliders for dynamic width
        let slider_count = radio
            .inputs
            .iter()
            .filter(|inp| inp.input_type == "FLEX" && inp.default == "SLIDER")
            .count();
        let switch_w = 140.0_f32;
        let slider_w_each = 40.0_f32;
        let stick_w = 140.0_f32;
        let left_sl = slider_count / 2;
        let right_sl = slider_count - left_sl;
        let controls_w = switch_w * 2.0
            + left_sl as f32 * slider_w_each
            + right_sl as f32 * slider_w_each
            + stick_w * 2.0
            + 60.0;
        let lcd_row_w = lcd_display_w + 196.0;
        let window_w = controls_w.max(lcd_row_w).max(900.0);
        let base_h = lcd_display_h + 500.0;
        let trace_panel_h = 380.0;
        let window_h = base_h + 30.0;

        let native_options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([window_w, window_h])
                .with_min_inner_size([window_w, base_h + 30.0])
                .with_title(format!("EdgeTX Simulator - {}", radio.name))
                .with_decorations(true),
            ..Default::default()
        };

        let mut app = SimulatorApp::new(radio, lcd_rx, input_tx, cs_rx, trace_rx);
        app.window_size = (window_w, base_h);
        app.trace_panel_height = trace_panel_h;

        eframe::run_native(
            "EdgeTX Simulator",
            native_options,
            Box::new(|cc| {
                egui_extras::install_image_loaders(&cc.egui_ctx);
                Ok(Box::new(app))
            }),
        )
        .map_err(|e| anyhow::anyhow!("eframe error: {e}"))?;

        Ok(())
    }
}

fn ctrlc_channel(tx: &std::sync::mpsc::Sender<()>) {
    let tx = tx.clone();
    let _ = ctrlc::set_handler(move || {
        let _ = tx.send(());
    });
}

// ctrlc crate is not in deps, use a simple signal approach
mod ctrlc {
    pub fn set_handler<F: Fn() + Send + 'static>(handler: F) -> Result<(), std::io::Error> {
        std::thread::spawn(move || {
            // Block on signal
            signal_hook_simple();
            handler();
        });
        Ok(())
    }

    fn signal_hook_simple() {
        // Simple approach: just sleep forever, the OS will handle SIGINT
        loop {
            std::thread::sleep(std::time::Duration::from_secs(3600));
        }
    }
}
