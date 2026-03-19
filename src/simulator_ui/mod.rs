pub mod app;
pub mod audio;
pub mod input;

use anyhow::Result;
use std::time::Duration;

use crate::simulator::SimulatorOptions;
use crate::simulator::input::{InputEvent, RuntimeMessage};
use crate::simulator::runtime;
use app::{CustomSwitchState, FirmwareState, SimulatorApp};

pub fn run(opts: SimulatorOptions, wasm_bytes: &[u8]) -> Result<()> {
    let radio = opts.radio.clone();
    let sdcard_dir = opts.sdcard_dir.clone();
    let settings_dir = opts.settings_dir.clone();

    // Start WASM runtime on a separate thread
    let (lcd_tx, lcd_rx) = std::sync::mpsc::channel::<Vec<u8>>();
    let (input_tx, input_rx) = std::sync::mpsc::channel::<RuntimeMessage>();
    let (state_tx, state_rx) = std::sync::mpsc::channel::<FirmwareState>();

    // Initialize audio and trace channels before spawning WASM thread
    let audio_rx = runtime::init_audio_channel();
    let trace_rx = runtime::init_trace_channel();
    let audio_player = audio::AudioPlayer::new()?;

    let radio_clone = radio.clone();
    let wasm_bytes = wasm_bytes.to_vec();

    let _wasm_thread = std::thread::spawn(move || -> Result<()> {
        let mut rt = runtime::Runtime::new(&wasm_bytes, &radio_clone, &sdcard_dir, &settings_dir)?;

        rt.start()?;

        // Main loop: poll inputs, send LCD updates on notification or every 100ms
        use std::collections::HashMap;
        let mut monitors_polling = false;
        loop {
            // Drain all pending input events, deduplicating trim/key to final state
            let mut trim_finals: HashMap<i32, bool> = HashMap::new();
            let mut key_finals: HashMap<i32, bool> = HashMap::new();

            while let Ok(msg) = input_rx.try_recv() {
                match msg {
                    RuntimeMessage::Input(event) => match event {
                        InputEvent::Key { index, pressed } => {
                            key_finals.insert(index, pressed);
                        }
                        InputEvent::Rotary(delta) => {
                            rt.rotary_encoder(delta);
                        }
                        InputEvent::Touch { x, y, down } => {
                            if down {
                                rt.touch_down(x, y);
                            } else {
                                rt.touch_up();
                            }
                        }
                        InputEvent::Switch { index, state } => {
                            rt.set_switch(index, state);
                        }
                        InputEvent::Trim { index, pressed } => {
                            trim_finals.insert(index, pressed);
                        }
                        InputEvent::Analog { index, value } => {
                            rt.set_analog(index, value);
                        }
                    },
                    RuntimeMessage::SetTrimValue { index, value } => {
                        rt.set_trim_value(index, value);
                    }
                    RuntimeMessage::ReloadLua => {
                        let _ = rt.reload_lua();
                    }
                    RuntimeMessage::Reset => {
                        let _ = rt.reset();
                    }
                    RuntimeMessage::MonitorsPoll(active) => {
                        monitors_polling = active;
                    }
                    RuntimeMessage::Quit => {
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

            // Poll firmware state (custom switch LEDs + audio volume)
            let volume = rt.get_audio_volume();
            let num_cs = rt.get_num_custom_switches() as usize;
            let custom_switches: Vec<CustomSwitchState> = (0..num_cs)
                .map(|i| {
                    let active = rt.get_custom_switch_state(i as u8);
                    let rgb = if active {
                        rt.get_custom_switch_color(i as u8)
                    } else {
                        0
                    };
                    let color = if active && rgb != 0 {
                        egui::Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
                    } else if active {
                        egui::Color32::from_rgb(56, 191, 249)
                    } else {
                        egui::Color32::from_gray(60)
                    };
                    CustomSwitchState { color }
                })
                .collect();

            // Poll monitor data only when the monitors tab is active
            let (
                monitors_active,
                logical_switches,
                channel_outputs,
                mix_outputs,
                channels_used,
                gvars,
                num_gvars,
                num_flight_modes,
            ) = if monitors_polling {
                let ls = rt.get_logical_switches();
                let ch = rt.get_channel_outputs();
                let mix = rt.get_mix_outputs();
                let used = rt.get_channels_used();
                let ng = rt.get_num_gvars();
                let nfm = rt.get_num_flight_modes();
                let gvars: Vec<Vec<runtime::GVarValue>> = (0..ng)
                    .map(|gv| (0..nfm).map(|fm| rt.get_gvar(gv, fm)).collect())
                    .collect();
                (true, ls, ch, mix, used, gvars, ng, nfm)
            } else {
                (
                    false,
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                    0,
                    Vec::new(),
                    0,
                    0,
                )
            };

            let _ = state_tx.send(FirmwareState {
                custom_switches,
                volume,
                monitors_active,
                logical_switches,
                channel_outputs,
                mix_outputs,
                channels_used,
                gvars,
                num_gvars,
                num_flight_modes,
            });

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
    let base_h = lcd_display_h + 450.0;
    let trace_panel_h = 380.0;
    let window_h = base_h + 30.0;

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([window_w, window_h])
            .with_min_inner_size([window_w, lcd_display_h + 450.0])
            .with_title(format!("EdgeTX Simulator - {}", radio.name))
            .with_decorations(true),
        ..Default::default()
    };

    let mut app = SimulatorApp::new(
        radio,
        lcd_rx,
        input_tx,
        state_rx,
        audio_player,
        audio_rx,
        trace_rx,
        opts.sdcard_dir,
    );
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
