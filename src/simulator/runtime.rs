use anyhow::Result;
use std::ffi::{CStr, CString, c_void};
use std::path::Path;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, SyncSender};

use wamr_rust_sdk::{
    function::Function, instance::Instance, module::Module, runtime::Runtime as WamrRuntime, sys,
    value::WasmValue, wasi_context::WasiCtxBuilder,
};

use super::framebuffer;
use crate::radio_catalog::RadioDef;

/// Global flag set by simuLcdNotify host callback when a new frame is ready.
pub static LCD_READY: AtomicBool = AtomicBool::new(false);

/// Global sender for audio samples from the WASM callback to the simulator loop.
static AUDIO_TX: OnceLock<SyncSender<Vec<i16>>> = OnceLock::new();

/// Global sender for trace messages from the WASM callback to the UI.
static TRACE_TX: OnceLock<SyncSender<String>> = OnceLock::new();

/// Create a bounded audio channel (capacity 64). Stores the sender in `AUDIO_TX`
/// and returns the receiver. Must be called before the WASM runtime starts.
pub fn init_audio_channel() -> Receiver<Vec<i16>> {
    let (tx, rx) = std::sync::mpsc::sync_channel(64);
    AUDIO_TX
        .set(tx)
        .expect("init_audio_channel called more than once");
    rx
}

/// Create a bounded trace channel (capacity 256). Stores the sender in `TRACE_TX`
/// and returns the receiver. Must be called before the WASM runtime starts.
pub fn init_trace_channel() -> Receiver<String> {
    let (tx, rx) = std::sync::mpsc::sync_channel(256);
    TRACE_TX
        .set(tx)
        .expect("init_trace_channel called more than once");
    rx
}

/// Shared analog values read by the firmware via simuGetAnalog host import.
/// Up to 16 analog inputs (sticks + pots + sliders). Indexed by input index.
#[allow(clippy::declare_interior_mutable_const)]
static ANALOG_VALUES: [std::sync::atomic::AtomicI32; 16] = {
    const INIT: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(2048);
    [INIT; 16]
};

/// Set an analog value in the shared array (callable before runtime starts).
pub fn set_analog_value(index: usize, value: u16) {
    if index < ANALOG_VALUES.len() {
        ANALOG_VALUES[index].store(value as i32, Ordering::Relaxed);
    }
}

/// WASM runtime wrapping WAMR (supports legacy exception handling).
pub struct Runtime {
    #[allow(dead_code)]
    wamr: WamrRuntime,
    radio: RadioDef,
    sdcard_dir: String,
    settings_dir: String,
    state: Option<RuntimeState>,
    /// Pre-allocated LCD buffer pointer in WASM memory (0 = not allocated).
    lcd_buf_ptr: u32,
    lcd_buf_size: u32,
}

struct RuntimeState {
    #[allow(dead_code)]
    module: Module<'static>,
    instance: Instance<'static>,
}

// Host function stubs — these are called by the WASM module via imports.
// For now they provide minimal implementations to prevent crashes.

unsafe extern "C" fn host_simu_get_analog(_exec_env: sys::wasm_exec_env_t, index: i32) -> i32 {
    if index >= 0 && (index as usize) < ANALOG_VALUES.len() {
        ANALOG_VALUES[index as usize].load(Ordering::Relaxed)
    } else {
        2048
    }
}

unsafe extern "C" fn host_simu_queue_audio(exec_env: sys::wasm_exec_env_t, buf_ptr: u32, len: u32) {
    let tx = match AUDIO_TX.get() {
        Some(tx) => tx,
        None => return,
    };

    if len < 2 {
        return;
    }

    let inst = unsafe { sys::wasm_runtime_get_module_inst(exec_env) };
    if inst.is_null() {
        return;
    }

    let native = unsafe { sys::wasm_runtime_addr_app_to_native(inst, buf_ptr as u64) };
    if native.is_null() {
        return;
    }

    let sample_count = (len / 2) as usize;
    let slice = unsafe { std::slice::from_raw_parts(native as *const i16, sample_count) };
    let samples = slice.to_vec();

    // Non-blocking send — drop audio if the channel is full
    let _ = tx.try_send(samples);
}

unsafe extern "C" fn host_simu_trace(exec_env: sys::wasm_exec_env_t, text_ptr: u32) {
    let tx = match TRACE_TX.get() {
        Some(tx) => tx,
        None => return,
    };

    let inst = unsafe { sys::wasm_runtime_get_module_inst(exec_env) };
    if inst.is_null() {
        return;
    }

    let native = unsafe { sys::wasm_runtime_addr_app_to_native(inst, text_ptr as u64) };
    if native.is_null() {
        return;
    }

    let cstr = unsafe { CStr::from_ptr(native as *const std::ffi::c_char) };
    if let Ok(s) = cstr.to_str() {
        let _ = tx.try_send(s.to_owned());
    }
}

unsafe extern "C" fn host_simu_lcd_notify(_exec_env: sys::wasm_exec_env_t) {
    LCD_READY.store(true, Ordering::Relaxed);
}

/// Register the 4 host import functions with WAMR under the "env" module.
fn register_env_natives() -> Result<()> {
    // Heap-allocate the symbols array so it lives for the process lifetime.
    // wasm_runtime_register_natives stores pointers, not copies.
    let symbols = Box::leak(Box::new([
        sys::NativeSymbol {
            symbol: c"simuGetAnalog".as_ptr(),
            func_ptr: host_simu_get_analog as *mut c_void,
            signature: c"(i)i".as_ptr(),
            attachment: std::ptr::null_mut(),
        },
        sys::NativeSymbol {
            symbol: c"simuQueueAudio".as_ptr(),
            func_ptr: host_simu_queue_audio as *mut c_void,
            signature: c"(ii)".as_ptr(),
            attachment: std::ptr::null_mut(),
        },
        sys::NativeSymbol {
            symbol: c"simuTrace".as_ptr(),
            func_ptr: host_simu_trace as *mut c_void,
            signature: c"(i)".as_ptr(),
            attachment: std::ptr::null_mut(),
        },
        sys::NativeSymbol {
            symbol: c"simuLcdNotify".as_ptr(),
            func_ptr: host_simu_lcd_notify as *mut c_void,
            signature: c"()".as_ptr(),
            attachment: std::ptr::null_mut(),
        },
    ]));

    let ok = unsafe {
        sys::wasm_runtime_register_natives(
            c"env".as_ptr(),
            symbols.as_mut_ptr(),
            symbols.len() as u32,
        )
    };

    if ok {
        Ok(())
    } else {
        Err(anyhow::anyhow!("failed to register env native symbols"))
    }
}

impl Runtime {
    pub fn new(
        wasm_bytes: &[u8],
        radio: &RadioDef,
        sdcard_dir: &Path,
        settings_dir: &Path,
    ) -> Result<Self> {
        let wamr = WamrRuntime::builder()
            .use_system_allocator()
            .run_as_interpreter()
            .build()
            .map_err(|e| anyhow::anyhow!("creating WAMR runtime: {e}"))?;

        // Register host import functions before loading the module
        register_env_natives()?;
        log::debug!("WAMR: host functions registered");

        // SAFETY: We store module and instance together in RuntimeState.
        // The module outlives the instance because RuntimeState drops instance first.
        let wamr_ref: &'static WamrRuntime = unsafe { &*(&wamr as *const WamrRuntime) };

        log::debug!("WAMR: loading module ({} bytes)", wasm_bytes.len());
        let mut module = Module::from_vec(wamr_ref, wasm_bytes.to_vec(), "edgetx")
            .map_err(|e| anyhow::anyhow!("loading WASM module: {e}"))?;
        log::debug!("WAMR: module loaded");

        // Configure WASI with preopened directories (host paths only, no mapping)
        let sdcard_str = sdcard_dir.to_string_lossy().to_string();
        let settings_str = settings_dir.to_string_lossy().to_string();
        let wasi_ctx = WasiCtxBuilder::new()
            .set_pre_open_path(vec![&sdcard_str, &settings_str], vec![])
            .build();
        module.set_wasi_context(wasi_ctx);

        let module_ref: &'static Module<'static> = unsafe { &*(&module as *const Module<'static>) };

        log::debug!("WAMR: instantiating module");
        // Match Go version: stack=256KB, heap=8MB
        let instance = Instance::new_with_args(wamr_ref, module_ref, 256 * 1024, 8 * 1024 * 1024)
            .map_err(|e| anyhow::anyhow!("instantiating WASM module: {e}"))?;
        log::debug!("WAMR: module instantiated");

        Ok(Self {
            wamr,
            radio: radio.clone(),
            sdcard_dir: sdcard_str,
            settings_dir: settings_str,
            state: Some(RuntimeState { module, instance }),
            lcd_buf_ptr: 0,
            lcd_buf_size: 0,
        })
    }

    /// Full startup sequence matching the Go simulator:
    /// simuInit -> simuFatfsSetPaths -> simuCreateDefaults -> simuStart -> alloc LCD buffer.
    pub fn start(&mut self) -> Result<()> {
        let state = match self.state.as_ref() {
            Some(s) => s,
            None => return Ok(()),
        };

        // 1. simuInit
        let func = Function::find_export_func(&state.instance, "simuInit")
            .map_err(|e| anyhow::anyhow!("finding simuInit: {e}"))?;
        func.call(&state.instance, &vec![])
            .map_err(|e| anyhow::anyhow!("calling simuInit: {e}"))?;
        log::debug!("WAMR: simuInit done");

        // 2. simuFatfsSetPaths
        let sdcard = self.sdcard_dir.clone();
        let settings = self.settings_dir.clone();
        self.set_fatfs_paths(&sdcard, &settings)?;

        // 3. simuCreateDefaults
        self.create_defaults()?;

        // 4. simuStart(tests=0)
        self.start_firmware()?;

        // 5. Pre-allocate LCD buffer
        let buf_size = framebuffer::lcd_buffer_size(&self.radio.display) as u32;
        self.alloc_lcd_buffer(buf_size)?;

        Ok(())
    }

    /// Allocate strings in WASM memory, call simuFatfsSetPaths, then free the strings.
    pub fn set_fatfs_paths(&mut self, sdcard: &str, settings: &str) -> Result<()> {
        let state = match self.state.as_ref() {
            Some(s) => s,
            None => return Ok(()),
        };

        let malloc_func = Function::find_export_func(&state.instance, "malloc")
            .map_err(|e| anyhow::anyhow!("finding malloc: {e}"))?;
        let free_func = Function::find_export_func(&state.instance, "free")
            .map_err(|e| anyhow::anyhow!("finding free: {e}"))?;

        let sdcard_c = CString::new(sdcard).unwrap();
        let settings_c = CString::new(settings).unwrap();
        let sdcard_bytes = sdcard_c.as_bytes_with_nul();
        let settings_bytes = settings_c.as_bytes_with_nul();

        // Allocate sdcard string in WASM
        let sd_results = malloc_func
            .call(
                &state.instance,
                &vec![WasmValue::I32(sdcard_bytes.len() as i32)],
            )
            .map_err(|e| anyhow::anyhow!("malloc for sdcard path: {e}"))?;
        let sd_ptr = match sd_results.first() {
            Some(WasmValue::I32(v)) => *v as u32,
            _ => {
                return Err(anyhow::anyhow!(
                    "malloc for sdcard path returned unexpected value"
                ));
            }
        };
        if sd_ptr == 0 {
            return Err(anyhow::anyhow!("malloc for sdcard path returned null"));
        }

        // Allocate settings string in WASM
        let set_results = malloc_func
            .call(
                &state.instance,
                &vec![WasmValue::I32(settings_bytes.len() as i32)],
            )
            .map_err(|e| anyhow::anyhow!("malloc for settings path: {e}"))?;
        let set_ptr = match set_results.first() {
            Some(WasmValue::I32(v)) => *v as u32,
            _ => {
                return Err(anyhow::anyhow!(
                    "malloc for settings path returned unexpected value"
                ));
            }
        };
        if set_ptr == 0 {
            let _ = free_func.call(&state.instance, &vec![WasmValue::I32(sd_ptr as i32)]);
            return Err(anyhow::anyhow!("malloc for settings path returned null"));
        }

        // Copy strings into WASM memory
        let inst_ptr = state.instance.get_inner_instance();
        unsafe {
            let sd_native = sys::wasm_runtime_addr_app_to_native(inst_ptr, sd_ptr as u64);
            if !sd_native.is_null() {
                std::ptr::copy_nonoverlapping(
                    sdcard_bytes.as_ptr(),
                    sd_native as *mut u8,
                    sdcard_bytes.len(),
                );
            }
            let set_native = sys::wasm_runtime_addr_app_to_native(inst_ptr, set_ptr as u64);
            if !set_native.is_null() {
                std::ptr::copy_nonoverlapping(
                    settings_bytes.as_ptr(),
                    set_native as *mut u8,
                    settings_bytes.len(),
                );
            }
        }

        // Call simuFatfsSetPaths(sdcard_ptr, settings_ptr)
        let set_paths_func = Function::find_export_func(&state.instance, "simuFatfsSetPaths")
            .map_err(|e| anyhow::anyhow!("finding simuFatfsSetPaths: {e}"))?;
        set_paths_func
            .call(
                &state.instance,
                &vec![
                    WasmValue::I32(sd_ptr as i32),
                    WasmValue::I32(set_ptr as i32),
                ],
            )
            .map_err(|e| anyhow::anyhow!("calling simuFatfsSetPaths: {e}"))?;
        log::debug!("WAMR: simuFatfsSetPaths done");

        // Free the strings
        let _ = free_func.call(&state.instance, &vec![WasmValue::I32(sd_ptr as i32)]);
        let _ = free_func.call(&state.instance, &vec![WasmValue::I32(set_ptr as i32)]);

        Ok(())
    }

    /// Call simuCreateDefaults.
    pub fn create_defaults(&mut self) -> Result<()> {
        let state = match self.state.as_ref() {
            Some(s) => s,
            None => return Ok(()),
        };
        let func = Function::find_export_func(&state.instance, "simuCreateDefaults")
            .map_err(|e| anyhow::anyhow!("finding simuCreateDefaults: {e}"))?;
        func.call(&state.instance, &vec![])
            .map_err(|e| anyhow::anyhow!("calling simuCreateDefaults: {e}"))?;
        log::debug!("WAMR: simuCreateDefaults done");
        Ok(())
    }

    /// Call simuStart(tests=0).
    pub fn start_firmware(&mut self) -> Result<()> {
        let state = match self.state.as_ref() {
            Some(s) => s,
            None => return Ok(()),
        };
        let func = Function::find_export_func(&state.instance, "simuStart")
            .map_err(|e| anyhow::anyhow!("finding simuStart: {e}"))?;
        func.call(&state.instance, &vec![WasmValue::I32(0)])
            .map_err(|e| anyhow::anyhow!("calling simuStart: {e}"))?;
        log::debug!("WAMR: simuStart done");
        Ok(())
    }

    /// Pre-allocate a reusable LCD buffer in WASM memory.
    pub fn alloc_lcd_buffer(&mut self, size: u32) -> Result<()> {
        let state = match self.state.as_ref() {
            Some(s) => s,
            None => return Ok(()),
        };

        let malloc_func = Function::find_export_func(&state.instance, "malloc")
            .map_err(|e| anyhow::anyhow!("finding malloc: {e}"))?;
        let results = malloc_func
            .call(&state.instance, &vec![WasmValue::I32(size as i32)])
            .map_err(|e| anyhow::anyhow!("malloc for LCD buffer: {e}"))?;
        let ptr = match results.first() {
            Some(WasmValue::I32(v)) => *v as u32,
            _ => {
                return Err(anyhow::anyhow!(
                    "malloc for LCD buffer returned unexpected value"
                ));
            }
        };
        if ptr == 0 {
            return Err(anyhow::anyhow!("malloc for LCD buffer returned null"));
        }

        self.lcd_buf_ptr = ptr;
        self.lcd_buf_size = size;
        log::debug!("WAMR: LCD buffer allocated at 0x{:x} ({} bytes)", ptr, size);
        Ok(())
    }

    /// Mark the LCD as flushed (acknowledge the change).
    pub fn lcd_flushed(&self) {
        let state = match self.state.as_ref() {
            Some(s) => s,
            None => return,
        };
        if let Ok(func) = Function::find_export_func(&state.instance, "simuLcdFlushed") {
            let _ = func.call(&state.instance, &vec![]);
        }
    }

    pub fn stop(&mut self) {
        if let Some(state) = self.state.as_ref()
            && let Ok(func) = Function::find_export_func(&state.instance, "simuStop")
        {
            let _ = func.call(&state.instance, &vec![]);
        }
        // Free the pre-allocated LCD buffer
        if self.lcd_buf_ptr != 0 {
            if let Some(state) = self.state.as_ref()
                && let Ok(free_func) = Function::find_export_func(&state.instance, "free")
            {
                let _ = free_func.call(
                    &state.instance,
                    &vec![WasmValue::I32(self.lcd_buf_ptr as i32)],
                );
            }
            self.lcd_buf_ptr = 0;
            self.lcd_buf_size = 0;
        }
        self.state = None;
    }

    pub fn set_key(&mut self, index: i32, pressed: bool) {
        if let Some(state) = self.state.as_ref()
            && let Ok(func) = Function::find_export_func(&state.instance, "simuSetKey")
        {
            let params = vec![
                WasmValue::I32(index),
                WasmValue::I32(if pressed { 1 } else { 0 }),
            ];
            let _ = func.call(&state.instance, &params);
        }
    }

    /// Set a switch position. State: -1 (up), 0 (mid), 1 (down).
    pub fn set_switch(&mut self, index: i32, state: i32) {
        if let Some(s) = self.state.as_ref()
            && let Ok(func) = Function::find_export_func(&s.instance, "simuSetSwitch")
        {
            let params = vec![WasmValue::I32(index), WasmValue::I32(state)];
            let _ = func.call(&s.instance, &params);
        }
    }

    /// Set a trim button state.
    pub fn set_trim(&mut self, index: i32, pressed: bool) {
        if let Some(state) = self.state.as_ref()
            && let Ok(func) = Function::find_export_func(&state.instance, "simuSetTrim")
        {
            let params = vec![
                WasmValue::I32(index),
                WasmValue::I32(if pressed { 1 } else { 0 }),
            ];
            let _ = func.call(&state.instance, &params);
        }
    }

    /// Set an analog input value (0-4096).
    /// Writes to the shared ANALOG_VALUES array read by the simuGetAnalog host import.
    pub fn set_analog(&mut self, index: i32, value: u16) {
        if index >= 0 && (index as usize) < ANALOG_VALUES.len() {
            ANALOG_VALUES[index as usize].store(value as i32, Ordering::Relaxed);
        }
    }

    pub fn rotary_encoder(&mut self, delta: i32) {
        if let Some(state) = self.state.as_ref()
            && let Ok(func) = Function::find_export_func(&state.instance, "simuRotaryEncoderEvent")
        {
            let _ = func.call(&state.instance, &vec![WasmValue::I32(delta)]);
        }
    }

    pub fn touch_down(&mut self, x: i32, y: i32) {
        if let Some(state) = self.state.as_ref()
            && let Ok(func) = Function::find_export_func(&state.instance, "simuTouchDown")
        {
            let params = vec![WasmValue::I32(x), WasmValue::I32(y)];
            let _ = func.call(&state.instance, &params);
        }
    }

    pub fn touch_up(&mut self) {
        if let Some(state) = self.state.as_ref()
            && let Ok(func) = Function::find_export_func(&state.instance, "simuTouchUp")
        {
            let _ = func.call(&state.instance, &vec![]);
        }
    }

    /// Get the number of custom switches reported by the firmware.
    /// Returns 0 if the export is not available.
    pub fn get_num_custom_switches(&self) -> u8 {
        let state = match self.state.as_ref() {
            Some(s) => s,
            None => return 0,
        };
        if let Ok(func) = Function::find_export_func(&state.instance, "simuGetNumCustomSwitches")
            && let Ok(results) = func.call(&state.instance, &vec![])
            && let Some(WasmValue::I32(v)) = results.first()
        {
            return *v as u8;
        }
        0
    }

    /// Get whether a custom switch LED is on.
    pub fn get_custom_switch_state(&self, idx: u8) -> bool {
        let state = match self.state.as_ref() {
            Some(s) => s,
            None => return false,
        };
        if let Ok(func) = Function::find_export_func(&state.instance, "simuGetCustomSwitchState")
            && let Ok(results) = func.call(&state.instance, &vec![WasmValue::I32(idx as i32)])
            && let Some(WasmValue::I32(v)) = results.first()
        {
            return *v != 0;
        }
        false
    }

    /// Get the packed RGB color for a custom switch LED.
    pub fn get_custom_switch_color(&self, idx: u8) -> u32 {
        let state = match self.state.as_ref() {
            Some(s) => s,
            None => return 0,
        };
        if let Ok(func) = Function::find_export_func(&state.instance, "simuGetCustomSwitchColor")
            && let Ok(results) = func.call(&state.instance, &vec![WasmValue::I32(idx as i32)])
            && let Some(WasmValue::I32(v)) = results.first()
        {
            return *v as u32;
        }
        0
    }

    /// Copy the LCD framebuffer using the pre-allocated buffer.
    /// Returns None if the LCD hasn't changed or the buffer isn't allocated.
    pub fn get_lcd_buffer(&mut self) -> Option<Vec<u8>> {
        let state = self.state.as_ref()?;

        if self.lcd_buf_ptr == 0 {
            return None;
        }

        // Call simuLcdCopy to fill the pre-allocated buffer
        let copy_func = match Function::find_export_func(&state.instance, "simuLcdCopy") {
            Ok(f) => f,
            Err(e) => {
                log::debug!("simuLcdCopy not found: {e}");
                return None;
            }
        };
        let copy_results = match copy_func.call(
            &state.instance,
            &vec![
                WasmValue::I32(self.lcd_buf_ptr as i32),
                WasmValue::I32(self.lcd_buf_size as i32),
            ],
        ) {
            Ok(r) => r,
            Err(e) => {
                log::debug!("simuLcdCopy call failed: {e}");
                return None;
            }
        };
        let copied = match copy_results.first() {
            Some(WasmValue::I32(v)) => *v as usize,
            other => {
                log::debug!("simuLcdCopy unexpected result: {other:?}");
                0
            }
        };

        if copied == 0 {
            log::trace!("simuLcdCopy returned 0 bytes");
            return None;
        }
        log::trace!("simuLcdCopy returned {copied} bytes");

        // Read from WASM memory
        let inst_ptr = state.instance.get_inner_instance();
        let data = unsafe {
            let native_ptr =
                sys::wasm_runtime_addr_app_to_native(inst_ptr, self.lcd_buf_ptr as u64);
            if native_ptr.is_null() {
                None
            } else {
                let slice = std::slice::from_raw_parts(native_ptr as *const u8, copied);
                Some(slice.to_vec())
            }
        };

        // Mark LCD as flushed after reading
        self.lcd_flushed();

        data
    }
}
