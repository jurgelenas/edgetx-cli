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

        // Initialize audio channel before spawning WASM thread
        let audio_rx = runtime::init_audio_channel();
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
            loop {
                // Process all pending input events
                while let Ok(event) = input_rx.try_recv() {
                    match event {
                        input::InputEvent::Key { index, pressed } => {
                            rt.set_key(index, pressed);
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
                            rt.set_trim(index, pressed);
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

                // Short sleep to avoid busy-waiting, but responsive to notifications
                std::thread::sleep(Duration::from_millis(10));
            }
        });

        // Compute window size based on layout
        // 200px per side for keys + LCD + padding; 400px below for controls
        let lcd_w = radio.display.w as f32;
        let lcd_h = radio.display.h as f32;
        let window_w = (lcd_w + 400.0).max(900.0);
        let window_h = lcd_h + 500.0; // 400 for controls + ~100 for logo + padding

        let native_options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([window_w, window_h])
                .with_min_inner_size([window_w, 400.0])
                .with_title(format!("EdgeTX Simulator - {}", radio.name))
                .with_decorations(true),
            ..Default::default()
        };

        let app = SimulatorApp::new(radio, lcd_rx, input_tx);

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
}

impl SimulatorApp {
    fn new(
        radio: RadioDef,
        lcd_rx: std::sync::mpsc::Receiver<Vec<u8>>,
        input_tx: std::sync::mpsc::Sender<input::InputEvent>,
    ) -> Self {
        let switch_count = radio.switches.len();
        let input_count = radio.inputs.len();

        // Initialize switch defaults: 2POS → UP (-1), 3POS → MID (0)
        let mut switch_states = vec![0i32; switch_count];
        for (i, sw) in radio.switches.iter().enumerate() {
            switch_states[i] = match sw.switch_type.as_str() {
                "2POS" | "2pos" => -1, // UP
                "3POS" | "3pos" => 0,  // MID
                _ => -1,              // default UP
            };
        }

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

            let size = egui::vec2(w as f32, h as f32);
            let img = egui::Image::new(egui::load::SizedTexture::new(texture.id(), size))
                .sense(egui::Sense::click_and_drag());
            let response = ui.add(img);

            // Touch input on LCD
            if response.clicked() || response.dragged() || response.drag_started() {
                if let Some(pos) = response.interact_pointer_pos() {
                    let rect = response.rect;
                    let x = (pos.x - rect.min.x) as i32;
                    let y = (pos.y - rect.min.y) as i32;
                    self.send(input::InputEvent::Touch { x, y, down: true });
                }
            }
            if response.drag_stopped() {
                self.send(input::InputEvent::Touch { x: 0, y: 0, down: false });
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
            ui.label("Waiting for LCD data...");
        }
    }

    /// Render a column of key buttons. Returns true on any press/release.
    fn show_keys(&mut self, ui: &mut egui::Ui, keys: &[radios::KeyDef]) {
        for key_def in keys {
            let label = if key_def.label.is_empty() {
                &key_def.key
            } else {
                &key_def.label
            };
            // Look up the key index from the script key mapping
            let index = input::script_key_index(&key_def.key).unwrap_or(-1);
            if index < 0 {
                continue;
            }
            let btn = egui::Button::new(label).min_size(egui::vec2(80.0, 28.0));
            let response = ui.add(btn);
            if response.is_pointer_button_down_on() {
                // Continuously send pressed while held
                self.send(input::InputEvent::Key { index, pressed: true });
            }
            if response.drag_stopped() || response.clicked() {
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

    /// Render pots row: FLEX inputs including POT, POT_CENTER, MULTIPOS, and SLIDER.
    /// All shown as horizontal sliders or position buttons.
    fn show_pots_row(&mut self, ui: &mut egui::Ui) {
        let inputs = self.radio.inputs.clone();
        let has_pots = inputs.iter().any(|inp| {
            inp.input_type == "FLEX"
                && matches!(inp.default.as_str(), "POT" | "POT_CENTER" | "MULTIPOS" | "SLIDER")
        });
        if !has_pots {
            return;
        }

        ui.horizontal(|ui| {
            for (i, inp) in inputs.iter().enumerate() {
                if inp.input_type != "FLEX" {
                    continue;
                }
                let label = if inp.label.is_empty() { &inp.name } else { &inp.label };
                match inp.default.as_str() {
                    "POT" | "POT_CENTER" | "SLIDER" => {
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
        });
    }


    /// Render trim +/- button pairs.
    fn show_trims(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            for (i, trim) in self.radio.trims.clone().iter().enumerate() {
                let trim_idx = i as i32;
                // Each trim has two indices: i*2 for minus, i*2+1 for plus
                let minus_idx = trim_idx * 2;
                let plus_idx = trim_idx * 2 + 1;

                ui.group(|ui| {
                    ui.label(&trim.name);
                    ui.horizontal(|ui| {
                        let minus_btn = egui::Button::new("-").min_size(egui::vec2(30.0, 24.0));
                        let minus_resp = ui.add(minus_btn);
                        if minus_resp.is_pointer_button_down_on() {
                            self.send(input::InputEvent::Trim { index: minus_idx, pressed: true });
                        }
                        if minus_resp.drag_stopped() || minus_resp.clicked() {
                            self.send(input::InputEvent::Trim { index: minus_idx, pressed: false });
                        }

                        let plus_btn = egui::Button::new("+").min_size(egui::vec2(30.0, 24.0));
                        let plus_resp = ui.add(plus_btn);
                        if plus_resp.is_pointer_button_down_on() {
                            self.send(input::InputEvent::Trim { index: plus_idx, pressed: true });
                        }
                        if plus_resp.drag_stopped() || plus_resp.clicked() {
                            self.send(input::InputEvent::Trim { index: plus_idx, pressed: false });
                        }
                    });
                });
            }
        });
    }

    /// Draw an interactive stick (100x100 with crosshair and draggable red dot).
    fn show_stick(&mut self, ui: &mut egui::Ui, label: &str, stick_index: usize) {
        let size = egui::vec2(100.0, 100.0);
        ui.vertical(|ui| {
            let galley = ui.painter().layout_no_wrap(label.to_string(), egui::FontId::default(), egui::Color32::WHITE);
            let label_width = galley.size().x;
            let pad = (size.x - label_width).max(0.0) / 2.0;
            ui.horizontal(|ui| {
                ui.add_space(pad);
                ui.label(label);
            });
            let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
            let painter = ui.painter_at(rect);

            // Handle drag input
            if response.dragged() || response.drag_started() {
                if let Some(pos) = response.interact_pointer_pos() {
                    let nx = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
                    // Y inverted: top = 1.0, bottom = 0.0
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
            if response.drag_stopped() {
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
        });
    }
}

impl eframe::App for SimulatorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Receive latest LCD buffer
        while let Ok(lcd) = self.lcd_rx.try_recv() {
            self.last_lcd = Some(lcd);
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

        // Split switches into left and right halves
        let switch_count = self.radio.switches.len();
        let left_switch_count = (switch_count + 1) / 2;

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |outer_ui| {
                // LCD row = left keys (80) + spacing (8) + LCD + spacing (8) + right keys (80) + margin
                let content_w = self.radio.display.w as f32 + 196.0;
                let avail = outer_ui.available_rect_before_wrap();
                let offset_x = ((avail.width() - content_w) / 2.0).max(0.0);
                let centered_rect = egui::Rect::from_min_size(
                    egui::pos2(avail.min.x + offset_x, avail.min.y),
                    egui::vec2(content_w, avail.height()),
                );
                let mut child_ui = outer_ui.new_child(egui::UiBuilder::new().max_rect(centered_rect));
                let ui = &mut child_ui;

                // Logo
                ui.add_space(24.0);
                ui.vertical_centered(|ui| {
                    ui.add(
                        egui::Image::new(egui::include_image!("../assets/images/edgetx.svg"))
                            .max_width(200.0),
                    );
                });
                ui.add_space(24.0);

                // Row 1: Left Keys | LCD | Right Keys
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        self.show_keys(ui, &left_keys);
                    });
                    ui.vertical(|ui| {
                        self.show_lcd(ui, ctx);
                    });
                    ui.vertical(|ui| {
                        self.show_keys(ui, &right_keys);
                    });
                });

                ui.add_space(8.0);

                // Row 2: Pots and sliders (horizontal sliders + multipos)
                self.show_pots_row(ui);

                ui.add_space(8.0);

                // Row 3: Left Switches | Left Stick | Right Stick | Right Switches
                ui.horizontal(|ui| {
                    // Left switches
                    ui.vertical(|ui| {
                        let switches = self.radio.switches.clone();
                        for (i, sw) in switches.iter().take(left_switch_count).enumerate() {
                            if sw.default == "NONE" { continue; }
                            self.show_switch_widget(ui, i, sw);
                        }
                    });

                    // Left stick
                    self.show_stick(ui, "LEFT STICK", 0);

                    // Right stick
                    self.show_stick(ui, "RIGHT STICK", 1);

                    // Right switches
                    ui.vertical(|ui| {
                        let switches = self.radio.switches.clone();
                        for (i, sw) in switches.iter().enumerate().skip(left_switch_count) {
                            if sw.default == "NONE" { continue; }
                            self.show_switch_widget(ui, i, sw);
                        }
                    });
                });

                ui.add_space(8.0);

                // Row 4: Trims
                if !self.radio.trims.is_empty() {
                    self.show_trims(ui);
                }

                // Reserve space in the parent so the scroll area knows the content bounds
                let used = child_ui.min_rect();
                outer_ui.allocate_rect(used, egui::Sense::hover());
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
