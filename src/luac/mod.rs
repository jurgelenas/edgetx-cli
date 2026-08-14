//! Lua bytecode compilation using EdgeTX's `edgetx-luac` WASM module.
//!
//! The module is a compile-only build of EdgeTX's Lua 5.3 fork. Its bytecode matches
//! what the radio expects: `LUA_32BITS` plus EdgeTX's patched header, which dumps
//! `size_t` as an `int` so simulator- and radio-generated `.luac` files are identical.

use std::io::Read;
use std::path::{Path, PathBuf};
use thiserror::Error;

use wamr_rust_sdk::{
    function::Function, instance::Instance, module::Module, runtime::Runtime as WamrRuntime, sys,
    value::WasmValue,
};

const LUAC_WASM_URL: &str = "https://edgetx-luac.pages.dev/edgetx-luac.wasm";
const LUAC_WASM_FILE: &str = "edgetx-luac.wasm";

/// Interpreter stack size. Lua's parser recurses over nested expressions, so this
/// matches the simulator's stack rather than the smaller SDK default.
const STACK_SIZE: u32 = 256 * 1024;

/// No app heap: the module carries its own allocator (`wasm_malloc`) in linear memory.
const HEAP_SIZE: u32 = 0;

// Negative return values of the WASM `compile` export.
const ERR_NO_STATE: i32 = -1;
const ERR_SYNTAX: i32 = -2;
const ERR_DUMP: i32 = -3;

#[derive(Error, Debug)]
pub enum LuacError {
    #[error("cannot determine cache directory")]
    CacheDir,
    #[error("{context}: {source}")]
    Http {
        context: String,
        source: reqwest::Error,
    },
    #[error("{context}: {source}")]
    Io {
        context: String,
        source: std::io::Error,
    },
    #[error("downloaded Lua compiler is not a valid WASM binary")]
    InvalidWasm,
    #[error("Lua compiler: {0}")]
    Runtime(String),
    #[error("{message}")]
    Compile { message: String },
}

fn cache_dir() -> Result<PathBuf, LuacError> {
    let base = directories::BaseDirs::new().ok_or(LuacError::CacheDir)?;
    Ok(base.cache_dir().join("edgetx-cli").join("luac"))
}

/// Download and cache the EdgeTX Lua compiler WASM module.
///
/// The upstream artifact carries no version metadata, so a cached copy is reused
/// indefinitely. Delete `<cache>/edgetx-cli/luac/edgetx-luac.wasm` to force a refresh.
pub fn ensure_luac_wasm(on_progress: impl Fn(u64, u64)) -> Result<PathBuf, LuacError> {
    let dir = cache_dir()?;
    let wasm_path = dir.join(LUAC_WASM_FILE);

    if wasm_path.exists() {
        if is_valid_wasm(&wasm_path) {
            log::debug!("Lua compiler cached at {}", wasm_path.display());
            return Ok(wasm_path);
        }
        log::debug!("cached file is not valid WASM, re-downloading");
        let _ = std::fs::remove_file(&wasm_path);
    }

    log::debug!("downloading {LUAC_WASM_URL}");

    let resp = reqwest::blocking::get(LUAC_WASM_URL).map_err(|e| LuacError::Http {
        context: "downloading the EdgeTX Lua compiler".into(),
        source: e,
    })?;
    if !resp.status().is_success() {
        return Err(LuacError::Http {
            context: "the EdgeTX Lua compiler is not available".into(),
            source: resp.error_for_status().unwrap_err(),
        });
    }

    let total = resp.content_length().unwrap_or(0);

    std::fs::create_dir_all(&dir).map_err(|e| LuacError::Io {
        context: "creating Lua compiler cache directory".into(),
        source: e,
    })?;

    let tmp_path = dir.join(format!("{LUAC_WASM_FILE}.tmp"));
    let mut file = std::fs::File::create(&tmp_path).map_err(|e| LuacError::Io {
        context: format!("creating temp file {}", tmp_path.display()),
        source: e,
    })?;

    let mut downloaded = 0u64;
    let mut reader = resp;
    let mut buf = [0u8; 8192];
    loop {
        let n = reader.read(&mut buf).map_err(|e| LuacError::Io {
            context: "reading Lua compiler download stream".into(),
            source: e,
        })?;
        if n == 0 {
            break;
        }
        std::io::Write::write_all(&mut file, &buf[..n]).map_err(|e| LuacError::Io {
            context: "writing Lua compiler to disk".into(),
            source: e,
        })?;
        downloaded += n as u64;
        on_progress(downloaded, total);
    }
    drop(file);

    std::fs::rename(&tmp_path, &wasm_path).map_err(|e| LuacError::Io {
        context: format!("renaming temp file to {}", wasm_path.display()),
        source: e,
    })?;

    if !is_valid_wasm(&wasm_path) {
        let _ = std::fs::remove_file(&wasm_path);
        return Err(LuacError::InvalidWasm);
    }

    Ok(wasm_path)
}

/// Check if a file starts with the WASM magic bytes (\x00asm).
fn is_valid_wasm(path: &Path) -> bool {
    let mut f = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return false,
    };
    let mut magic = [0u8; 4];
    if f.read_exact(&mut magic).is_err() {
        return false;
    }
    magic == [0x00, b'a', b's', b'm']
}

/// Compiles Lua source to EdgeTX bytecode.
///
/// Exists so package-pipeline code can be exercised without the WASM module.
pub trait LuaCompiler {
    fn compile(&mut self, source: &[u8]) -> Result<Vec<u8>, LuacError>;
}

/// The `edgetx-luac` WASM module loaded into its own WAMR runtime.
///
/// WAMR's init/destroy are process-global, so at most one WAMR-backed component
/// (this or `simulator::Runtime`) may be alive at a time.
pub struct Compiler {
    /// Holds the WASM instance + module, which borrow `wamr` through transmuted
    /// `'static` references (see `Compiler::new`). This field MUST be declared
    /// before `wamr`: Rust drops fields in declaration order, so the instance and
    /// module must drop *before* the WAMR runtime they point into — otherwise
    /// their destructors run against a freed runtime (use-after-free).
    state: CompilerState,
    #[allow(dead_code)]
    wamr: WamrRuntime,
    strip: bool,
}

struct CompilerState {
    #[allow(dead_code)]
    module: Module<'static>,
    instance: Instance<'static>,
}

impl Compiler {
    /// Load the compiler module. `strip` drops debug info from the bytecode.
    pub fn new(wasm_bytes: &[u8], strip: bool) -> Result<Self, LuacError> {
        let wamr = WamrRuntime::builder()
            .use_system_allocator()
            .run_as_interpreter()
            .build()
            .map_err(|e| LuacError::Runtime(format!("creating WAMR runtime: {e}")))?;

        // SAFETY: module and instance live in CompilerState, which is declared
        // before `wamr` and therefore dropped before it.
        let wamr_ref: &'static WamrRuntime = unsafe { &*(&wamr as *const WamrRuntime) };

        log::debug!("loading Lua compiler module ({} bytes)", wasm_bytes.len());
        let module = Module::from_vec(wamr_ref, wasm_bytes.to_vec(), "edgetx-luac")
            .map_err(|e| LuacError::Runtime(format!("loading WASM module: {e}")))?;

        let module_ref: &'static Module<'static> = unsafe { &*(&module as *const Module<'static>) };

        // The module is a WASI reactor; WAMR runs its `_initialize` during
        // instantiation, so calling it here would re-enter and trap.
        let instance = Instance::new_with_args(wamr_ref, module_ref, STACK_SIZE, HEAP_SIZE)
            .map_err(|e| LuacError::Runtime(format!("instantiating WASM module: {e}")))?;

        Ok(Self {
            state: CompilerState { module, instance },
            wamr,
            strip,
        })
    }

    /// Download (or reuse) the cached compiler module and load it.
    pub fn from_cache_or_download(
        strip: bool,
        on_progress: impl Fn(u64, u64),
    ) -> Result<Self, LuacError> {
        let path = ensure_luac_wasm(on_progress)?;
        let bytes = std::fs::read(&path).map_err(|e| LuacError::Io {
            context: format!("reading {}", path.display()),
            source: e,
        })?;
        Self::new(&bytes, strip)
    }

    /// Call a no-argument export returning a pointer into WASM memory, and copy
    /// `len` bytes out of it.
    fn read_exported_buffer(&self, export: &str, len: usize) -> Result<Vec<u8>, LuacError> {
        let inst = &self.state.instance;

        let func = Function::find_export_func(inst, export)
            .map_err(|e| LuacError::Runtime(format!("finding {export}: {e}")))?;
        let results = func
            .call(inst, &vec![])
            .map_err(|e| LuacError::Runtime(format!("calling {export}: {e}")))?;

        let app_ptr = match results.first() {
            Some(WasmValue::I32(v)) => *v as u32,
            other => {
                return Err(LuacError::Runtime(format!(
                    "{export} returned unexpected value: {other:?}"
                )));
            }
        };
        if app_ptr == 0 {
            return Err(LuacError::Runtime(format!("{export} returned null")));
        }

        unsafe {
            let native =
                sys::wasm_runtime_addr_app_to_native(inst.get_inner_instance(), app_ptr as u64);
            if native.is_null() {
                return Err(LuacError::Runtime(format!(
                    "{export} pointer is outside WASM memory"
                )));
            }
            Ok(std::slice::from_raw_parts(native as *const u8, len).to_vec())
        }
    }

    /// Read the compiler's error buffer.
    fn read_error(&self) -> String {
        let inst = &self.state.instance;

        let len = Function::find_export_func(inst, "get_error_len")
            .and_then(|f| f.call(inst, &vec![]))
            .ok()
            .and_then(|r| match r.first() {
                Some(WasmValue::I32(v)) if *v > 0 => Some(*v as usize),
                _ => None,
            });

        match len {
            Some(len) => match self.read_exported_buffer("get_error_ptr", len) {
                Ok(bytes) => String::from_utf8_lossy(&bytes).trim().to_string(),
                Err(e) => format!("unknown compile error ({e})"),
            },
            None => "unknown compile error".to_string(),
        }
    }
}

impl LuaCompiler for Compiler {
    fn compile(&mut self, source: &[u8]) -> Result<Vec<u8>, LuacError> {
        let inst = &self.state.instance;

        let malloc = Function::find_export_func(inst, "wasm_malloc")
            .map_err(|e| LuacError::Runtime(format!("finding wasm_malloc: {e}")))?;
        let free = Function::find_export_func(inst, "wasm_free")
            .map_err(|e| LuacError::Runtime(format!("finding wasm_free: {e}")))?;

        // Empty sources are valid Lua chunks, but a zero-size allocation is not.
        let alloc_len = source.len().max(1);
        let results = malloc
            .call(inst, &vec![WasmValue::I32(alloc_len as i32)])
            .map_err(|e| LuacError::Runtime(format!("allocating source buffer: {e}")))?;
        let ptr = match results.first() {
            Some(WasmValue::I32(v)) => *v as u32,
            other => {
                return Err(LuacError::Runtime(format!(
                    "wasm_malloc returned unexpected value: {other:?}"
                )));
            }
        };
        if ptr == 0 {
            return Err(LuacError::Runtime("wasm_malloc returned null".into()));
        }

        let copied = unsafe {
            let native =
                sys::wasm_runtime_addr_app_to_native(inst.get_inner_instance(), ptr as u64);
            if native.is_null() {
                false
            } else {
                std::ptr::copy_nonoverlapping(source.as_ptr(), native as *mut u8, source.len());
                true
            }
        };
        if !copied {
            let _ = free.call(inst, &vec![WasmValue::I32(ptr as i32)]);
            return Err(LuacError::Runtime(
                "source buffer is outside WASM memory".into(),
            ));
        }

        let compile = Function::find_export_func(inst, "compile")
            .map_err(|e| LuacError::Runtime(format!("finding compile: {e}")))?;
        let call_result = compile.call(
            inst,
            &vec![
                WasmValue::I32(ptr as i32),
                WasmValue::I32(source.len() as i32),
                WasmValue::I32(self.strip as i32),
            ],
        );

        // The source buffer is no longer needed once compile() returns, on every path.
        let _ = free.call(inst, &vec![WasmValue::I32(ptr as i32)]);

        let results =
            call_result.map_err(|e| LuacError::Runtime(format!("calling compile: {e}")))?;
        let ret = match results.first() {
            Some(WasmValue::I32(v)) => *v,
            other => {
                return Err(LuacError::Runtime(format!(
                    "compile returned unexpected value: {other:?}"
                )));
            }
        };

        match ret {
            n if n > 0 => self.read_exported_buffer("get_output_ptr", n as usize),
            ERR_SYNTAX => Err(LuacError::Compile {
                message: self.read_error(),
            }),
            ERR_NO_STATE => Err(LuacError::Runtime("could not create a Lua state".into())),
            ERR_DUMP => Err(LuacError::Runtime("writing bytecode failed".into())),
            n => Err(LuacError::Runtime(format!("compile returned {n}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn valid_wasm_detects_magic() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("mod.wasm");

        std::fs::write(&path, b"\0asm\x01\0\0\0").unwrap();
        assert!(is_valid_wasm(&path));

        std::fs::write(&path, b"<!DOCTYPE html>").unwrap();
        assert!(!is_valid_wasm(&path));

        std::fs::write(&path, b"ab").unwrap();
        assert!(!is_valid_wasm(&path));
    }

    #[test]
    fn valid_wasm_rejects_missing_file() {
        let dir = TempDir::new().unwrap();
        assert!(!is_valid_wasm(&dir.path().join("nope.wasm")));
    }

    #[test]
    fn cache_dir_ends_with_luac() {
        let dir = cache_dir().unwrap();
        assert!(dir.ends_with("edgetx-cli/luac"), "got {}", dir.display());
    }
}
