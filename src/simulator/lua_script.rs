use anyhow::Result;
use mlua::prelude::*;
use std::io::BufRead;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use super::runtime::Runtime;
use super::{SimulatorOptions, framebuffer, input, screenshot};
use crate::radio_catalog::RadioDef;

/// Custom error type used by the `exit(code)` Lua function to signal
/// that the script wants to terminate with a specific process exit code.
#[derive(Debug, Clone)]
pub struct ScriptExit(pub i32);

impl std::fmt::Display for ScriptExit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "script exited with code {}", self.0)
    }
}

impl std::error::Error for ScriptExit {}

/// Extract a `ScriptExit` code from an mlua error, if present.
fn extract_exit_code(err: &LuaError) -> Option<i32> {
    if let LuaError::CallbackError { cause, .. } = err {
        return extract_exit_code(cause);
    }
    if let LuaError::ExternalError(arc) = err
        && let Some(exit) = arc.downcast_ref::<ScriptExit>()
    {
        return Some(exit.0);
    }
    None
}

/// Run a Lua test script against the simulator runtime.
/// Returns the exit code (0 by default, or the code passed to `exit()`).
pub fn run_lua_script(
    path: &Path,
    rt: &mut Runtime,
    radio: &RadioDef,
    opts: &SimulatorOptions,
) -> Result<i32> {
    let source = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("reading script {}: {e}", path.display()))?;

    let lua = Lua::new();

    // Load and syntax-check before executing (fast-fail on parse errors)
    let chunk = lua
        .load(&source)
        .set_name(path.to_string_lossy())
        .into_function()
        .map_err(|e| anyhow::anyhow!("loading script {}: {e}", path.display()))?;

    // RefCell must outlive the scope so closures can borrow it
    let rt = std::cell::RefCell::new(rt);

    let result = lua.scope(|scope| {
        register_globals(&lua, scope, &rt, radio, opts)?;
        chunk.call::<()>(())
    });

    match result {
        Ok(()) => Ok(0),
        Err(e) => {
            if let Some(code) = extract_exit_code(&e) {
                Ok(code)
            } else {
                Err(anyhow::anyhow!("executing script {}: {e}", path.display()))
            }
        }
    }
}

/// Run Lua commands from a buffered reader (stdin streaming).
/// Returns the exit code (0 by default, or the code passed to `exit()`).
pub fn run_lua_stdin(
    reader: impl BufRead,
    rt: &mut Runtime,
    radio: &RadioDef,
    opts: &SimulatorOptions,
) -> Result<i32> {
    let lua = Lua::new();

    // RefCell must outlive the scope so closures can borrow it
    let rt = std::cell::RefCell::new(rt);

    let result = lua.scope(|scope| {
        register_globals(&lua, scope, &rt, radio, opts)?;

        let mut buffer = String::new();
        for line in reader.lines() {
            let line = line.map_err(LuaError::external)?;
            if !buffer.is_empty() {
                buffer.push('\n');
            }
            buffer.push_str(&line);

            // Try to parse the buffer as a complete chunk
            match lua.load(&buffer).into_function() {
                Ok(func) => {
                    // Complete chunk — execute it
                    func.call::<()>(())?;
                    buffer.clear();
                }
                Err(e) => {
                    // Check if the error indicates incomplete input (needs more lines)
                    let msg = e.to_string();
                    if msg.contains("<eof>") || msg.contains("'<eof>'") {
                        // Incomplete — keep buffering
                        continue;
                    }
                    // Real syntax error — print and clear buffer
                    eprintln!("Error: {e}");
                    buffer.clear();
                }
            }
        }

        // EOF — try to execute any remaining buffer
        if !buffer.is_empty() {
            match lua.load(&buffer).exec() {
                Ok(()) => {}
                Err(e) => {
                    if extract_exit_code(&e).is_some() {
                        return Err(e);
                    }
                    eprintln!("Error: {e}");
                }
            }
        }

        Ok(())
    });

    match result {
        Ok(()) => Ok(0),
        Err(e) => {
            if let Some(code) = extract_exit_code(&e) {
                Ok(code)
            } else {
                Err(anyhow::anyhow!("stdin script error: {e}"))
            }
        }
    }
}

fn register_globals<'scope, 'env: 'scope>(
    lua: &Lua,
    scope: &'scope mlua::Scope<'scope, 'env>,
    rt: &'env std::cell::RefCell<&mut Runtime>,
    radio: &'env RadioDef,
    opts: &'env SimulatorOptions,
) -> LuaResult<()> {
    // -- KEY constants table --
    // If the radio defines keys, only register those (warns on non-existent keys).
    // Otherwise fall back to the full hardcoded set.
    let key_table = lua.create_table()?;
    if radio.keys.is_empty() {
        for &(name, _) in input::SCRIPT_KEYS {
            key_table.set(name, name)?;
        }
    } else {
        for k in &radio.keys {
            let name = k.key.strip_prefix("KEY_").unwrap_or(&k.key);
            if input::script_key_index(name).is_some() {
                key_table.set(name, name)?;
            }
        }
    }
    lua.globals().set("KEY", key_table)?;

    // -- key.* functions --
    let key_ns = lua.create_table()?;

    key_ns.set(
        "press",
        scope.create_function(|_, name: LuaValue| {
            let name = resolve_key_name(&name)?;
            let idx = key_index(&name)?;
            rt.borrow_mut().set_key(idx, true);
            std::thread::sleep(Duration::from_millis(100));
            rt.borrow_mut().set_key(idx, false);
            Ok(())
        })?,
    )?;

    key_ns.set(
        "longpress",
        scope.create_function(|_, name: LuaValue| {
            let name = resolve_key_name(&name)?;
            let idx = key_index(&name)?;
            rt.borrow_mut().set_key(idx, true);
            std::thread::sleep(Duration::from_secs(1));
            rt.borrow_mut().set_key(idx, false);
            Ok(())
        })?,
    )?;

    key_ns.set(
        "down",
        scope.create_function(|_, name: LuaValue| {
            let name = resolve_key_name(&name)?;
            let idx = key_index(&name)?;
            rt.borrow_mut().set_key(idx, true);
            Ok(())
        })?,
    )?;

    key_ns.set(
        "up",
        scope.create_function(|_, name: LuaValue| {
            let name = resolve_key_name(&name)?;
            let idx = key_index(&name)?;
            rt.borrow_mut().set_key(idx, false);
            Ok(())
        })?,
    )?;

    lua.globals().set("key", key_ns)?;

    // -- touch.* functions --
    let touch_ns = lua.create_table()?;

    touch_ns.set(
        "tap",
        scope.create_function(|_, (x, y): (i32, i32)| {
            rt.borrow_mut().touch_down(x, y);
            std::thread::sleep(Duration::from_millis(100));
            rt.borrow_mut().touch_up();
            Ok(())
        })?,
    )?;

    touch_ns.set(
        "longpress",
        scope.create_function(|_, (x, y): (i32, i32)| {
            rt.borrow_mut().touch_down(x, y);
            std::thread::sleep(Duration::from_secs(1));
            rt.borrow_mut().touch_up();
            Ok(())
        })?,
    )?;

    touch_ns.set(
        "down",
        scope.create_function(|_, (x, y): (i32, i32)| {
            rt.borrow_mut().touch_down(x, y);
            Ok(())
        })?,
    )?;

    touch_ns.set(
        "release",
        scope.create_function(|_, ()| {
            rt.borrow_mut().touch_up();
            Ok(())
        })?,
    )?;

    lua.globals().set("touch", touch_ns)?;

    // -- SWITCH constants table (from RadioDef) --
    let switch_table = lua.create_table()?;
    for (i, sw) in radio.switches.iter().enumerate() {
        switch_table.set(sw.name.as_str(), i as i32)?;
    }
    lua.globals().set("SWITCH", switch_table)?;

    // -- INPUT constants table (from RadioDef) --
    let input_table = lua.create_table()?;
    for (i, inp) in radio.inputs.iter().enumerate() {
        input_table.set(inp.name.as_str(), i as i32)?;
    }
    lua.globals().set("INPUT", input_table)?;

    // -- switch(name_or_index, state) --
    lua.globals().set(
        "switch",
        scope.create_function(|_, (name_or_idx, state): (LuaValue, i32)| {
            if !(-1..=1).contains(&state) {
                return Err(LuaError::runtime(format!(
                    "switch state {state} out of range (-1, 0, 1)"
                )));
            }
            let idx = resolve_switch_index(&name_or_idx, radio)?;
            rt.borrow_mut().set_switch(idx, state);
            Ok(())
        })?,
    )?;

    // -- analog(name_or_index, value) --
    lua.globals().set(
        "analog",
        scope.create_function(|_, (name_or_idx, value): (LuaValue, i32)| {
            if !(0..=4096).contains(&value) {
                return Err(LuaError::runtime(format!(
                    "analog value {value} out of range (0-4096)"
                )));
            }
            let idx = resolve_input_index(&name_or_idx, radio)?;
            rt.borrow_mut().set_analog(idx, value as u16);
            Ok(())
        })?,
    )?;

    // -- TRIM constants table (from RadioDef) --
    let trim_table = lua.create_table()?;
    for (i, tr) in radio.trims.iter().enumerate() {
        trim_table.set(tr.name.as_str(), i as i32)?;
    }
    lua.globals().set("TRIM", trim_table)?;

    // -- trim.* namespace --
    let trim_ns = lua.create_table()?;

    trim_ns.set(
        "press",
        scope.create_function(|_, name_or_idx: LuaValue| {
            let idx = resolve_trim_index(&name_or_idx, radio)?;
            rt.borrow_mut().set_trim(idx, true);
            std::thread::sleep(Duration::from_millis(100));
            rt.borrow_mut().set_trim(idx, false);
            Ok(())
        })?,
    )?;

    trim_ns.set(
        "longpress",
        scope.create_function(|_, name_or_idx: LuaValue| {
            let idx = resolve_trim_index(&name_or_idx, radio)?;
            rt.borrow_mut().set_trim(idx, true);
            std::thread::sleep(Duration::from_secs(1));
            rt.borrow_mut().set_trim(idx, false);
            Ok(())
        })?,
    )?;

    trim_ns.set(
        "down",
        scope.create_function(|_, name_or_idx: LuaValue| {
            let idx = resolve_trim_index(&name_or_idx, radio)?;
            rt.borrow_mut().set_trim(idx, true);
            Ok(())
        })?,
    )?;

    trim_ns.set(
        "up",
        scope.create_function(|_, name_or_idx: LuaValue| {
            let idx = resolve_trim_index(&name_or_idx, radio)?;
            rt.borrow_mut().set_trim(idx, false);
            Ok(())
        })?,
    )?;

    trim_ns.set(
        "get",
        scope.create_function(|_, name_or_idx: LuaValue| {
            let idx = resolve_trim_index(&name_or_idx, radio)?;
            Ok(rt.borrow().get_trim_value(idx))
        })?,
    )?;

    trim_ns.set(
        "set",
        scope.create_function(|_, (name_or_idx, value): (LuaValue, i32)| {
            let idx = resolve_trim_index(&name_or_idx, radio)?;
            rt.borrow_mut().set_trim_value(idx, value);
            Ok(())
        })?,
    )?;

    trim_ns.set(
        "range",
        scope.create_function(|_, ()| {
            let (min, max) = rt.borrow().get_trim_range();
            Ok((min, max))
        })?,
    )?;

    lua.globals().set("trim", trim_ns)?;

    // -- rotary(delta) --
    lua.globals().set(
        "rotary",
        scope.create_function(|_, delta: i32| {
            rt.borrow_mut().rotary_encoder(delta);
            Ok(())
        })?,
    )?;

    // -- wait(seconds) --
    lua.globals().set(
        "wait",
        scope.create_function(|_, secs: f64| {
            if secs < 0.0 {
                return Err(LuaError::runtime("wait duration must be non-negative"));
            }
            std::thread::sleep(Duration::from_secs_f64(secs));
            Ok(())
        })?,
    )?;

    // -- screenshot(path) --
    lua.globals().set(
        "screenshot",
        scope.create_function(|_, path: String| {
            let lcd = rt
                .borrow_mut()
                .get_lcd_buffer()
                .ok_or_else(|| LuaError::runtime("screenshot failed: no LCD buffer available"))?;
            let rgba = framebuffer::decode(&lcd, &opts.radio.display);
            screenshot::save_screenshot(
                Path::new(&path),
                &rgba,
                opts.radio.display.w as u32,
                opts.radio.display.h as u32,
            )
            .map_err(|e| LuaError::runtime(format!("screenshot failed: {e}")))?;
            Ok(())
        })?,
    )?;

    // -- reset() --
    lua.globals().set(
        "reset",
        scope.create_function(|_, ()| {
            rt.borrow_mut()
                .reset()
                .map_err(|e| LuaError::runtime(format!("reset failed: {e}")))?;
            Ok(())
        })?,
    )?;

    // -- reload() --
    lua.globals().set(
        "reload",
        scope.create_function(|_, ()| {
            rt.borrow_mut()
                .reload_lua()
                .map_err(|e| LuaError::runtime(format!("reload failed: {e}")))?;
            Ok(())
        })?,
    )?;

    // -- exit(code) --
    lua.globals().set(
        "exit",
        scope.create_function(|_, code: i32| -> LuaResult<()> {
            Err(LuaError::ExternalError(Arc::new(ScriptExit(code))))
        })?,
    )?;

    // -- channel.* namespace (1-based indices) --
    let channel_ns = lua.create_table()?;

    channel_ns.set(
        "count",
        scope.create_function(|_, ()| Ok(rt.borrow().get_num_channels() as i32))?,
    )?;

    channel_ns.set(
        "get",
        scope.create_function(|_, index: i32| {
            let count = rt.borrow().get_num_channels() as i32;
            if index < 1 || index > count {
                return Err(LuaError::runtime(format!(
                    "channel index {index} out of range (1-{count})"
                )));
            }
            let outputs = rt.borrow().get_channel_outputs();
            Ok(outputs.get((index - 1) as usize).copied().unwrap_or(0) as i32)
        })?,
    )?;

    channel_ns.set(
        "mixer",
        scope.create_function(|_, index: i32| {
            let count = rt.borrow().get_num_channels() as i32;
            if index < 1 || index > count {
                return Err(LuaError::runtime(format!(
                    "channel index {index} out of range (1-{count})"
                )));
            }
            let outputs = rt.borrow().get_mix_outputs();
            Ok(outputs.get((index - 1) as usize).copied().unwrap_or(0) as i32)
        })?,
    )?;

    channel_ns.set(
        "used",
        scope.create_function(|_, index: i32| {
            let count = rt.borrow().get_num_channels() as i32;
            if index < 1 || index > count {
                return Err(LuaError::runtime(format!(
                    "channel index {index} out of range (1-{count})"
                )));
            }
            let mask = rt.borrow().get_channels_used();
            Ok(mask & (1 << (index - 1)) != 0)
        })?,
    )?;

    channel_ns.set(
        "mix_count",
        scope.create_function(|_, ()| Ok(rt.borrow().get_mix_count() as i32))?,
    )?;

    lua.globals().set("channel", channel_ns)?;

    // -- logicalswitch.* namespace (1-based indices) --
    let ls_ns = lua.create_table()?;

    ls_ns.set(
        "count",
        scope.create_function(|_, ()| Ok(rt.borrow().get_num_logical_switches() as i32))?,
    )?;

    ls_ns.set(
        "get",
        scope.create_function(|_, index: i32| {
            let count = rt.borrow().get_num_logical_switches() as i32;
            if index < 1 || index > count {
                return Err(LuaError::runtime(format!(
                    "logical switch index {index} out of range (1-{count})"
                )));
            }
            let switches = rt.borrow().get_logical_switches();
            Ok(switches.get((index - 1) as usize).copied().unwrap_or(false))
        })?,
    )?;

    lua.globals().set("logicalswitch", ls_ns)?;

    // -- gvar.* namespace (1-based indices) --
    let gvar_ns = lua.create_table()?;

    gvar_ns.set(
        "count",
        scope.create_function(|_, ()| Ok(rt.borrow().get_num_gvars() as i32))?,
    )?;

    gvar_ns.set(
        "flightmodes",
        scope.create_function(|_, ()| Ok(rt.borrow().get_num_flight_modes() as i32))?,
    )?;

    gvar_ns.set(
        "flightmode",
        scope.create_function(|_, ()| Ok(rt.borrow().get_flight_mode() as i32))?,
    )?;

    gvar_ns.set(
        "get",
        scope.create_function(|_, (gvar, flightmode): (i32, i32)| {
            let ng = rt.borrow().get_num_gvars() as i32;
            let nfm = rt.borrow().get_num_flight_modes() as i32;
            if gvar < 1 || gvar > ng {
                return Err(LuaError::runtime(format!(
                    "gvar index {gvar} out of range (1-{ng})"
                )));
            }
            if flightmode < 1 || flightmode > nfm {
                return Err(LuaError::runtime(format!(
                    "flight mode {flightmode} out of range (1-{nfm})"
                )));
            }
            let gv = rt
                .borrow()
                .get_gvar((gvar - 1) as u8, (flightmode - 1) as u8);
            Ok(gv.value as i32)
        })?,
    )?;

    lua.globals().set("gvar", gvar_ns)?;

    Ok(())
}

/// Resolve a Lua value to a key name string.
fn resolve_key_name(val: &LuaValue) -> LuaResult<String> {
    match val {
        LuaValue::String(s) => Ok(s.to_str()?.to_uppercase()),
        other => Err(LuaError::runtime(format!(
            "expected string, got {}",
            other.type_name()
        ))),
    }
}

/// Look up a key name to its simulator index.
fn key_index(name: &str) -> LuaResult<i32> {
    // Strip optional KEY_ prefix
    let name = name.strip_prefix("KEY_").unwrap_or(name);
    input::script_key_index(name).ok_or_else(|| {
        let available: Vec<&str> = input::SCRIPT_KEYS.iter().map(|(n, _)| *n).collect();
        LuaError::runtime(format!(
            "unknown key \"{name}\" (available: {})",
            available.join(", ")
        ))
    })
}

/// Resolve a switch name (string) or index (integer) to a simulator index.
fn resolve_switch_index(val: &LuaValue, radio: &RadioDef) -> LuaResult<i32> {
    match val {
        LuaValue::Integer(idx) => {
            let idx = *idx as i32;
            if idx < 0 || idx as usize >= radio.switches.len() {
                let available: Vec<&str> = radio.switches.iter().map(|s| s.name.as_str()).collect();
                return Err(LuaError::runtime(format!(
                    "switch index {idx} out of range for {} (available: {})",
                    radio.name,
                    available.join(", ")
                )));
            }
            Ok(idx)
        }
        LuaValue::String(s) => {
            let name = s.to_str()?.to_string();
            radio
                .switches
                .iter()
                .position(|sw| sw.name.eq_ignore_ascii_case(&name))
                .map(|i| i as i32)
                .ok_or_else(|| {
                    let available: Vec<&str> =
                        radio.switches.iter().map(|s| s.name.as_str()).collect();
                    LuaError::runtime(format!(
                        "unknown switch \"{name}\" for {} (available: {})",
                        radio.name,
                        available.join(", ")
                    ))
                })
        }
        other => Err(LuaError::runtime(format!(
            "expected string or integer, got {}",
            other.type_name()
        ))),
    }
}

/// Resolve a trim name (string) or index (integer) to a simulator index.
fn resolve_trim_index(val: &LuaValue, radio: &RadioDef) -> LuaResult<i32> {
    match val {
        LuaValue::Integer(idx) => {
            let idx = *idx as i32;
            if idx < 0 || idx as usize >= radio.trims.len() {
                let available: Vec<&str> = radio.trims.iter().map(|t| t.name.as_str()).collect();
                return Err(LuaError::runtime(format!(
                    "trim index {idx} out of range for {} (available: {})",
                    radio.name,
                    available.join(", ")
                )));
            }
            Ok(idx)
        }
        LuaValue::String(s) => {
            let name = s.to_str()?.to_string();
            radio
                .trims
                .iter()
                .position(|tr| tr.name.eq_ignore_ascii_case(&name))
                .map(|i| i as i32)
                .ok_or_else(|| {
                    let available: Vec<&str> =
                        radio.trims.iter().map(|t| t.name.as_str()).collect();
                    LuaError::runtime(format!(
                        "unknown trim \"{name}\" for {} (available: {})",
                        radio.name,
                        available.join(", ")
                    ))
                })
        }
        other => Err(LuaError::runtime(format!(
            "expected string or integer, got {}",
            other.type_name()
        ))),
    }
}

/// Resolve an input name (string) or index (integer) to a simulator index.
fn resolve_input_index(val: &LuaValue, radio: &RadioDef) -> LuaResult<i32> {
    match val {
        LuaValue::Integer(idx) => {
            let idx = *idx as i32;
            if idx < 0 || idx as usize >= radio.inputs.len() {
                let available: Vec<&str> = radio.inputs.iter().map(|i| i.name.as_str()).collect();
                return Err(LuaError::runtime(format!(
                    "input index {idx} out of range for {} (available: {})",
                    radio.name,
                    available.join(", ")
                )));
            }
            Ok(idx)
        }
        LuaValue::String(s) => {
            let name = s.to_str()?.to_string();
            radio
                .inputs
                .iter()
                .position(|inp| inp.name.eq_ignore_ascii_case(&name))
                .map(|i| i as i32)
                .ok_or_else(|| {
                    let available: Vec<&str> =
                        radio.inputs.iter().map(|i| i.name.as_str()).collect();
                    LuaError::runtime(format!(
                        "unknown input \"{name}\" for {} (available: {})",
                        radio.name,
                        available.join(", ")
                    ))
                })
        }
        other => Err(LuaError::runtime(format!(
            "expected string or integer, got {}",
            other.type_name()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::simulator::input::{InputEvent, RuntimeMessage};
    use std::cell::RefCell;
    use std::rc::Rc;

    /// Script actions: hardware events, runtime messages, and script-only actions.
    #[derive(Debug, PartialEq)]
    enum RecordedAction {
        Input(InputEvent),
        Message(RuntimeMessage),
        Wait(Duration),
        Screenshot(String),
        Reset,
        Reload,
    }

    /// Convenience constructors to keep test assertions readable.
    impl RecordedAction {
        fn key_down(name: &str) -> Self {
            Self::Input(InputEvent::Key {
                index: input::script_key_index(name).unwrap(),
                pressed: true,
            })
        }
        fn key_up(name: &str) -> Self {
            Self::Input(InputEvent::Key {
                index: input::script_key_index(name).unwrap(),
                pressed: false,
            })
        }
        fn touch_down(x: i32, y: i32) -> Self {
            Self::Input(InputEvent::Touch { x, y, down: true })
        }
        fn touch_up() -> Self {
            Self::Input(InputEvent::Touch {
                x: 0,
                y: 0,
                down: false,
            })
        }
        fn analog(index: i32, value: u16) -> Self {
            Self::Input(InputEvent::Analog { index, value })
        }
        fn switch(index: i32, state: i32) -> Self {
            Self::Input(InputEvent::Switch { index, state })
        }
        fn trim(index: i32, pressed: bool) -> Self {
            Self::Input(InputEvent::Trim { index, pressed })
        }
        fn set_trim_value(index: i32, value: i32) -> Self {
            Self::Message(RuntimeMessage::SetTrimValue { index, value })
        }
        fn rotary(delta: i32) -> Self {
            Self::Input(InputEvent::Rotary(delta))
        }
    }

    type Actions = Rc<RefCell<Vec<RecordedAction>>>;

    /// Register all Lua globals backed by a recording vec (no real runtime needed).
    fn setup_lua_test(lua: &Lua, actions: &Actions, radio: &RadioDef) -> LuaResult<()> {
        // KEY constants
        let key_table = lua.create_table()?;
        for &(name, _) in input::SCRIPT_KEYS {
            key_table.set(name, name)?;
        }
        lua.globals().set("KEY", key_table)?;

        // key namespace
        let key_ns = lua.create_table()?;

        let a = actions.clone();
        key_ns.set(
            "press",
            lua.create_function(move |_, name: LuaValue| {
                let name = resolve_key_name(&name)?;
                let idx = key_index(&name)?;
                a.borrow_mut().push(RecordedAction::key_down(&name));
                a.borrow_mut()
                    .push(RecordedAction::Wait(Duration::from_millis(100)));
                a.borrow_mut().push(RecordedAction::key_up(&name));
                let _ = idx;
                Ok(())
            })?,
        )?;

        let a = actions.clone();
        key_ns.set(
            "longpress",
            lua.create_function(move |_, name: LuaValue| {
                let name = resolve_key_name(&name)?;
                let _ = key_index(&name)?;
                a.borrow_mut().push(RecordedAction::key_down(&name));
                a.borrow_mut()
                    .push(RecordedAction::Wait(Duration::from_secs(1)));
                a.borrow_mut().push(RecordedAction::key_up(&name));
                Ok(())
            })?,
        )?;

        let a = actions.clone();
        key_ns.set(
            "down",
            lua.create_function(move |_, name: LuaValue| {
                let name = resolve_key_name(&name)?;
                let _ = key_index(&name)?;
                a.borrow_mut().push(RecordedAction::key_down(&name));
                Ok(())
            })?,
        )?;

        let a = actions.clone();
        key_ns.set(
            "up",
            lua.create_function(move |_, name: LuaValue| {
                let name = resolve_key_name(&name)?;
                let _ = key_index(&name)?;
                a.borrow_mut().push(RecordedAction::key_up(&name));
                Ok(())
            })?,
        )?;

        lua.globals().set("key", key_ns)?;

        // touch namespace
        let touch_ns = lua.create_table()?;

        let a = actions.clone();
        touch_ns.set(
            "tap",
            lua.create_function(move |_, (x, y): (i32, i32)| {
                a.borrow_mut().push(RecordedAction::touch_down(x, y));
                a.borrow_mut()
                    .push(RecordedAction::Wait(Duration::from_millis(100)));
                a.borrow_mut().push(RecordedAction::touch_up());
                Ok(())
            })?,
        )?;

        let a = actions.clone();
        touch_ns.set(
            "longpress",
            lua.create_function(move |_, (x, y): (i32, i32)| {
                a.borrow_mut().push(RecordedAction::touch_down(x, y));
                a.borrow_mut()
                    .push(RecordedAction::Wait(Duration::from_secs(1)));
                a.borrow_mut().push(RecordedAction::touch_up());
                Ok(())
            })?,
        )?;

        let a = actions.clone();
        touch_ns.set(
            "down",
            lua.create_function(move |_, (x, y): (i32, i32)| {
                a.borrow_mut().push(RecordedAction::touch_down(x, y));
                Ok(())
            })?,
        )?;

        let a = actions.clone();
        touch_ns.set(
            "release",
            lua.create_function(move |_, ()| {
                a.borrow_mut().push(RecordedAction::touch_up());
                Ok(())
            })?,
        )?;

        lua.globals().set("touch", touch_ns)?;

        // SWITCH constants
        let switch_table = lua.create_table()?;
        for (i, sw) in radio.switches.iter().enumerate() {
            switch_table.set(sw.name.as_str(), i as i32)?;
        }
        lua.globals().set("SWITCH", switch_table)?;

        // INPUT constants
        let input_table = lua.create_table()?;
        for (i, inp) in radio.inputs.iter().enumerate() {
            input_table.set(inp.name.as_str(), i as i32)?;
        }
        lua.globals().set("INPUT", input_table)?;

        // TRIM constants
        let trim_table = lua.create_table()?;
        for (i, tr) in radio.trims.iter().enumerate() {
            trim_table.set(tr.name.as_str(), i as i32)?;
        }
        lua.globals().set("TRIM", trim_table)?;

        // switch()
        let a = actions.clone();
        let radio_c = radio.clone();
        lua.globals().set(
            "switch",
            lua.create_function(move |_, (name_or_idx, state): (LuaValue, i32)| {
                if !(-1..=1).contains(&state) {
                    return Err(LuaError::runtime(format!(
                        "switch state {state} out of range (-1, 0, 1)"
                    )));
                }
                let idx = resolve_switch_index(&name_or_idx, &radio_c)?;
                a.borrow_mut().push(RecordedAction::switch(idx, state));
                Ok(())
            })?,
        )?;

        // analog()
        let a = actions.clone();
        let radio_c = radio.clone();
        lua.globals().set(
            "analog",
            lua.create_function(move |_, (name_or_idx, value): (LuaValue, i32)| {
                if !(0..=4096).contains(&value) {
                    return Err(LuaError::runtime(format!(
                        "analog value {value} out of range (0-4096)"
                    )));
                }
                let idx = resolve_input_index(&name_or_idx, &radio_c)?;
                a.borrow_mut()
                    .push(RecordedAction::analog(idx, value as u16));
                Ok(())
            })?,
        )?;

        // trim namespace
        let trim_ns = lua.create_table()?;

        let a = actions.clone();
        let radio_c = radio.clone();
        trim_ns.set(
            "press",
            lua.create_function(move |_, name_or_idx: LuaValue| {
                let idx = resolve_trim_index(&name_or_idx, &radio_c)?;
                a.borrow_mut().push(RecordedAction::trim(idx, true));
                a.borrow_mut()
                    .push(RecordedAction::Wait(Duration::from_millis(100)));
                a.borrow_mut().push(RecordedAction::trim(idx, false));
                Ok(())
            })?,
        )?;

        let a = actions.clone();
        let radio_c = radio.clone();
        trim_ns.set(
            "longpress",
            lua.create_function(move |_, name_or_idx: LuaValue| {
                let idx = resolve_trim_index(&name_or_idx, &radio_c)?;
                a.borrow_mut().push(RecordedAction::trim(idx, true));
                a.borrow_mut()
                    .push(RecordedAction::Wait(Duration::from_secs(1)));
                a.borrow_mut().push(RecordedAction::trim(idx, false));
                Ok(())
            })?,
        )?;

        let a = actions.clone();
        let radio_c = radio.clone();
        trim_ns.set(
            "down",
            lua.create_function(move |_, name_or_idx: LuaValue| {
                let idx = resolve_trim_index(&name_or_idx, &radio_c)?;
                a.borrow_mut().push(RecordedAction::trim(idx, true));
                Ok(())
            })?,
        )?;

        let a = actions.clone();
        let radio_c = radio.clone();
        trim_ns.set(
            "up",
            lua.create_function(move |_, name_or_idx: LuaValue| {
                let idx = resolve_trim_index(&name_or_idx, &radio_c)?;
                a.borrow_mut().push(RecordedAction::trim(idx, false));
                Ok(())
            })?,
        )?;

        let radio_c = radio.clone();
        trim_ns.set(
            "get",
            lua.create_function(move |_, name_or_idx: LuaValue| {
                let _idx = resolve_trim_index(&name_or_idx, &radio_c)?;
                Ok(0i32)
            })?,
        )?;

        let a = actions.clone();
        let radio_c = radio.clone();
        trim_ns.set(
            "set",
            lua.create_function(move |_, (name_or_idx, value): (LuaValue, i32)| {
                let idx = resolve_trim_index(&name_or_idx, &radio_c)?;
                a.borrow_mut()
                    .push(RecordedAction::set_trim_value(idx, value));
                Ok(())
            })?,
        )?;

        trim_ns.set(
            "range",
            lua.create_function(|_, ()| Ok((-1024i32, 1024i32)))?,
        )?;

        lua.globals().set("trim", trim_ns)?;

        // rotary()
        let a = actions.clone();
        lua.globals().set(
            "rotary",
            lua.create_function(move |_, delta: i32| {
                a.borrow_mut().push(RecordedAction::rotary(delta));
                Ok(())
            })?,
        )?;

        // wait()
        let a = actions.clone();
        lua.globals().set(
            "wait",
            lua.create_function(move |_, secs: f64| {
                if secs < 0.0 {
                    return Err(LuaError::runtime("wait duration must be non-negative"));
                }
                a.borrow_mut()
                    .push(RecordedAction::Wait(Duration::from_secs_f64(secs)));
                Ok(())
            })?,
        )?;

        // screenshot()
        let a = actions.clone();
        lua.globals().set(
            "screenshot",
            lua.create_function(move |_, path: String| {
                a.borrow_mut().push(RecordedAction::Screenshot(path));
                Ok(())
            })?,
        )?;

        // reset()
        let a = actions.clone();
        lua.globals().set(
            "reset",
            lua.create_function(move |_, ()| {
                a.borrow_mut().push(RecordedAction::Reset);
                Ok(())
            })?,
        )?;

        // reload()
        let a = actions.clone();
        lua.globals().set(
            "reload",
            lua.create_function(move |_, ()| {
                a.borrow_mut().push(RecordedAction::Reload);
                Ok(())
            })?,
        )?;

        // exit()
        lua.globals().set(
            "exit",
            lua.create_function(|_, code: i32| -> LuaResult<()> {
                Err(LuaError::ExternalError(Arc::new(ScriptExit(code))))
            })?,
        )?;

        // gvar namespace (stub for tests)
        let gvar_ns = lua.create_table()?;
        gvar_ns.set("count", lua.create_function(|_, ()| Ok(9i32))?)?;
        gvar_ns.set("flightmodes", lua.create_function(|_, ()| Ok(9i32))?)?;
        gvar_ns.set("flightmode", lua.create_function(|_, ()| Ok(0i32))?)?;
        gvar_ns.set(
            "get",
            lua.create_function(|_, (_gvar, _fm): (i32, i32)| Ok(0i32))?,
        )?;
        lua.globals().set("gvar", gvar_ns)?;

        Ok(())
    }

    /// Matches a real TX16S-style radio catalog entry.
    fn test_radio() -> RadioDef {
        use crate::radio_catalog::*;

        let input = |name: &str| InputDef {
            name: name.into(),
            input_type: InputType::default(),
            label: "".into(),
            default: InputDefault::default(),
        };
        let switch = |name: &str| SwitchDef {
            name: name.into(),
            switch_type: SwitchType::default(),
            default: SwitchDefault::default(),
        };
        let trim = |name: &str| TrimDef { name: name.into() };

        RadioDef {
            name: "Radiomaster TX16S".into(),
            wasm: "tx16s.wasm".into(),
            display: DisplayDef {
                w: 480,
                h: 272,
                depth: 16,
            },
            inputs: vec![
                input("LH"),
                input("LV"),
                input("RV"),
                input("RH"),
                input("P1"),
                input("P2"),
                input("SL1"),
                input("SL2"),
            ],
            switches: vec![
                switch("SA"),
                switch("SB"),
                switch("SC"),
                switch("SD"),
                switch("SE"),
                switch("SF"),
                switch("SG"),
                switch("SH"),
            ],
            trims: vec![
                trim("T1"),
                trim("T2"),
                trim("T3"),
                trim("T4"),
                trim("T5"),
                trim("T6"),
            ],
            keys: vec![],
        }
    }

    /// Result type that includes both actions and exit code.
    struct ScriptResult {
        actions: Vec<RecordedAction>,
        exit_code: Option<i32>,
    }

    fn run_test_script(script: &str) -> Result<Vec<RecordedAction>, LuaError> {
        let result = run_test_script_full(script)?;
        Ok(result.actions)
    }

    fn run_test_script_full(script: &str) -> Result<ScriptResult, LuaError> {
        let radio = test_radio();
        let actions: Actions = Rc::new(RefCell::new(Vec::new()));
        let exit_code;
        {
            let lua = Lua::new();
            setup_lua_test(&lua, &actions, &radio)?;
            match lua.load(script).exec() {
                Ok(()) => exit_code = None,
                Err(e) => {
                    if let Some(code) = extract_exit_code(&e) {
                        exit_code = Some(code);
                    } else {
                        return Err(e);
                    }
                }
            }
        }
        Ok(ScriptResult {
            actions: Rc::try_unwrap(actions).unwrap().into_inner(),
            exit_code,
        })
    }

    #[test]
    fn test_key_constants() {
        let lua = Lua::new();
        let key_table = lua.create_table().unwrap();
        for &(name, _) in input::SCRIPT_KEYS {
            key_table.set(name, name).unwrap();
        }
        lua.globals().set("KEY", key_table).unwrap();

        // Verify all 14 constants
        let result: String = lua.load("return KEY.MENU").eval().unwrap();
        assert_eq!(result, "MENU");
        let result: String = lua.load("return KEY.SYS").eval().unwrap();
        assert_eq!(result, "SYS");
        let result: String = lua.load("return KEY.ENTER").eval().unwrap();
        assert_eq!(result, "ENTER");

        // Count entries
        let count: i32 = lua
            .load(
                r#"
            local n = 0
            for _ in pairs(KEY) do n = n + 1 end
            return n
        "#,
            )
            .eval()
            .unwrap();
        assert_eq!(count, 14);
    }

    #[test]
    fn test_key_press() {
        let actions = run_test_script("key.press(KEY.ENTER)").unwrap();
        assert_eq!(
            actions,
            vec![
                RecordedAction::key_down("ENTER"),
                RecordedAction::Wait(Duration::from_millis(100)),
                RecordedAction::key_up("ENTER"),
            ]
        );
    }

    #[test]
    fn test_key_longpress() {
        let actions = run_test_script("key.longpress(KEY.SYS)").unwrap();
        assert_eq!(
            actions,
            vec![
                RecordedAction::key_down("SYS"),
                RecordedAction::Wait(Duration::from_secs(1)),
                RecordedAction::key_up("SYS"),
            ]
        );
    }

    #[test]
    fn test_key_down_up() {
        let actions = run_test_script("key.down(KEY.ENTER)\nkey.up(KEY.ENTER)").unwrap();
        assert_eq!(
            actions,
            vec![
                RecordedAction::key_down("ENTER"),
                RecordedAction::key_up("ENTER")
            ]
        );
    }

    #[test]
    fn test_key_string_arg() {
        let actions = run_test_script(r#"key.press("ENTER")"#).unwrap();
        assert_eq!(
            actions,
            vec![
                RecordedAction::key_down("ENTER"),
                RecordedAction::Wait(Duration::from_millis(100)),
                RecordedAction::key_up("ENTER"),
            ]
        );
    }

    #[test]
    fn test_touch_tap() {
        let actions = run_test_script("touch.tap(100, 200)").unwrap();
        assert_eq!(
            actions,
            vec![
                RecordedAction::touch_down(100, 200),
                RecordedAction::Wait(Duration::from_millis(100)),
                RecordedAction::touch_up(),
            ]
        );
    }

    #[test]
    fn test_touch_longpress() {
        let actions = run_test_script("touch.longpress(50, 75)").unwrap();
        assert_eq!(
            actions,
            vec![
                RecordedAction::touch_down(50, 75),
                RecordedAction::Wait(Duration::from_secs(1)),
                RecordedAction::touch_up(),
            ]
        );
    }

    #[test]
    fn test_touch_down_release() {
        let actions = run_test_script("touch.down(10, 20)\ntouch.release()").unwrap();
        assert_eq!(
            actions,
            vec![
                RecordedAction::touch_down(10, 20),
                RecordedAction::touch_up()
            ]
        );
    }

    #[test]
    fn test_analog() {
        let actions = run_test_script("analog(0, 3000)").unwrap();
        assert_eq!(actions, vec![RecordedAction::analog(0, 3000)]);
    }

    #[test]
    fn test_analog_by_name() {
        let actions = run_test_script(r#"analog("LH", 2000)"#).unwrap();
        assert_eq!(actions, vec![RecordedAction::analog(0, 2000)]);
    }

    #[test]
    fn test_analog_by_constant() {
        let actions = run_test_script("analog(INPUT.RH, 1500)").unwrap();
        assert_eq!(actions, vec![RecordedAction::analog(3, 1500)]);
    }

    #[test]
    fn test_switch() {
        let actions = run_test_script("switch(0, -1)").unwrap();
        assert_eq!(actions, vec![RecordedAction::switch(0, -1)]);
    }

    #[test]
    fn test_switch_by_name() {
        let actions = run_test_script(r#"switch("SA", 1)"#).unwrap();
        assert_eq!(actions, vec![RecordedAction::switch(0, 1)]);
    }

    #[test]
    fn test_switch_by_constant() {
        let actions = run_test_script("switch(SWITCH.SD, 0)").unwrap();
        assert_eq!(actions, vec![RecordedAction::switch(3, 0)]);
    }

    #[test]
    fn test_trim_press() {
        let actions = run_test_script(r#"trim.press("T1")"#).unwrap();
        assert_eq!(
            actions,
            vec![
                RecordedAction::trim(0, true),
                RecordedAction::Wait(Duration::from_millis(100)),
                RecordedAction::trim(0, false),
            ]
        );
    }

    #[test]
    fn test_trim_longpress() {
        let actions = run_test_script("trim.longpress(TRIM.T1)").unwrap();
        assert_eq!(
            actions,
            vec![
                RecordedAction::trim(0, true),
                RecordedAction::Wait(Duration::from_secs(1)),
                RecordedAction::trim(0, false),
            ]
        );
    }

    #[test]
    fn test_trim_down_up() {
        let actions = run_test_script("trim.down(TRIM.T2)\ntrim.up(TRIM.T2)").unwrap();
        assert_eq!(
            actions,
            vec![
                RecordedAction::trim(1, true),
                RecordedAction::trim(1, false)
            ]
        );
    }

    #[test]
    fn test_trim_get() {
        let actions = run_test_script("local v = trim.get(0)\nassert(v == 0)").unwrap();
        assert_eq!(actions, vec![]);
    }

    #[test]
    fn test_trim_get_by_name() {
        let actions = run_test_script("local v = trim.get(\"T1\")\nassert(v == 0)").unwrap();
        assert_eq!(actions, vec![]);
    }

    #[test]
    fn test_trim_get_by_constant() {
        let actions = run_test_script("local v = trim.get(TRIM.T4)\nassert(v == 0)").unwrap();
        assert_eq!(actions, vec![]);
    }

    #[test]
    fn test_trim_set() {
        let actions = run_test_script("trim.set(0, 100)").unwrap();
        assert_eq!(actions, vec![RecordedAction::set_trim_value(0, 100)]);
    }

    #[test]
    fn test_trim_set_by_name() {
        let actions = run_test_script(r#"trim.set("T1", -50)"#).unwrap();
        assert_eq!(actions, vec![RecordedAction::set_trim_value(0, -50)]);
    }

    #[test]
    fn test_trim_set_by_constant() {
        let actions = run_test_script("trim.set(TRIM.T2, 0)").unwrap();
        assert_eq!(actions, vec![RecordedAction::set_trim_value(1, 0)]);
    }

    #[test]
    fn test_rotary() {
        let actions = run_test_script("rotary(2)").unwrap();
        assert_eq!(actions, vec![RecordedAction::rotary(2)]);
    }

    #[test]
    fn test_wait() {
        let actions = run_test_script("wait(0.5)").unwrap();
        assert_eq!(
            actions,
            vec![RecordedAction::Wait(Duration::from_millis(500))]
        );
    }

    #[test]
    fn test_screenshot() {
        let actions = run_test_script(r#"screenshot("out.png")"#).unwrap();
        assert_eq!(actions, vec![RecordedAction::Screenshot("out.png".into())]);
    }

    #[test]
    fn test_full_script() {
        let script = r#"
            wait(5)
            key.press(KEY.SYS)
            wait(1)
            key.press(KEY.PAGEDN)
            wait(0.5)
            screenshot("tools-menu.png")
        "#;
        let actions = run_test_script(script).unwrap();
        assert_eq!(
            actions,
            vec![
                RecordedAction::Wait(Duration::from_secs(5)),
                RecordedAction::key_down("SYS"),
                RecordedAction::Wait(Duration::from_millis(100)),
                RecordedAction::key_up("SYS"),
                RecordedAction::Wait(Duration::from_secs(1)),
                RecordedAction::key_down("PAGEDN"),
                RecordedAction::Wait(Duration::from_millis(100)),
                RecordedAction::key_up("PAGEDN"),
                RecordedAction::Wait(Duration::from_millis(500)),
                RecordedAction::Screenshot("tools-menu.png".into()),
            ]
        );
    }

    #[test]
    fn test_invalid_key_name() {
        let result = run_test_script(r#"key.press("BOGUS")"#);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("unknown key \"BOGUS\""), "got: {err}");
        assert!(err.contains("available:"), "got: {err}");
    }

    #[test]
    fn test_missing_args() {
        let result = run_test_script("key.press()");
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_switch_state() {
        let result = run_test_script("switch(0, 5)");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("out of range"), "got: {err}");
    }

    #[test]
    fn test_invalid_analog_value() {
        let result = run_test_script("analog(0, 5000)");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("out of range"), "got: {err}");
    }

    #[test]
    fn test_negative_wait() {
        let result = run_test_script("wait(-1)");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("non-negative"), "got: {err}");
    }

    #[test]
    fn test_unknown_switch_name() {
        let result = run_test_script(r#"switch("SZ", -1)"#);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("unknown switch \"SZ\""), "got: {err}");
    }

    #[test]
    fn test_unknown_input_name() {
        let result = run_test_script(r#"analog("XX", 3000)"#);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("unknown input \"XX\""), "got: {err}");
    }

    #[test]
    fn test_unknown_trim_name() {
        let result = run_test_script(r#"trim.press("Bogus")"#);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("unknown trim \"Bogus\""), "got: {err}");
    }

    #[test]
    fn test_lua_loop_and_functions() {
        let script = r#"
            function nav_down(n)
                for i = 1, n do
                    key.press(KEY.DOWN)
                end
            end
            nav_down(3)
        "#;
        let actions = run_test_script(script).unwrap();
        assert_eq!(actions.len(), 9); // 3 * (key_down + Wait + key_up)
        assert_eq!(actions[0], RecordedAction::key_down("DOWN"));
    }

    #[test]
    fn test_syntax_error() {
        let lua = Lua::new();
        let result = lua.load("this is not valid lua !!!").exec();
        assert!(result.is_err());
    }

    // -- exit() tests --

    #[test]
    fn test_exit_zero() {
        let result = run_test_script_full("exit(0)").unwrap();
        assert_eq!(result.exit_code, Some(0));
    }

    #[test]
    fn test_exit_nonzero() {
        let result = run_test_script_full("exit(1)").unwrap();
        assert_eq!(result.exit_code, Some(1));
    }

    #[test]
    fn test_exit_42() {
        let result = run_test_script_full("exit(42)").unwrap();
        assert_eq!(result.exit_code, Some(42));
    }

    #[test]
    fn test_exit_no_args() {
        let result = run_test_script("exit()");
        assert!(result.is_err());
    }

    #[test]
    fn test_exit_stops_execution() {
        let result = run_test_script_full(
            r#"
            wait(1)
            exit(0)
            wait(999)
        "#,
        )
        .unwrap();
        assert_eq!(result.exit_code, Some(0));
        // Only the wait before exit should have been recorded
        assert_eq!(
            result.actions,
            vec![RecordedAction::Wait(Duration::from_secs(1))]
        );
    }

    #[test]
    fn test_no_exit_returns_none() {
        let result = run_test_script_full("wait(0.1)").unwrap();
        assert_eq!(result.exit_code, None);
    }

    // -- stdin streaming tests --

    #[test]
    fn test_stdin_single_line() {
        let input = b"print('hello')\n";
        let lua = Lua::new();
        let mut buffer = String::new();
        for line in std::io::BufRead::lines(&input[..]) {
            let line = line.unwrap();
            buffer.push_str(&line);
            // Should parse as a complete chunk
            assert!(lua.load(&buffer).into_function().is_ok());
            buffer.clear();
        }
    }

    #[test]
    fn test_stdin_multiline_detection() {
        let lua = Lua::new();
        // Incomplete chunk: "for i=1,3 do" without "end"
        let result = lua.load("for i=1,3 do").into_function();
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("<eof>"),
            "expected <eof> in error for incomplete chunk, got: {err_msg}"
        );
    }

    #[test]
    fn test_stdin_multiline_complete() {
        let lua = Lua::new();
        // Complete multi-line chunk
        let result = lua.load("for i=1,3 do\nprint(i)\nend").into_function();
        assert!(result.is_ok());
    }

    #[test]
    fn test_stdin_real_syntax_error() {
        let lua = Lua::new();
        // Real syntax error (not incomplete)
        let result = lua.load("print(42))").into_function();
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            !err_msg.contains("<eof>"),
            "real syntax error should not contain <eof>, got: {err_msg}"
        );
    }

    // -- reset() / reload() tests --

    #[test]
    fn test_reset() {
        let actions = run_test_script("reset()").unwrap();
        assert_eq!(actions, vec![RecordedAction::Reset]);
    }

    #[test]
    fn test_reload() {
        let actions = run_test_script("reload()").unwrap();
        assert_eq!(actions, vec![RecordedAction::Reload]);
    }

    #[test]
    fn test_reset_in_sequence() {
        let script = r#"
            wait(5)
            reset()
            wait(3)
            screenshot("after-reset.png")
        "#;
        let actions = run_test_script(script).unwrap();
        assert_eq!(
            actions,
            vec![
                RecordedAction::Wait(Duration::from_secs(5)),
                RecordedAction::Reset,
                RecordedAction::Wait(Duration::from_secs(3)),
                RecordedAction::Screenshot("after-reset.png".into()),
            ]
        );
    }

    // -- gvar.flightmode() tests --

    #[test]
    fn test_gvar_flightmode() {
        let radio = test_radio();
        let actions: Actions = Rc::new(RefCell::new(Vec::new()));
        let lua = Lua::new();
        setup_lua_test(&lua, &actions, &radio).unwrap();
        let fm: i32 = lua.load("return gvar.flightmode()").eval().unwrap();
        assert_eq!(fm, 0);
    }

    // -- trim.range() tests --

    #[test]
    fn test_trim_range() {
        let radio = test_radio();
        let actions: Actions = Rc::new(RefCell::new(Vec::new()));
        let lua = Lua::new();
        setup_lua_test(&lua, &actions, &radio).unwrap();
        let (min, max): (i32, i32) = lua.load("return trim.range()").eval().unwrap();
        assert_eq!(min, -1024);
        assert_eq!(max, 1024);
    }
}
