pub mod framebuffer;
pub mod input;
pub mod lua_script;
pub mod runtime;
pub mod screenshot;
pub mod sdcard;

use anyhow::Result;
use std::path::PathBuf;
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
    pub stdin_script: bool,
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

    // Execute script if provided, otherwise wait
    let exit_code = if opts.stdin_script {
        // Spawn timeout watchdog — kills runaway scripts in CI
        if let Some(timeout) = opts.timeout {
            std::thread::spawn(move || {
                std::thread::sleep(timeout);
                eprintln!("Timeout ({timeout:?}) reached, exiting");
                std::process::exit(1);
            });
        }
        let stdin = std::io::stdin().lock();
        lua_script::run_lua_stdin(stdin, &mut rt, &opts.radio, &opts)?
    } else if let Some(ref script_path) = opts.script_path {
        // Spawn timeout watchdog — kills runaway scripts in CI
        if let Some(timeout) = opts.timeout {
            std::thread::spawn(move || {
                std::thread::sleep(timeout);
                eprintln!("Timeout ({timeout:?}) reached, exiting");
                std::process::exit(1);
            });
        }
        lua_script::run_lua_script(script_path, &mut rt, &opts.radio, &opts)?
    } else if let Some(timeout) = opts.timeout {
        std::thread::sleep(timeout);
        0
    } else {
        // Wait for Ctrl+C
        let (tx, rx) = std::sync::mpsc::channel();
        ctrlc_channel(&tx);
        let _ = rx.recv();
        0
    };

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

    if exit_code != 0 {
        std::process::exit(exit_code);
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
