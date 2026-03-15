pub mod audio;
pub mod display;
pub mod input;
pub mod radios;
pub mod runtime;
pub mod script;
pub mod sdcard;

use anyhow::Result;
use std::path::PathBuf;
use std::time::Duration;

use radios::RadioDef;

/// Firmware-reported custom switch LED color, sent from WASM thread to UI.
struct CustomSwitchState {
    color: egui::Color32,
}

pub struct SimulatorOptions {
    pub radio: RadioDef,
    pub wasm_path: PathBuf,
    pub sdcard_dir: PathBuf,
    pub settings_dir: PathBuf,
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

        // Run with timeout or until stopped
        if let Some(timeout) = self.opts.timeout {
            std::thread::sleep(timeout);
        } else {
            // Wait for Ctrl+C
            let (tx, rx) = std::sync::mpsc::channel();
            ctrlc_channel(&tx);
            let _ = rx.recv();
        }

        // Take screenshot if requested
        if let Some(ref path) = self.opts.screenshot_path {
            if let Some(lcd) = rt.get_lcd_buffer() {
                let rgba = display::decode_framebuffer(&lcd, &self.opts.radio.display);
                script::save_screenshot(
                    path,
                    &rgba,
                    self.opts.radio.display.w as u32,
                    self.opts.radio.display.h as u32,
                )?;
            }
        }

        rt.stop();
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
        let audio_player = audio::AudioPlayer::new()?;

        let radio_clone = radio.clone();
        let wasm_bytes = wasm_bytes.to_vec();

        let _wasm_thread = std::thread::spawn(move || -> Result<()> {
            let mut rt = runtime::Runtime::new(
                &wasm_bytes,
                &radio_clone,
                &sdcard_dir,
                &settings_dir,
            )?;

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
                if runtime::LCD_READY.swap(false, std::sync::atomic::Ordering::Relaxed) {
                    if let Some(lcd) = rt.get_lcd_buffer() {
                        let _ = lcd_tx.send(lcd);
                    }
                }

                // Drain queued audio samples and play them
                while let Ok(samples) = audio_rx.try_recv() {
                    audio_player.play_samples(&samples, 32000);
                }

                // Poll custom switch LED states from firmware
                let num_cs = rt.get_num_custom_switches() as usize;
                if num_cs > 0 {
                    let states: Vec<CustomSwitchState> = (0..num_cs).map(|i| {
                        let active = rt.get_custom_switch_state(i as u8);
                        let rgb = if active { rt.get_custom_switch_color(i as u8) } else { 0 };
                        let color = if active && rgb != 0 {
                            egui::Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
                        } else if active {
                            egui::Color32::from_rgb(56, 191, 249)
                        } else {
                            egui::Color32::from_gray(60)
                        };
                        CustomSwitchState { color }
                    }).collect();
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
        let slider_count = radio.inputs.iter()
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

struct SimulatorApp {
    radio: RadioDef,
    lcd_rx: std::sync::mpsc::Receiver<Vec<u8>>,
    input_tx: std::sync::mpsc::Sender<input::InputEvent>,
    lcd_texture: Option<egui::TextureHandle>,
    last_lcd: Option<Vec<u8>>,
    /// Current switch states (index -> state value).
    switch_states: Vec<i32>,
    /// Current potentiometer/slider values (index -> value).
    analog_values: Vec<u16>,
    /// Current multipos positions (index -> selected position 0..5).
    multipos_positions: Vec<usize>,
    /// Current stick positions as (x, y) in 0.0..1.0 range. Index 0 = left, 1 = right.
    stick_positions: [(f32, f32); 2],
    /// Maps stick index (0=left, 1=right) to (x_analog_idx, y_analog_idx).
    stick_analog_indices: [(usize, usize); 2],
    /// Tracks which trim buttons are currently pressed (for edge-detection).
    trim_pressed: std::collections::HashSet<i32>,
    /// Tracks which key buttons are currently pressed (for edge-detection).
    key_pressed: std::collections::HashSet<i32>,
    /// Firmware-reported custom switch LED states (indexed by custom switch position).
    custom_switch_led_states: Vec<CustomSwitchState>,
    cs_rx: std::sync::mpsc::Receiver<Vec<CustomSwitchState>>,
    /// Scale factor for LCD display rendering (>1 for small/BW displays).
    lcd_scale: f32,
    /// Receiver for trace messages from the WASM firmware.
    trace_rx: std::sync::mpsc::Receiver<String>,
    /// Ring buffer of trace output lines (max 200).
    trace_lines: Vec<String>,
    /// Whether the trace output panel is expanded.
    trace_open: bool,
    /// Base window dimensions (without trace panel).
    window_size: (f32, f32),
    /// Height of the trace panel when expanded.
    trace_panel_height: f32,
}

impl SimulatorApp {
    fn new(
        radio: RadioDef,
        lcd_rx: std::sync::mpsc::Receiver<Vec<u8>>,
        input_tx: std::sync::mpsc::Sender<input::InputEvent>,
        cs_rx: std::sync::mpsc::Receiver<Vec<CustomSwitchState>>,
        trace_rx: std::sync::mpsc::Receiver<String>,
    ) -> Self {
        let switch_count = radio.switches.len();
        let input_count = radio.inputs.len();

        // Initialize switch defaults: all switches start DOWN (1)
        let switch_states = vec![1i32; switch_count];

        // Initialize analog defaults
        let mut analog_values = vec![2048u16; input_count];
        for (i, inp) in radio.inputs.iter().enumerate() {
            if let Ok(v) = inp.default.parse::<u16>() {
                analog_values[i] = v;
            }
            // MULTIPOS starts at position 0 → ADC value 0
            if inp.default == "MULTIPOS" {
                analog_values[i] = 0;
            }
        }

        // Sync initial analog values to the shared array so the firmware
        // sees the same defaults the UI shows on boot.
        for (i, &val) in analog_values.iter().enumerate() {
            runtime::set_analog_value(i, val);
        }

        let multipos_positions = vec![0usize; input_count];

        // Compute stick-to-analog index mapping from STICK-type inputs.
        // Names starting with L → left stick (0), R → right stick (1).
        // H suffix → X-axis, V suffix → Y-axis.
        let mut stick_analog_indices = [(0usize, 0usize); 2];
        for (i, inp) in radio.inputs.iter().enumerate() {
            if inp.input_type != "STICK" {
                continue;
            }
            let name = inp.name.trim();
            let stick_idx = if name.starts_with('L') || name.starts_with('l') {
                0
            } else if name.starts_with('R') || name.starts_with('r') {
                1
            } else {
                continue;
            };
            if name.ends_with('H') || name.ends_with('h') {
                stick_analog_indices[stick_idx].0 = i;
            } else if name.ends_with('V') || name.ends_with('v') {
                stick_analog_indices[stick_idx].1 = i;
            }
        }

        let lcd_scale = if radio.display.is_color() {
            1.0
        } else {
            (480.0 / radio.display.w as f32).max(1.0).floor()
        };

        Self {
            radio,
            lcd_rx,
            input_tx,
            lcd_texture: None,
            last_lcd: None,
            switch_states,
            analog_values,
            multipos_positions,
            stick_positions: [(0.5, 0.5); 2],
            stick_analog_indices,
            trim_pressed: std::collections::HashSet::new(),
            key_pressed: std::collections::HashSet::new(),
            custom_switch_led_states: Vec::new(),
            cs_rx,
            lcd_scale,
            trace_rx,
            trace_lines: Vec::new(),
            trace_open: false,
            window_size: (0.0, 0.0),
            trace_panel_height: 380.0,
        }
    }

    fn send(&self, event: input::InputEvent) {
        let _ = self.input_tx.send(event);
    }

    /// Render the LCD display with touch support and mouse-wheel rotary encoder.
    fn show_lcd(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if let Some(ref lcd_data) = self.last_lcd {
            let rgba = display::decode_framebuffer(lcd_data, &self.radio.display);
            let w = self.radio.display.w as usize;
            let h = self.radio.display.h as usize;

            let image = egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba);

            let texture = self.lcd_texture.get_or_insert_with(|| {
                ctx.load_texture("lcd", image.clone(), egui::TextureOptions::NEAREST)
            });
            texture.set(image, egui::TextureOptions::NEAREST);

            let size = egui::vec2(w as f32 * self.lcd_scale, h as f32 * self.lcd_scale);
            let img = egui::Image::new(egui::load::SizedTexture::new(texture.id(), size))
                .sense(egui::Sense::click_and_drag());
            let response = ui.add(img);

            if self.radio.display.is_color() {
                if response.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
                }

                // Touch input on LCD — mirror the web reference:
                // mousedown/mousemove → simuTouchDown(x,y) continuously while held,
                // mouseup → simuTouchUp() on release.
                if response.is_pointer_button_down_on() {
                    if let Some(pos) = response.interact_pointer_pos() {
                        let rect = response.rect;
                        let scale_x = w as f32 / rect.width();
                        let scale_y = h as f32 / rect.height();
                        let x = ((pos.x - rect.min.x) * scale_x).clamp(0.0, (w - 1) as f32) as i32;
                        let y = ((pos.y - rect.min.y) * scale_y).clamp(0.0, (h - 1) as f32) as i32;
                        self.send(input::InputEvent::Touch { x, y, down: true });
                    }
                }
                // clicked() handles tap release (no drag), drag_stopped() handles drag release
                if response.drag_stopped() || response.clicked() {
                    self.send(input::InputEvent::Touch { x: 0, y: 0, down: false });
                }
            }

            // Mouse wheel for rotary encoder on LCD area
            // Use raw_scroll_delta for discrete per-notch events (not smooth/animated)
            let hover = response.hovered();
            if hover {
                ctx.input(|i| {
                    let scroll = i.raw_scroll_delta.y;
                    if scroll.abs() > 0.5 {
                        let delta = if scroll > 0.0 { 1 } else { -1 };
                        let _ = self.input_tx.send(input::InputEvent::Rotary(delta));
                    }
                });
            }
        } else {
            let w = self.radio.display.w as usize;
            let h = self.radio.display.h as usize;
            let black = vec![0u8; w * h * 4];
            let image = egui::ColorImage::from_rgba_unmultiplied([w, h], &black);
            let texture = self.lcd_texture.get_or_insert_with(|| {
                ctx.load_texture("lcd", image.clone(), egui::TextureOptions::NEAREST)
            });
            texture.set(image, egui::TextureOptions::NEAREST);
            let size = egui::vec2(w as f32 * self.lcd_scale, h as f32 * self.lcd_scale);
            let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
            ui.painter().image(
                texture.id(),
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
            ui.put(rect, egui::Spinner::new().size(24.0));
        }
    }

    /// Render a column of key buttons. Returns true on any press/release.
    fn show_keys(&mut self, ui: &mut egui::Ui, keys: &[radios::KeyDef]) {
        for key_def in keys {
            let raw_label = if key_def.label.is_empty() {
                &key_def.key
            } else {
                &key_def.label
            };
            let label = raw_label.replace('\u{23CE}', "").trim().to_uppercase();
            // Look up the key index from the script key mapping
            let index = input::script_key_index(&key_def.key).unwrap_or(-1);
            if index < 0 {
                continue;
            }
            let btn = egui::Button::new(&label).sense(egui::Sense::click_and_drag());
            let response = ui.add_sized(egui::vec2(80.0, 28.0), btn);
            if response.is_pointer_button_down_on() {
                self.key_pressed.insert(index);
                self.send(input::InputEvent::Key { index, pressed: true });
            } else if self.key_pressed.remove(&index) {
                self.send(input::InputEvent::Key { index, pressed: false });
            }
        }
    }

    /// Render a single switch as a horizontal row: name UP [MID] DN
    fn show_switch_widget(&mut self, ui: &mut egui::Ui, index: usize, sw: &radios::SwitchDef) {
        let is_3pos = sw.switch_type == "3POS" || sw.switch_type == "3pos";
        let current = self.switch_states[index];
        let name = &sw.name;
        let display_name = name.strip_prefix("Source").unwrap_or(name);

        ui.horizontal(|ui| {
            ui.label(display_name);
            if ui.selectable_label(current == -1, "UP").clicked() {
                self.switch_states[index] = -1;
                self.send(input::InputEvent::Switch { index: index as i32, state: -1 });
            }
            if is_3pos {
                if ui.selectable_label(current == 0, "MID").clicked() {
                    self.switch_states[index] = 0;
                    self.send(input::InputEvent::Switch { index: index as i32, state: 0 });
                }
            }
            if ui.selectable_label(current == 1, "DN").clicked() {
                self.switch_states[index] = 1;
                self.send(input::InputEvent::Switch { index: index as i32, state: 1 });
            }
        });
    }

    /// Render a custom switch (SW1-SW6) as a momentary push button.
    /// `cs_index` is the 0-based position within the custom_switches list,
    /// used to look up firmware-reported LED state.
    fn show_custom_switch_widget(&mut self, ui: &mut egui::Ui, index: usize, sw: &radios::SwitchDef, cs_index: usize) {
        let display_name = sw.name.strip_prefix("Source").unwrap_or(&sw.name);
        let is_pressed = self.switch_states[index] == 1;

        // Use firmware-reported LED color if available, otherwise fallback
        let fill_color = if let Some(led) = self.custom_switch_led_states.get(cs_index) {
            led.color
        } else if is_pressed {
            egui::Color32::from_rgb(56, 191, 249)
        } else {
            egui::Color32::from_gray(60)
        };

        ui.vertical(|ui| {
            ui.set_width(40.0);
            ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                // Colored momentary button
                let btn = egui::Button::new("")
                    .fill(fill_color)
                    .corner_radius(6.0);
                let resp = ui.add_sized(egui::vec2(36.0, 22.0), btn);

                // Momentary: pressed while pointer is down, released otherwise
                if resp.is_pointer_button_down_on() {
                    if !is_pressed {
                        self.switch_states[index] = 1;
                        self.send(input::InputEvent::Switch { index: index as i32, state: 1 });
                    }
                } else if is_pressed {
                    self.switch_states[index] = -1;
                    self.send(input::InputEvent::Switch { index: index as i32, state: -1 });
                }

                ui.label(display_name);
            });
        });
    }

    /// Render pot widgets inline (caller provides the horizontal context).
    fn show_pots_row_inner(&mut self, ui: &mut egui::Ui) {
        let inputs = self.radio.inputs.clone();
        {
            for (i, inp) in inputs.iter().enumerate() {
                if inp.input_type != "FLEX" {
                    continue;
                }
                let label = if inp.label.is_empty() { &inp.name } else { &inp.label };
                match inp.default.as_str() {
                    "POT" | "POT_CENTER" => {
                        ui.vertical(|ui| {
                            ui.label(label);
                            let mut val = self.analog_values[i] as f32;
                            let slider = egui::Slider::new(&mut val, 0.0..=4096.0)
                                .show_value(false);
                            if ui.add(slider).changed() {
                                let v = val as u16;
                                self.analog_values[i] = v;
                                self.send(input::InputEvent::Analog {
                                    index: i as i32,
                                    value: v,
                                });
                            }
                        });
                    }
                    "MULTIPOS" => {
                        ui.vertical(|ui| {
                            ui.label(label);
                            ui.horizontal(|ui| {
                                for pos in 0..6u16 {
                                    let btn_label = format!("{}", pos + 1);
                                    let selected = self.multipos_positions[i] == pos as usize;
                                    if ui.selectable_label(selected, btn_label).clicked() {
                                        self.multipos_positions[i] = pos as usize;
                                        let value = match pos {
                                            0 => 0u16,
                                            1 => 819,
                                            2 => 1638,
                                            3 => 2457,
                                            4 => 3276,
                                            5 => 4095,
                                            _ => 0,
                                        };
                                        self.analog_values[i] = value;
                                        self.send(input::InputEvent::Analog {
                                            index: i as i32,
                                            value,
                                        });
                                    }
                                }
                            });
                        });
                    }
                    _ => {} // NONE/empty hidden
                }
            }
        }
    }


    /// Render a single FLEX SLIDER input as a vertical slider.
    fn show_vertical_slider(&mut self, ui: &mut egui::Ui, index: usize, label: &str) {
        ui.vertical(|ui| {
            ui.label(label);
            let mut val = self.analog_values[index] as f32;
            let slider = egui::Slider::new(&mut val, 0.0..=4096.0)
                .vertical()
                .show_value(false);
            if ui.add_sized(egui::vec2(24.0, 100.0), slider).changed() {
                let v = val as u16;
                self.analog_values[index] = v;
                self.send(input::InputEvent::Analog {
                    index: index as i32,
                    value: v,
                });
            }
        });
    }

    /// Render a single trim button (+ or -).
    fn show_trim_button(&mut self, ui: &mut egui::Ui, trim_index: usize, is_plus: bool) {
        let idx = (trim_index as i32) * 2 + if is_plus { 1 } else { 0 };
        let label = if is_plus { "+" } else { "-" };
        let btn = egui::Button::new(label).sense(egui::Sense::click_and_drag());
        let resp = ui.add_sized(egui::vec2(24.0, 24.0), btn);
        if resp.is_pointer_button_down_on() {
            self.trim_pressed.insert(idx);
            self.send(input::InputEvent::Trim { index: idx, pressed: true });
        } else if self.trim_pressed.remove(&idx) {
            self.send(input::InputEvent::Trim { index: idx, pressed: false });
        }
    }

    /// Draw an interactive stick with optional integrated trims.
    /// `v_trim` — vertical trim index (rendered as a column on one side of the stick)
    /// `h_trim` — horizontal trim index (rendered as a row below the stick)
    /// `v_trim_on_left` — if true, vertical trim column is on the left side
    fn show_stick_with_trims(
        &mut self,
        ui: &mut egui::Ui,
        label: &str,
        stick_index: usize,
        v_trim: Option<usize>,
        h_trim: Option<usize>,
        v_trim_on_left: bool,
    ) {
        let stick_size = egui::vec2(100.0, 100.0);
        ui.vertical(|ui| {
            // Label — rendered after stick row measurement, but we need it on top.
            // Use previous frame's measured width for centering.
            let col_id = ui.id().with(label);
            let col_w: f32 = ui.ctx().data(|d| d.get_temp(col_id))
                .unwrap_or(stick_size.x + if v_trim.is_some() { 30.0 } else { 0.0 });
            let galley = ui.painter().layout_no_wrap(label.to_string(), egui::FontId::default(), egui::Color32::WHITE);
            let label_width = galley.size().x;
            let label_pad = ((col_w - label_width) / 2.0).max(0.0);
            ui.horizontal(|ui| {
                ui.add_space(label_pad);
                ui.label(label);
            });

            // Stick + vertical trim side by side
            let stick_row = ui.horizontal(|ui| {
                if v_trim_on_left {
                    if let Some(vt) = v_trim {
                        let trim_name = self.radio.trims.get(vt).map(|t| t.name.clone()).unwrap_or_default();
                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                let label_galley = ui.painter().layout_no_wrap(
                                    trim_name.clone(), egui::FontId::default(), egui::Color32::WHITE);
                                let label_w = label_galley.size().x;
                                let pad = ((24.0 - label_w) / 2.0).max(0.0);
                                ui.add_space(pad);
                                ui.label(&trim_name);
                            });
                            self.show_trim_button(ui, vt, true);
                            self.show_trim_button(ui, vt, false);
                        });
                    }
                }

                self.show_stick_inner(ui, stick_size, stick_index);

                if !v_trim_on_left {
                    if let Some(vt) = v_trim {
                        let trim_name = self.radio.trims.get(vt).map(|t| t.name.clone()).unwrap_or_default();
                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                let label_galley = ui.painter().layout_no_wrap(
                                    trim_name.clone(), egui::FontId::default(), egui::Color32::WHITE);
                                let label_w = label_galley.size().x;
                                let pad = ((24.0 - label_w) / 2.0).max(0.0);
                                ui.add_space(pad);
                                ui.label(&trim_name);
                            });
                            self.show_trim_button(ui, vt, true);
                            self.show_trim_button(ui, vt, false);
                        });
                    }
                }
            });
            // Store measured stick+trim row width for label centering next frame
            let measured_col = stick_row.response.rect.width();
            if measured_col > 0.0 {
                ui.ctx().data_mut(|d| d.insert_temp(col_id, measured_col));
            }

            // Horizontal trim below stick — centered under the stick canvas
            if let Some(ht) = h_trim {
                let trim_name = self.radio.trims.get(ht).map(|t| t.name.clone()).unwrap_or_default();
                let v_trim_col_w = col_w - stick_size.x;
                ui.horizontal(|ui| {
                    // Offset past v_trim column + center within stick width
                    if v_trim_on_left && v_trim.is_some() {
                        ui.add_space(v_trim_col_w + 10.0);
                    } else {
                        ui.add_space(10.0);
                    }
                    self.show_trim_button(ui, ht, false);
                    ui.label(&trim_name);
                    self.show_trim_button(ui, ht, true);
                });
            }
        });
    }

    /// Draw the stick canvas (shared by show_stick_with_trims).
    fn show_stick_inner(&mut self, ui: &mut egui::Ui, size: egui::Vec2, stick_index: usize) {
        let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
        }
        let painter = ui.painter_at(rect);

        // Handle drag input
        if response.is_pointer_button_down_on() {
            if let Some(pos) = response.interact_pointer_pos() {
                let nx = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
                let ny = 1.0 - ((pos.y - rect.top()) / rect.height()).clamp(0.0, 1.0);
                self.stick_positions[stick_index] = (nx, ny);

                let (x_idx, y_idx) = self.stick_analog_indices[stick_index];
                let x_val = (nx * 4096.0) as u16;
                let y_val = (ny * 4096.0) as u16;
                self.analog_values[x_idx] = x_val;
                self.analog_values[y_idx] = y_val;
                self.send(input::InputEvent::Analog { index: x_idx as i32, value: x_val });
                self.send(input::InputEvent::Analog { index: y_idx as i32, value: y_val });
            }
        }

        // Spring back to center on release
        if response.drag_stopped() || response.clicked() {
            self.stick_positions[stick_index] = (0.5, 0.5);
            let (x_idx, y_idx) = self.stick_analog_indices[stick_index];
            self.analog_values[x_idx] = 2048;
            self.analog_values[y_idx] = 2048;
            self.send(input::InputEvent::Analog { index: x_idx as i32, value: 2048 });
            self.send(input::InputEvent::Analog { index: y_idx as i32, value: 2048 });
        }

        // Background
        painter.rect_filled(rect, 4.0, egui::Color32::from_gray(40));
        painter.rect_stroke(rect, 4.0, egui::Stroke::new(1.0, egui::Color32::GRAY), egui::StrokeKind::Inside);

        // Crosshair
        let center = rect.center();
        painter.line_segment(
            [egui::pos2(rect.left(), center.y), egui::pos2(rect.right(), center.y)],
            egui::Stroke::new(1.0, egui::Color32::from_gray(80)),
        );
        painter.line_segment(
            [egui::pos2(center.x, rect.top()), egui::pos2(center.x, rect.bottom())],
            egui::Stroke::new(1.0, egui::Color32::from_gray(80)),
        );

        // Red dot at current stick position
        let (sx, sy) = self.stick_positions[stick_index];
        let dot_x = rect.left() + sx * rect.width();
        let dot_y = rect.top() + (1.0 - sy) * rect.height();
        painter.circle_filled(egui::pos2(dot_x, dot_y), 5.0, egui::Color32::RED);
    }
}

impl eframe::App for SimulatorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Receive latest LCD buffer
        while let Ok(lcd) = self.lcd_rx.try_recv() {
            self.last_lcd = Some(lcd);
        }

        // Receive latest custom switch LED states
        while let Ok(states) = self.cs_rx.try_recv() {
            self.custom_switch_led_states = states;
        }

        // Drain trace messages and cap at 200 lines
        while let Ok(msg) = self.trace_rx.try_recv() {
            self.trace_lines.push(msg);
        }
        if self.trace_lines.len() > 200 {
            let excess = self.trace_lines.len() - 200;
            self.trace_lines.drain(..excess);
        }

        // Handle keyboard input
        ctx.input(|i| {
            for event in &i.events {
                if let egui::Event::Key { key, pressed, .. } = event {
                    if let Some(idx) = input::egui_key_to_index(key) {
                        let _ = self.input_tx.send(input::InputEvent::Key {
                            index: idx,
                            pressed: *pressed,
                        });
                    }
                }
            }
        });

        // Split keys into left and right
        let left_keys: Vec<radios::KeyDef> = self
            .radio
            .keys
            .iter()
            .filter(|k| k.side != "R")
            .cloned()
            .collect();
        let right_keys: Vec<radios::KeyDef> = self
            .radio
            .keys
            .iter()
            .filter(|k| k.side == "R")
            .cloned()
            .collect();

        // Split visible toggle switches (non-NONE, non-SW) into left and right halves
        let visible_switches: Vec<usize> = self.radio.switches.iter().enumerate()
            .filter(|(_, sw)| sw.default != "NONE" && !sw.name.starts_with("SW"))
            .map(|(i, _)| i)
            .collect();

        // Custom switches (SW1-SW6): momentary push buttons
        let custom_switches: Vec<usize> = self.radio.switches.iter().enumerate()
            .filter(|(_, sw)| sw.default != "NONE" && sw.name.starts_with("SW"))
            .map(|(i, _)| i)
            .collect();
        let left_switch_indices: Vec<usize> = visible_switches[..visible_switches.len() / 2].to_vec();
        let right_switch_indices: Vec<usize> = visible_switches[visible_switches.len() / 2..].to_vec();

        // Collect SLIDER-type FLEX inputs for vertical slider columns
        let sliders: Vec<(usize, String)> = self
            .radio
            .inputs
            .iter()
            .enumerate()
            .filter(|(_, inp)| inp.input_type == "FLEX" && inp.default == "SLIDER")
            .map(|(i, inp)| {
                let label = if inp.label.is_empty() { inp.name.clone() } else { inp.label.clone() };
                (i, label)
            })
            .collect();
        let left_sliders_count = sliders.len() / 2;

        // Compute dynamic content width
        let switch_w = 140.0_f32;
        let slider_w = 40.0_f32;
        let stick_w = 140.0_f32;
        let right_sliders_count = sliders.len() - left_sliders_count;
        let controls_w = switch_w * 2.0
            + left_sliders_count as f32 * slider_w
            + right_sliders_count as f32 * slider_w
            + stick_w * 2.0
            + 92.0;
        let lcd_row_w = self.radio.display.w as f32 * self.lcd_scale + 196.0;
        let content_w = controls_w.max(lcd_row_w);

        // Trace output panel pinned to window bottom
        egui::TopBottomPanel::bottom("trace_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                let arrow = if self.trace_open { "\u{25BC}" } else { "\u{25B6}" };
                if ui.button(egui::RichText::new(format!("{} Console Output", arrow)).strong()).clicked() {
                    self.trace_open = !self.trace_open;
                    let (_base_w, base_h) = self.window_size;
                    let current_w = ctx.screen_rect().width();
                    let new_h = if self.trace_open {
                        base_h + self.trace_panel_height
                    } else {
                        base_h + 30.0
                    };
                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(current_w, new_h)));
                }
            });
            if self.trace_open {
                egui::ScrollArea::vertical()
                    .min_scrolled_height(350.0)
                    .max_height(350.0)
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        for line in &self.trace_lines {
                            ui.horizontal(|ui| {
                                ui.add_space(100.0);
                                ui.label(egui::RichText::new(line).monospace().size(11.0));
                            });
                        }
                    });
            }
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                // Helper: center a horizontal row by adding left padding
                let avail_w = ui.available_width();
                let center_row = |ui: &mut egui::Ui, row_w: f32| {
                    let pad = ((avail_w - row_w) / 2.0).max(0.0);
                    ui.add_space(pad);
                };

                // Logo
                ui.add_space(24.0);
                ui.vertical_centered(|ui| {
                    ui.add(
                        egui::Image::new(egui::include_image!("../assets/images/edgetx.svg"))
                            .max_width(140.0),
                    );
                });
                ui.add_space(24.0);

                // Row 1: Left Keys | LCD | Right Keys (keys vertically centered on LCD)
                let lcd_h = self.radio.display.h as f32 * self.lcd_scale;
                let lcd_row_w = self.radio.display.w as f32 * self.lcd_scale + 196.0;
                ui.horizontal(|ui| {
                    center_row(ui, lcd_row_w);

                    // Left keys — vertically centered
                    let left_key_count = left_keys.len() as f32;
                    let left_keys_h = left_key_count * 28.0 + (left_key_count - 1.0).max(0.0) * 8.0;
                    let left_pad = ((lcd_h - left_keys_h) / 2.0).max(0.0);
                    ui.vertical(|ui| {
                        ui.add_space(left_pad);
                        self.show_keys(ui, &left_keys);
                    });

                    ui.vertical(|ui| {
                        self.show_lcd(ui, ctx);
                    });

                    // Right keys — vertically centered
                    let right_key_count = right_keys.len() as f32;
                    let right_keys_h = right_key_count * 28.0 + (right_key_count - 1.0).max(0.0) * 8.0;
                    let right_pad = ((lcd_h - right_keys_h) / 2.0).max(0.0);
                    ui.vertical(|ui| {
                        ui.add_space(right_pad);
                        self.show_keys(ui, &right_keys);
                    });
                });

                ui.add_space(16.0);

                // Row 2: Pots — estimate width for centering
                let inputs = self.radio.inputs.clone();
                let pot_count = inputs.iter().filter(|inp| {
                    inp.input_type == "FLEX"
                        && matches!(inp.default.as_str(), "POT" | "POT_CENTER" | "MULTIPOS")
                }).count();
                if pot_count > 0 {
                    // Measure pots width from previous frame, default to estimate
                    let pots_id = ui.id().with("pots_row_w");
                    let pots_w: f32 = ui.ctx().data(|d| d.get_temp(pots_id)).unwrap_or(pot_count as f32 * 150.0);
                    let measured = ui.horizontal(|ui| {
                        center_row(ui, pots_w);
                        let start_x = ui.cursor().left();
                        self.show_pots_row_inner(ui);
                        ui.cursor().left() - start_x
                    }).inner;
                    if measured > 0.0 {
                        ui.ctx().data_mut(|d| d.insert_temp(pots_id, measured));
                    }
                }

                // Custom switches row (SW1-SW6): momentary push buttons
                if !custom_switches.is_empty() {
                    ui.add_space(8.0);
                    let cs_id = ui.id().with("custom_switches_w");
                    let cs_w: f32 = ui.ctx().data(|d| d.get_temp(cs_id))
                        .unwrap_or(custom_switches.len() as f32 * 48.0);
                    let measured_cs = ui.horizontal(|ui| {
                        center_row(ui, cs_w);
                        let start_x = ui.cursor().left();
                        let switches = self.radio.switches.clone();
                        for (cs_index, &i) in custom_switches.iter().enumerate() {
                            self.show_custom_switch_widget(ui, i, &switches[i], cs_index);
                        }
                        ui.cursor().left() - start_x
                    }).inner;
                    if measured_cs > 0.0 {
                        ui.ctx().data_mut(|d| d.insert_temp(cs_id, measured_cs));
                    }
                }

                ui.add_space(16.0);

                // Compute trim assignments for sticks
                let trim_count = self.radio.trims.len();
                let left_v_trim = if trim_count >= 2 { Some(1) } else { None };
                let left_h_trim = if trim_count >= 2 { Some(0) } else { None };
                let right_v_trim = if trim_count >= 4 { Some(2) } else { None };
                let right_h_trim = if trim_count >= 4 { Some(3) } else { None };

                ui.add_space(8.0);

                // Row 3: Left Switches | Left Sliders | Left Stick+Trims | Right Stick+Trims | Right Sliders | Right Switches
                // Measure inner controls width (LS+sticks+RS) from previous frame for centering
                let inner_id = ui.id().with("inner_controls_w");
                let inner_w: f32 = ui.ctx().data(|d| d.get_temp(inner_id)).unwrap_or(400.0);
                // Center inner controls in window: left_pad positions so inner starts at (avail-inner)/2
                let left_pad_for_inner = ((avail_w - inner_w) / 2.0 - switch_w - 16.0).max(0.0);
                let measured_inner = ui.horizontal(|ui| {
                    ui.add_space(left_pad_for_inner);

                    // Left switches
                    ui.vertical(|ui| {
                        ui.add_space(24.0);
                        let switches = self.radio.switches.clone();
                        for &i in &left_switch_indices {
                            self.show_switch_widget(ui, i, &switches[i]);
                        }
                    });

                    ui.add_space(16.0);

                    // Inner controls: measure start
                    let start_x = ui.cursor().left();

                    // Left vertical sliders
                    for &(idx, ref label) in sliders.iter().take(left_sliders_count) {
                        self.show_vertical_slider(ui, idx, label);
                    }

                    // Left stick with trims (vertical trim on left side)
                    self.show_stick_with_trims(ui, "LEFT STICK", 0, left_v_trim, left_h_trim, true);

                    // Right stick with trims (vertical trim on right side)
                    self.show_stick_with_trims(ui, "RIGHT STICK", 1, right_v_trim, right_h_trim, false);

                    // Right vertical sliders
                    for &(idx, ref label) in sliders.iter().skip(left_sliders_count) {
                        self.show_vertical_slider(ui, idx, label);
                    }

                    // Inner controls: measure end
                    let end_x = ui.cursor().left();

                    ui.add_space(16.0);

                    // Right switches
                    ui.vertical(|ui| {
                        ui.add_space(24.0);
                        let switches = self.radio.switches.clone();
                        for &i in &right_switch_indices {
                            self.show_switch_widget(ui, i, &switches[i]);
                        }
                    });

                    end_x - start_x
                }).inner;
                if measured_inner > 0.0 {
                    ui.ctx().data_mut(|d| d.insert_temp(inner_id, measured_inner));
                }

                ui.add_space(16.0);

                // Row 4: Extra trims (index 4+) — only if more than 4 trims
                if trim_count > 4 {
                    let trims_id = ui.id().with("extra_trims_w");
                    let trims_w: f32 = ui.ctx().data(|d| d.get_temp(trims_id)).unwrap_or(200.0);
                    // Center relative to inner controls (same center as sticks)
                    let trims_pad = ((avail_w - inner_w) / 2.0 + (inner_w - trims_w) / 2.0).max(0.0);
                    let measured_trims = ui.horizontal(|ui| {
                        ui.add_space(trims_pad);
                        let start_x = ui.cursor().left();
                        for i in 4..trim_count {
                            let trim_name = self.radio.trims[i].name.clone();
                            self.show_trim_button(ui, i, false);
                            ui.label(&trim_name);
                            self.show_trim_button(ui, i, true);
                            ui.add_space(24.0);
                        }
                        ui.cursor().left() - start_x
                    }).inner;
                    if measured_trims > 0.0 {
                        ui.ctx().data_mut(|d| d.insert_temp(trims_id, measured_trims));
                    }
                }
            });
        });

        // Request continuous repaint
        ctx.request_repaint();
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        let _ = self.input_tx.send(input::InputEvent::Quit);
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
            let _ = signal_hook_simple();
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
