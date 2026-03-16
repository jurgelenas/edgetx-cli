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

pub struct SimulatorOptions {
    pub radio: RadioDef,
    pub sdcard_dir: PathBuf,
    pub settings_dir: PathBuf,
    #[allow(dead_code)]
    pub watch_dir: Option<PathBuf>,
    pub timeout: Option<Duration>,
    pub screenshot_path: Option<String>,
    pub script_path: Option<PathBuf>,
}

pub fn run(opts: SimulatorOptions, wasm_bytes: &[u8]) -> Result<()> {
    // Initialize audio channel; drop receiver so samples are silently discarded
    let _audio_rx = runtime::init_audio_channel();
    drop(_audio_rx);

    let mut rt = runtime::Runtime::new(
        wasm_bytes,
        &opts.radio,
        &opts.sdcard_dir,
        &opts.settings_dir,
    )?;

    rt.start()?;

    // Execute script if provided, otherwise run with timeout or until stopped
    if let Some(ref script_path) = opts.script_path {
        run_script(&opts, &mut rt, script_path)?;
    } else if let Some(timeout) = opts.timeout {
        std::thread::sleep(timeout);
    } else {
        // Wait for Ctrl+C
        let (tx, rx) = std::sync::mpsc::channel();
        ctrlc_channel(&tx);
        let _ = rx.recv();
    }

    // Take screenshot if requested (separate from script screenshots)
    if let Some(ref path) = opts.screenshot_path
        && let Some(lcd) = rt.get_lcd_buffer()
    {
        let rgba = framebuffer::decode(&lcd, &opts.radio.display);
        screenshot::save_screenshot(
            path,
            &rgba,
            opts.radio.display.w as u32,
            opts.radio.display.h as u32,
        )?;
    }

    rt.stop();
    Ok(())
}

fn run_script(opts: &SimulatorOptions, rt: &mut runtime::Runtime, script_path: &Path) -> Result<()> {
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
                    let rgba = framebuffer::decode(&lcd, &opts.radio.display);
                    screenshot::save_screenshot(
                        path,
                        &rgba,
                        opts.radio.display.w as u32,
                        opts.radio.display.h as u32,
                    )?;
                }
            }
        }
    }
    Ok(())
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
