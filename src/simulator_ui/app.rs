use std::path::PathBuf;

use egui_toast::{Toast, ToastKind, ToastOptions, Toasts};

use crate::radio_catalog::{KeyDef, RadioDef, SwitchDef};
use crate::simulator::framebuffer;
use crate::simulator::input::{InputEvent, RuntimeMessage};
use crate::simulator::runtime::{self, GVarValue};
use crate::simulator::screenshot;

use super::audio::AudioPlayer;
use super::input::egui_key_to_index;

/// Firmware-reported custom switch LED color, received from WASM thread.
pub(crate) struct CustomSwitchState {
    pub color: egui::Color32,
}

const VOLUME_LEVEL_MAX: i32 = 23;

/// Bundled firmware state sent from WASM thread to UI each poll cycle.
pub(crate) struct FirmwareState {
    pub custom_switches: Vec<CustomSwitchState>,
    pub volume: i32,
    /// Whether monitor data is populated (only when monitors tab is active).
    pub monitors_active: bool,
    pub logical_switches: Vec<bool>,
    pub channel_outputs: Vec<i16>,
    pub mix_outputs: Vec<i16>,
    pub channels_used: u32,
    pub gvars: Vec<Vec<GVarValue>>,
    pub num_gvars: u8,
    pub num_flight_modes: u8,
}

/// Which tab is active in the bottom panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BottomTab {
    Console,
    Monitors,
}

pub struct SimulatorApp {
    radio: RadioDef,
    lcd_rx: std::sync::mpsc::Receiver<Vec<u8>>,
    input_tx: std::sync::mpsc::Sender<RuntimeMessage>,
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
    state_rx: std::sync::mpsc::Receiver<FirmwareState>,
    audio_player: AudioPlayer,
    audio_rx: std::sync::mpsc::Receiver<Vec<i16>>,
    firmware_volume: i32,
    muted: bool,
    /// Scale factor for LCD display rendering (>1 for small/BW displays).
    lcd_scale: f32,
    /// Receiver for trace messages from the WASM firmware.
    trace_rx: std::sync::mpsc::Receiver<String>,
    /// Ring buffer of trace output lines (max 200).
    trace_lines: Vec<String>,
    /// Whether the trace output panel is expanded.
    trace_open: bool,
    /// Base window dimensions (without trace panel).
    pub window_size: (f32, f32),
    /// Height of the trace panel when expanded.
    pub trace_panel_height: f32,
    /// Directory for saving screenshots (SD card SCREENSHOTS folder).
    screenshots_dir: PathBuf,
    /// Toast notification manager.
    toasts: Toasts,
    /// Active tab in the bottom panel.
    bottom_tab: BottomTab,
    /// Latest logical switch states from firmware.
    logical_switches: Vec<bool>,
    /// Latest channel output values from firmware.
    channel_outputs: Vec<i16>,
    /// Latest mixer output values from firmware.
    mix_outputs: Vec<i16>,
    /// Bitmask of channels in use.
    channels_used: u32,
    /// GVar values: [gvar_idx][flight_mode].
    gvars: Vec<Vec<GVarValue>>,
    num_gvars: u8,
    num_flight_modes: u8,
    /// Toggle visibility of monitor sub-sections.
    monitors_show_ls: bool,
    monitors_show_ch: bool,
    monitors_show_gv: bool,
    /// Whether we have sent MonitorsPoll(true) to the WASM thread.
    monitors_poll_sent: bool,
}

impl SimulatorApp {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        radio: RadioDef,
        lcd_rx: std::sync::mpsc::Receiver<Vec<u8>>,
        input_tx: std::sync::mpsc::Sender<RuntimeMessage>,
        state_rx: std::sync::mpsc::Receiver<FirmwareState>,
        audio_player: AudioPlayer,
        audio_rx: std::sync::mpsc::Receiver<Vec<i16>>,
        trace_rx: std::sync::mpsc::Receiver<String>,
        sdcard_dir: PathBuf,
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
            state_rx,
            audio_player,
            audio_rx,
            firmware_volume: VOLUME_LEVEL_MAX,
            muted: false,
            lcd_scale,
            trace_rx,
            trace_lines: Vec::new(),
            trace_open: false,
            window_size: (0.0, 0.0),
            trace_panel_height: 380.0,
            screenshots_dir: sdcard_dir.join("SCREENSHOTS"),
            toasts: Toasts::new()
                .anchor(egui::Align2::CENTER_BOTTOM, (0.0, -10.0))
                .direction(egui::Direction::BottomUp),
            bottom_tab: BottomTab::Console,
            logical_switches: Vec::new(),
            channel_outputs: Vec::new(),
            mix_outputs: Vec::new(),
            channels_used: 0,
            gvars: Vec::new(),
            num_gvars: 0,
            num_flight_modes: 0,
            monitors_show_ls: true,
            monitors_show_ch: true,
            monitors_show_gv: true,
            monitors_poll_sent: false,
        }
    }

    fn send(&self, msg: impl Into<RuntimeMessage>) {
        let _ = self.input_tx.send(msg.into());
    }

    /// Save the current LCD buffer as a PNG screenshot to the SD card SCREENSHOTS folder.
    fn take_screenshot(&mut self) {
        if let Some(ref lcd_data) = self.last_lcd {
            let rgba = framebuffer::decode(lcd_data, &self.radio.display);
            let w = self.radio.display.w as u32;
            let h = self.radio.display.h as u32;
            if let Err(e) = std::fs::create_dir_all(&self.screenshots_dir) {
                self.toasts.add(Toast {
                    text: format!("Failed to create screenshots dir: {e}").into(),
                    kind: ToastKind::Error,
                    options: ToastOptions::default()
                        .duration_in_seconds(5.0)
                        .show_progress(true),
                    ..Default::default()
                });
                return;
            }
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let path = self
                .screenshots_dir
                .join(format!("screenshot_{timestamp}.png"));
            let path_str = path.to_string_lossy();
            match screenshot::save_screenshot(&path_str, &rgba, w, h) {
                Ok(()) => {
                    let filename = path.file_name().unwrap_or_default().to_string_lossy();
                    self.toasts.add(Toast {
                        text: format!("Screenshot saved: {filename}").into(),
                        kind: ToastKind::Success,
                        options: ToastOptions::default()
                            .duration_in_seconds(3.0)
                            .show_progress(true),
                        ..Default::default()
                    });
                }
                Err(e) => {
                    self.toasts.add(Toast {
                        text: format!("Screenshot failed: {e}").into(),
                        kind: ToastKind::Error,
                        options: ToastOptions::default()
                            .duration_in_seconds(5.0)
                            .show_progress(true),
                        ..Default::default()
                    });
                }
            }
        }
    }

    /// Render the LCD display with touch support and mouse-wheel rotary encoder.
    fn show_lcd(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if let Some(ref lcd_data) = self.last_lcd {
            let (rgba, tex_w, tex_h) = if self.radio.display.is_color() {
                let decoded = framebuffer::decode(lcd_data, &self.radio.display);
                let w = self.radio.display.w as usize;
                let h = self.radio.display.h as usize;
                (decoded, w, h)
            } else {
                framebuffer::decode_lcd(lcd_data, &self.radio.display, self.lcd_scale as usize)
            };

            let image = egui::ColorImage::from_rgba_unmultiplied([tex_w, tex_h], &rgba);

            let texture = self.lcd_texture.get_or_insert_with(|| {
                ctx.load_texture("lcd", image.clone(), egui::TextureOptions::NEAREST)
            });
            texture.set(image, egui::TextureOptions::NEAREST);

            let display_size = if self.radio.display.is_color() {
                egui::vec2(tex_w as f32 * self.lcd_scale, tex_h as f32 * self.lcd_scale)
            } else {
                egui::vec2(tex_w as f32, tex_h as f32)
            };
            let img = egui::Image::new(egui::load::SizedTexture::new(texture.id(), display_size))
                .sense(egui::Sense::click_and_drag());
            let response = ui.add(img);

            if self.radio.display.is_color() {
                let w = self.radio.display.w as usize;
                let h = self.radio.display.h as usize;

                if response.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
                }

                // Touch input on LCD — mirror the web reference:
                // mousedown/mousemove → simuTouchDown(x,y) continuously while held,
                // mouseup → simuTouchUp() on release.
                if response.is_pointer_button_down_on()
                    && let Some(pos) = response.interact_pointer_pos()
                {
                    let rect = response.rect;
                    let scale_x = w as f32 / rect.width();
                    let scale_y = h as f32 / rect.height();
                    let x = ((pos.x - rect.min.x) * scale_x).clamp(0.0, (w - 1) as f32) as i32;
                    let y = ((pos.y - rect.min.y) * scale_y).clamp(0.0, (h - 1) as f32) as i32;
                    self.send(InputEvent::Touch { x, y, down: true });
                }
                // clicked() handles tap release (no drag), drag_stopped() handles drag release
                if response.drag_stopped() || response.clicked() {
                    self.send(InputEvent::Touch {
                        x: 0,
                        y: 0,
                        down: false,
                    });
                }
            }

            // Mouse wheel for rotary encoder on LCD area
            // Use raw_scroll_delta for discrete per-notch events (not smooth/animated)
            let hover = response.hovered();
            if hover {
                ctx.input(|i| {
                    let scroll = i.raw_scroll_delta.y;
                    if scroll.abs() > 0.5 {
                        let delta = if scroll > 0.0 { -1 } else { 1 };
                        self.send(InputEvent::Rotary(delta));
                    }
                });
            }
        } else {
            let scale = self.lcd_scale as usize;
            let w = self.radio.display.w as usize * scale;
            let h = self.radio.display.h as usize * scale;
            let black = vec![0u8; w * h * 4];
            let image = egui::ColorImage::from_rgba_unmultiplied([w, h], &black);
            let texture = self.lcd_texture.get_or_insert_with(|| {
                ctx.load_texture("lcd", image.clone(), egui::TextureOptions::NEAREST)
            });
            texture.set(image, egui::TextureOptions::NEAREST);
            let size = egui::vec2(w as f32, h as f32);
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
    fn show_keys(&mut self, ui: &mut egui::Ui, keys: &[KeyDef]) {
        for key_def in keys {
            let raw_label = if key_def.label.is_empty() {
                &key_def.key
            } else {
                &key_def.label
            };
            let label = raw_label.replace('\u{23CE}', "").trim().to_uppercase();
            // Look up the key index from the script key mapping
            let index = crate::simulator::input::script_key_index(&key_def.key).unwrap_or(-1);
            if index < 0 {
                continue;
            }
            let btn = egui::Button::new(&label).sense(egui::Sense::click_and_drag());
            let response = ui.add_sized(egui::vec2(80.0, 28.0), btn);
            if response.is_pointer_button_down_on() {
                self.key_pressed.insert(index);
                self.send(InputEvent::Key {
                    index,
                    pressed: true,
                });
            } else if self.key_pressed.remove(&index) {
                self.send(InputEvent::Key {
                    index,
                    pressed: false,
                });
            }
        }
    }

    /// Render a single switch as a horizontal row: name UP [MID] DN
    fn show_switch_widget(&mut self, ui: &mut egui::Ui, index: usize, sw: &SwitchDef) {
        let is_3pos = sw.switch_type == "3POS" || sw.switch_type == "3pos";
        let current = self.switch_states[index];
        let name = &sw.name;
        let display_name = name.strip_prefix("Source").unwrap_or(name);

        ui.horizontal(|ui| {
            ui.label(display_name);
            if ui.selectable_label(current == -1, "UP").clicked() {
                self.switch_states[index] = -1;
                self.send(InputEvent::Switch {
                    index: index as i32,
                    state: -1,
                });
            }
            if is_3pos && ui.selectable_label(current == 0, "MID").clicked() {
                self.switch_states[index] = 0;
                self.send(InputEvent::Switch {
                    index: index as i32,
                    state: 0,
                });
            }
            if ui.selectable_label(current == 1, "DN").clicked() {
                self.switch_states[index] = 1;
                self.send(InputEvent::Switch {
                    index: index as i32,
                    state: 1,
                });
            }
        });
    }

    /// Render a custom switch (SW1-SW6) as a momentary push button.
    /// `cs_index` is the 0-based position within the custom_switches list,
    /// used to look up firmware-reported LED state.
    fn show_custom_switch_widget(
        &mut self,
        ui: &mut egui::Ui,
        index: usize,
        sw: &SwitchDef,
        cs_index: usize,
    ) {
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
                let btn = egui::Button::new("").fill(fill_color).corner_radius(6.0);
                let resp = ui.add_sized(egui::vec2(36.0, 22.0), btn);

                // Momentary: pressed while pointer is down, released otherwise
                if resp.is_pointer_button_down_on() {
                    if !is_pressed {
                        self.switch_states[index] = 1;
                        self.send(InputEvent::Switch {
                            index: index as i32,
                            state: 1,
                        });
                    }
                } else if is_pressed {
                    self.switch_states[index] = -1;
                    self.send(InputEvent::Switch {
                        index: index as i32,
                        state: -1,
                    });
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
                let label = if inp.label.is_empty() {
                    &inp.name
                } else {
                    &inp.label
                };
                match inp.default.as_str() {
                    "POT" | "POT_CENTER" => {
                        ui.add_space(10.0);
                        ui.vertical(|ui| {
                            let mut val = self.analog_values[i] as f32;
                            let slider =
                                egui::Slider::new(&mut val, 0.0..=4096.0).show_value(false);
                            let resp = ui.add(slider);
                            let col_width = resp.rect.width();
                            ui.allocate_ui_with_layout(
                                egui::vec2(col_width, ui.spacing().interact_size.y),
                                egui::Layout::top_down(egui::Align::Center),
                                |ui| {
                                    ui.label(label);
                                },
                            );
                            if resp.changed() {
                                let v = val as u16;
                                self.analog_values[i] = v;
                                self.send(InputEvent::Analog {
                                    index: i as i32,
                                    value: v,
                                });
                            }
                        });
                        ui.add_space(10.0);
                    }
                    "MULTIPOS" => {
                        ui.add_space(18.0);
                        ui.vertical(|ui| {
                            let buttons_resp = ui.horizontal(|ui| {
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
                                        self.send(InputEvent::Analog {
                                            index: i as i32,
                                            value,
                                        });
                                    }
                                }
                            });
                            let col_width = buttons_resp.response.rect.width();
                            ui.allocate_ui_with_layout(
                                egui::vec2(col_width, ui.spacing().interact_size.y),
                                egui::Layout::top_down(egui::Align::Center),
                                |ui| {
                                    ui.label(label);
                                },
                            );
                        });
                        ui.add_space(18.0);
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
                self.send(InputEvent::Analog {
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
            self.send(InputEvent::Trim {
                index: idx,
                pressed: true,
            });
        } else if self.trim_pressed.remove(&idx) {
            self.send(InputEvent::Trim {
                index: idx,
                pressed: false,
            });
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
            let col_w: f32 = ui
                .ctx()
                .data(|d| d.get_temp(col_id))
                .unwrap_or(stick_size.x + if v_trim.is_some() { 30.0 } else { 0.0 });
            let galley = ui.painter().layout_no_wrap(
                label.to_string(),
                egui::FontId::default(),
                egui::Color32::WHITE,
            );
            let label_width = galley.size().x;
            let label_pad = ((col_w - label_width) / 2.0).max(0.0);
            ui.horizontal(|ui| {
                ui.add_space(label_pad);
                ui.label(label);
            });

            // Stick + vertical trim side by side
            let stick_row = ui.horizontal(|ui| {
                if v_trim_on_left && let Some(vt) = v_trim {
                    let trim_name = self
                        .radio
                        .trims
                        .get(vt)
                        .map(|t| t.name.clone())
                        .unwrap_or_default();
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            let label_galley = ui.painter().layout_no_wrap(
                                trim_name.clone(),
                                egui::FontId::default(),
                                egui::Color32::WHITE,
                            );
                            let label_w = label_galley.size().x;
                            let pad = ((24.0 - label_w) / 2.0).max(0.0);
                            ui.add_space(pad);
                            ui.label(&trim_name);
                        });
                        self.show_trim_button(ui, vt, true);
                        self.show_trim_button(ui, vt, false);
                    });
                }

                self.show_stick_inner(ui, stick_size, stick_index);

                if !v_trim_on_left && let Some(vt) = v_trim {
                    let trim_name = self
                        .radio
                        .trims
                        .get(vt)
                        .map(|t| t.name.clone())
                        .unwrap_or_default();
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            let label_galley = ui.painter().layout_no_wrap(
                                trim_name.clone(),
                                egui::FontId::default(),
                                egui::Color32::WHITE,
                            );
                            let label_w = label_galley.size().x;
                            let pad = ((24.0 - label_w) / 2.0).max(0.0);
                            ui.add_space(pad);
                            ui.label(&trim_name);
                        });
                        self.show_trim_button(ui, vt, true);
                        self.show_trim_button(ui, vt, false);
                    });
                }
            });
            // Store measured stick+trim row width for label centering next frame
            let measured_col = stick_row.response.rect.width();
            if measured_col > 0.0 {
                ui.ctx().data_mut(|d| d.insert_temp(col_id, measured_col));
            }

            // Horizontal trim below stick — centered under the stick canvas
            if let Some(ht) = h_trim {
                let trim_name = self
                    .radio
                    .trims
                    .get(ht)
                    .map(|t| t.name.clone())
                    .unwrap_or_default();
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
        if response.is_pointer_button_down_on()
            && let Some(pos) = response.interact_pointer_pos()
        {
            let nx = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
            let ny = 1.0 - ((pos.y - rect.top()) / rect.height()).clamp(0.0, 1.0);
            self.stick_positions[stick_index] = (nx, ny);

            let (x_idx, y_idx) = self.stick_analog_indices[stick_index];
            let x_val = (nx * 4096.0) as u16;
            let y_val = (ny * 4096.0) as u16;
            self.analog_values[x_idx] = x_val;
            self.analog_values[y_idx] = y_val;
            self.send(InputEvent::Analog {
                index: x_idx as i32,
                value: x_val,
            });
            self.send(InputEvent::Analog {
                index: y_idx as i32,
                value: y_val,
            });
        }

        // Spring back to center on release
        if response.drag_stopped() || response.clicked() {
            self.stick_positions[stick_index] = (0.5, 0.5);
            let (x_idx, y_idx) = self.stick_analog_indices[stick_index];
            self.analog_values[x_idx] = 2048;
            self.analog_values[y_idx] = 2048;
            self.send(InputEvent::Analog {
                index: x_idx as i32,
                value: 2048,
            });
            self.send(InputEvent::Analog {
                index: y_idx as i32,
                value: 2048,
            });
        }

        // Background
        painter.rect_filled(rect, 4.0, egui::Color32::from_gray(40));
        painter.rect_stroke(
            rect,
            4.0,
            egui::Stroke::new(1.0, egui::Color32::GRAY),
            egui::StrokeKind::Inside,
        );

        // Dotted center reference lines
        let center = rect.center();
        let dash_len = 4.0;
        let gap_len = 4.0;
        let dotted_stroke = egui::Stroke::new(1.0, egui::Color32::from_gray(50));
        // Horizontal
        let mut x = rect.left();
        while x < rect.right() {
            let x_end = (x + dash_len).min(rect.right());
            painter.line_segment(
                [egui::pos2(x, center.y), egui::pos2(x_end, center.y)],
                dotted_stroke,
            );
            x += dash_len + gap_len;
        }
        // Vertical
        let mut y = rect.top();
        while y < rect.bottom() {
            let y_end = (y + dash_len).min(rect.bottom());
            painter.line_segment(
                [egui::pos2(center.x, y), egui::pos2(center.x, y_end)],
                dotted_stroke,
            );
            y += dash_len + gap_len;
        }

        // Stick position
        let (sx, sy) = self.stick_positions[stick_index];
        let dot_x = rect.left() + sx * rect.width();
        let dot_y = rect.top() + (1.0 - sy) * rect.height();

        // Solid crosshair at stick position
        painter.line_segment(
            [
                egui::pos2(rect.left(), dot_y),
                egui::pos2(rect.right(), dot_y),
            ],
            egui::Stroke::new(1.0, egui::Color32::from_gray(80)),
        );
        painter.line_segment(
            [
                egui::pos2(dot_x, rect.top()),
                egui::pos2(dot_x, rect.bottom()),
            ],
            egui::Stroke::new(1.0, egui::Color32::from_gray(80)),
        );
        painter.circle_filled(egui::pos2(dot_x, dot_y), 5.0, egui::Color32::RED);
    }

    /// Render the Monitors panel content (logical switches, channels, GVars).
    fn render_monitors(&self, ui: &mut egui::Ui) {
        if self.logical_switches.is_empty()
            && self.channel_outputs.is_empty()
            && self.gvars.is_empty()
        {
            ui.label(
                egui::RichText::new("Waiting for monitor data...")
                    .monospace()
                    .size(11.0)
                    .color(egui::Color32::GRAY),
            );
            return;
        }

        // --- Logical Switches ---
        if self.monitors_show_ls && !self.logical_switches.is_empty() {
            ui.add_space(4.0);
            ui.label(egui::RichText::new("Logical Switches").strong().size(12.0));
            ui.add_space(2.0);
            let cols = 16;
            egui::Grid::new("ls_grid")
                .spacing([2.0, 2.0])
                .show(ui, |ui| {
                    for (i, &active) in self.logical_switches.iter().enumerate() {
                        let label = format!("{:02}", i + 1);
                        let (bg, fg) = if active {
                            (egui::Color32::from_rgb(40, 167, 69), egui::Color32::WHITE)
                        } else {
                            (egui::Color32::from_gray(50), egui::Color32::from_gray(140))
                        };
                        let rect =
                            ui.allocate_exact_size(egui::vec2(28.0, 18.0), egui::Sense::hover());
                        if ui.is_rect_visible(rect.0) {
                            ui.painter().rect_filled(rect.0, 2.0, bg);
                            ui.painter().text(
                                rect.0.center(),
                                egui::Align2::CENTER_CENTER,
                                &label,
                                egui::FontId::monospace(10.0),
                                fg,
                            );
                        }
                        if (i + 1) % cols == 0 {
                            ui.end_row();
                        }
                    }
                });
            ui.add_space(4.0);
        }

        // --- Channel Outputs ---
        if self.monitors_show_ch && !self.channel_outputs.is_empty() {
            ui.label(egui::RichText::new("Channel Outputs").strong().size(12.0));
            ui.add_space(2.0);
            let bar_width = 120.0_f32;
            let bar_height = 12.0_f32;
            let cols_per_row = 4;
            let mut col = 0;
            egui::Grid::new("ch_grid")
                .spacing([8.0, 3.0])
                .show(ui, |ui| {
                    for (i, &ch_val) in self.channel_outputs.iter().enumerate() {
                        let mix_val = self.mix_outputs.get(i).copied().unwrap_or(0);
                        let label = format!("CH{:02}", i + 1);
                        ui.label(egui::RichText::new(&label).monospace().size(10.0));

                        // Draw bar pair
                        let (rect, _) = ui.allocate_exact_size(
                            egui::vec2(bar_width, bar_height),
                            egui::Sense::hover(),
                        );
                        if ui.is_rect_visible(rect) {
                            let painter = ui.painter();
                            painter.rect_filled(rect, 1.0, egui::Color32::from_gray(40));
                            let mid_x = rect.center().x;

                            // Mixer bar (yellow, behind)
                            let mix_frac = mix_val as f32 / 1024.0;
                            let mix_x = mid_x + mix_frac * (bar_width / 2.0);
                            let mix_rect = if mix_val >= 0 {
                                egui::Rect::from_min_max(
                                    egui::pos2(mid_x, rect.top()),
                                    egui::pos2(mix_x, rect.top() + bar_height),
                                )
                            } else {
                                egui::Rect::from_min_max(
                                    egui::pos2(mix_x, rect.top()),
                                    egui::pos2(mid_x, rect.top() + bar_height),
                                )
                            };
                            painter.rect_filled(
                                mix_rect,
                                0.0,
                                egui::Color32::from_rgba_unmultiplied(255, 193, 7, 100),
                            );

                            // Channel bar (blue, front)
                            let ch_frac = ch_val as f32 / 1024.0;
                            let ch_x = mid_x + ch_frac * (bar_width / 2.0);
                            let ch_rect = if ch_val >= 0 {
                                egui::Rect::from_min_max(
                                    egui::pos2(mid_x, rect.top() + 2.0),
                                    egui::pos2(ch_x, rect.top() + bar_height - 2.0),
                                )
                            } else {
                                egui::Rect::from_min_max(
                                    egui::pos2(ch_x, rect.top() + 2.0),
                                    egui::pos2(mid_x, rect.top() + bar_height - 2.0),
                                )
                            };
                            painter.rect_filled(
                                ch_rect,
                                0.0,
                                egui::Color32::from_rgb(66, 133, 244),
                            );

                            // Center line
                            painter.line_segment(
                                [
                                    egui::pos2(mid_x, rect.top()),
                                    egui::pos2(mid_x, rect.bottom()),
                                ],
                                egui::Stroke::new(1.0, egui::Color32::from_gray(100)),
                            );
                        }

                        // Numeric value
                        ui.label(
                            egui::RichText::new(format!("{:5}", ch_val))
                                .monospace()
                                .size(10.0),
                        );

                        col += 1;
                        if col % cols_per_row == 0 {
                            ui.end_row();
                        }
                    }
                });
            ui.add_space(4.0);
        }

        // --- Global Variables ---
        if self.monitors_show_gv && !self.gvars.is_empty() {
            ui.label(egui::RichText::new("Global Variables").strong().size(12.0));
            ui.add_space(2.0);
            egui::Grid::new("gv_grid")
                .spacing([6.0, 2.0])
                .show(ui, |ui| {
                    // Header row
                    ui.label(egui::RichText::new("").monospace().size(10.0));
                    for fm in 0..self.num_flight_modes {
                        ui.label(
                            egui::RichText::new(format!("FM{}", fm))
                                .monospace()
                                .size(10.0)
                                .strong(),
                        );
                    }
                    ui.end_row();

                    // Data rows
                    for (gv_idx, fm_values) in self.gvars.iter().enumerate() {
                        ui.label(
                            egui::RichText::new(format!("GV{}", gv_idx + 1))
                                .monospace()
                                .size(10.0)
                                .strong(),
                        );
                        for (fm_idx, gv) in fm_values.iter().enumerate() {
                            let text = match gv.precision {
                                1 => format!("{:.1}", gv.value as f64 / 10.0),
                                2 => format!("{:.2}", gv.value as f64 / 100.0),
                                _ => format!("{}", gv.value),
                            };
                            let is_active = gv.mode as usize == fm_idx;
                            let rt = if is_active {
                                egui::RichText::new(&text)
                                    .monospace()
                                    .size(10.0)
                                    .color(egui::Color32::from_rgb(66, 133, 244))
                                    .strong()
                            } else {
                                egui::RichText::new(&text).monospace().size(10.0)
                            };
                            ui.label(rt);
                        }
                        ui.end_row();
                    }
                });
        }
    }
}

impl eframe::App for SimulatorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Receive latest LCD buffer
        while let Ok(lcd) = self.lcd_rx.try_recv() {
            self.last_lcd = Some(lcd);
        }

        // Receive latest firmware state (custom switches + volume + monitors)
        while let Ok(state) = self.state_rx.try_recv() {
            self.custom_switch_led_states = state.custom_switches;
            self.firmware_volume = state.volume;
            if state.monitors_active {
                self.logical_switches = state.logical_switches;
                self.channel_outputs = state.channel_outputs;
                self.mix_outputs = state.mix_outputs;
                self.channels_used = state.channels_used;
                self.gvars = state.gvars;
                self.num_gvars = state.num_gvars;
                self.num_flight_modes = state.num_flight_modes;
            }
        }

        // Send monitors poll state to WASM thread when tab changes
        let want_poll = self.trace_open && self.bottom_tab == BottomTab::Monitors;
        if want_poll != self.monitors_poll_sent {
            self.send(RuntimeMessage::MonitorsPoll(want_poll));
            self.monitors_poll_sent = want_poll;
        }

        // Drain queued audio samples and play with volume scaling
        let effective_vol = if self.muted {
            0.0
        } else {
            self.firmware_volume as f32 / VOLUME_LEVEL_MAX as f32
        };
        while let Ok(samples) = self.audio_rx.try_recv() {
            self.audio_player
                .play_samples(&samples, 32000, effective_vol);
        }

        // Drain trace messages and cap at 200 lines
        while let Ok(msg) = self.trace_rx.try_recv() {
            self.trace_lines.push(msg);
        }
        if self.trace_lines.len() > 200 {
            let excess = self.trace_lines.len() - 200;
            self.trace_lines.drain(..excess);
        }

        // Handle keyboard shortcuts: F7 = Reload Lua, F8 = Reset, F9 = Screenshot
        let mut reload_lua = false;
        let mut reset = false;
        let mut take_screenshot = false;
        ctx.input(|i| {
            reload_lua = i.key_pressed(egui::Key::F7);
            reset = i.key_pressed(egui::Key::F8);
            take_screenshot = i.key_pressed(egui::Key::F9);
        });
        if reload_lua {
            self.send(RuntimeMessage::ReloadLua);
        }
        if reset {
            self.send(RuntimeMessage::Reset);
        }
        if take_screenshot {
            self.take_screenshot();
        }

        // Handle keyboard input for simulator keys
        ctx.input(|i| {
            for event in &i.events {
                if let egui::Event::Key { key, pressed, .. } = event
                    && let Some(idx) = egui_key_to_index(key)
                {
                    self.send(InputEvent::Key {
                        index: idx,
                        pressed: *pressed,
                    });
                }
            }
        });

        // Menu bar
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("Simulator", |ui| {
                    if ui
                        .add(egui::Button::new("Reload Lua Scripts").shortcut_text("F7"))
                        .clicked()
                    {
                        self.send(RuntimeMessage::ReloadLua);
                        ui.close();
                    }
                    if ui
                        .add(egui::Button::new("Reset Simulator").shortcut_text("F8"))
                        .clicked()
                    {
                        self.send(RuntimeMessage::Reset);
                        ui.close();
                    }
                });
                ui.menu_button("Tools", |ui| {
                    if ui
                        .add(egui::Button::new("Screenshot").shortcut_text("F9"))
                        .clicked()
                    {
                        self.take_screenshot();
                        ui.close();
                    }
                    if ui.button("Open Screenshots Folder").clicked() {
                        let dir = &self.screenshots_dir;
                        let _ = std::fs::create_dir_all(dir);
                        let _ = open::that(dir);
                        ui.close();
                    }
                });
                let vol_pct = (self.firmware_volume as f32 / VOLUME_LEVEL_MAX as f32 * 100.0) as u8;
                let vol_label = if self.muted {
                    "Audio (muted)".to_string()
                } else {
                    format!("Audio ({}%)", vol_pct)
                };
                ui.menu_button(vol_label, |ui| {
                    ui.checkbox(&mut self.muted, "Mute");
                    ui.label(format!("Volume: {}%", vol_pct));
                });
            });
        });

        // Split keys into left and right
        let left_keys: Vec<KeyDef> = self
            .radio
            .keys
            .iter()
            .filter(|k| k.side != "R")
            .cloned()
            .collect();
        let right_keys: Vec<KeyDef> = self
            .radio
            .keys
            .iter()
            .filter(|k| k.side == "R")
            .cloned()
            .collect();

        // Split visible toggle switches (non-NONE, non-SW) into left and right halves
        let visible_switches: Vec<usize> = self
            .radio
            .switches
            .iter()
            .enumerate()
            .filter(|(_, sw)| sw.default != "NONE" && !sw.name.starts_with("SW"))
            .map(|(i, _)| i)
            .collect();

        // Custom switches (SW1-SW6): momentary push buttons
        let custom_switches: Vec<usize> = self
            .radio
            .switches
            .iter()
            .enumerate()
            .filter(|(_, sw)| sw.default != "NONE" && sw.name.starts_with("SW"))
            .map(|(i, _)| i)
            .collect();
        let left_switch_indices: Vec<usize> =
            visible_switches[..visible_switches.len() / 2].to_vec();
        let right_switch_indices: Vec<usize> =
            visible_switches[visible_switches.len() / 2..].to_vec();

        // Collect SLIDER-type FLEX inputs for vertical slider columns
        let sliders: Vec<(usize, String)> = self
            .radio
            .inputs
            .iter()
            .enumerate()
            .filter(|(_, inp)| inp.input_type == "FLEX" && inp.default == "SLIDER")
            .map(|(i, inp)| {
                let label = if inp.label.is_empty() {
                    inp.name.clone()
                } else {
                    inp.label.clone()
                };
                (i, label)
            })
            .collect();
        let left_sliders_count = sliders.len() / 2;

        // Compute dynamic content width
        let switch_w = 140.0_f32;
        let slider_w = 40.0_f32;
        let stick_w = 140.0_f32;
        let right_sliders_count = sliders.len() - left_sliders_count;
        let _content_w = switch_w * 2.0
            + left_sliders_count as f32 * slider_w
            + right_sliders_count as f32 * slider_w
            + stick_w * 2.0
            + 92.0;

        // Bottom panel with Console / Monitors tabs
        egui::TopBottomPanel::bottom("trace_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                // Tab buttons — click inactive tab to switch & open,
                // click active tab to toggle collapse.
                let resize_panel = |this: &Self, ctx: &egui::Context, open: bool| {
                    let (_base_w, base_h) = this.window_size;
                    let current_w = ctx.content_rect().width();
                    let new_h = if open {
                        base_h + this.trace_panel_height
                    } else {
                        base_h + 30.0
                    };
                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
                        current_w, new_h,
                    )));
                };

                let console_selected = self.trace_open && self.bottom_tab == BottomTab::Console;
                if ui
                    .selectable_label(console_selected, egui::RichText::new("Console").strong())
                    .clicked()
                {
                    if console_selected {
                        self.trace_open = false;
                        resize_panel(self, ctx, false);
                    } else {
                        self.bottom_tab = BottomTab::Console;
                        if !self.trace_open {
                            self.trace_open = true;
                            resize_panel(self, ctx, true);
                        }
                    }
                }

                let monitors_selected = self.trace_open && self.bottom_tab == BottomTab::Monitors;
                if ui
                    .selectable_label(monitors_selected, egui::RichText::new("Monitors").strong())
                    .clicked()
                {
                    if monitors_selected {
                        self.trace_open = false;
                        resize_panel(self, ctx, false);
                    } else {
                        self.bottom_tab = BottomTab::Monitors;
                        if !self.trace_open {
                            self.trace_open = true;
                            resize_panel(self, ctx, true);
                        }
                    }
                }

                // Section toggles when Monitors tab is open
                if self.trace_open && self.bottom_tab == BottomTab::Monitors {
                    ui.separator();
                    ui.toggle_value(
                        &mut self.monitors_show_ls,
                        egui::RichText::new("Logical Switches").monospace(),
                    );
                    ui.toggle_value(
                        &mut self.monitors_show_ch,
                        egui::RichText::new("Channels").monospace(),
                    );
                    ui.toggle_value(
                        &mut self.monitors_show_gv,
                        egui::RichText::new("Global Vars").monospace(),
                    );
                }
            });
            if self.trace_open {
                match self.bottom_tab {
                    BottomTab::Console => {
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
                    BottomTab::Monitors => {
                        egui::ScrollArea::vertical()
                            .min_scrolled_height(350.0)
                            .max_height(350.0)
                            .show(ui, |ui| {
                                self.render_monitors(ui);
                            });
                    }
                }
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
                        let left_keys_h =
                            left_key_count * 28.0 + (left_key_count - 1.0).max(0.0) * 8.0;
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
                        let right_keys_h =
                            right_key_count * 28.0 + (right_key_count - 1.0).max(0.0) * 8.0;
                        let right_pad = ((lcd_h - right_keys_h) / 2.0).max(0.0);
                        ui.vertical(|ui| {
                            ui.add_space(right_pad);
                            self.show_keys(ui, &right_keys);
                        });
                    });

                    ui.add_space(24.0);

                    // Row 2: Pots — estimate width for centering
                    let inputs = self.radio.inputs.clone();
                    let pot_count = inputs
                        .iter()
                        .filter(|inp| {
                            inp.input_type == "FLEX"
                                && matches!(inp.default.as_str(), "POT" | "POT_CENTER" | "MULTIPOS")
                        })
                        .count();
                    if pot_count > 0 {
                        // Measure pots width from previous frame, default to estimate
                        let pots_id = ui.id().with("pots_row_w");
                        let pots_w: f32 = ui
                            .ctx()
                            .data(|d| d.get_temp(pots_id))
                            .unwrap_or(pot_count as f32 * 150.0);
                        let measured = ui
                            .horizontal(|ui| {
                                center_row(ui, pots_w);
                                let start_x = ui.cursor().left();
                                self.show_pots_row_inner(ui);
                                ui.cursor().left() - start_x
                            })
                            .inner;
                        if measured > 0.0 {
                            ui.ctx().data_mut(|d| d.insert_temp(pots_id, measured));
                        }
                    }

                    // Custom switches row (SW1-SW6): momentary push buttons
                    if !custom_switches.is_empty() {
                        ui.add_space(8.0);
                        let cs_id = ui.id().with("custom_switches_w");
                        let cs_w: f32 = ui
                            .ctx()
                            .data(|d| d.get_temp(cs_id))
                            .unwrap_or(custom_switches.len() as f32 * 48.0);
                        let measured_cs = ui
                            .horizontal(|ui| {
                                center_row(ui, cs_w);
                                let start_x = ui.cursor().left();
                                let switches = self.radio.switches.clone();
                                for (cs_index, &i) in custom_switches.iter().enumerate() {
                                    self.show_custom_switch_widget(ui, i, &switches[i], cs_index);
                                }
                                ui.cursor().left() - start_x
                            })
                            .inner;
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
                    let measured_inner = ui
                        .horizontal(|ui| {
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
                            self.show_stick_with_trims(
                                ui,
                                "LEFT STICK",
                                0,
                                left_v_trim,
                                left_h_trim,
                                true,
                            );

                            // Right stick with trims (vertical trim on right side)
                            self.show_stick_with_trims(
                                ui,
                                "RIGHT STICK",
                                1,
                                right_v_trim,
                                right_h_trim,
                                false,
                            );

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
                        })
                        .inner;
                    if measured_inner > 0.0 {
                        ui.ctx()
                            .data_mut(|d| d.insert_temp(inner_id, measured_inner));
                    }

                    ui.add_space(16.0);

                    // Row 4: Extra trims (index 4+) — only if more than 4 trims
                    if trim_count > 4 {
                        let trims_id = ui.id().with("extra_trims_w");
                        let trims_w: f32 = ui.ctx().data(|d| d.get_temp(trims_id)).unwrap_or(200.0);
                        // Center relative to inner controls (same center as sticks)
                        let trims_pad =
                            ((avail_w - inner_w) / 2.0 + (inner_w - trims_w) / 2.0).max(0.0);
                        let measured_trims = ui
                            .horizontal(|ui| {
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
                            })
                            .inner;
                        if measured_trims > 0.0 {
                            ui.ctx()
                                .data_mut(|d| d.insert_temp(trims_id, measured_trims));
                        }
                    }
                });
        });

        // Show toast notifications
        self.toasts.show(ctx);

        // Request continuous repaint
        ctx.request_repaint();
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.send(RuntimeMessage::Quit);
    }
}
