/// Input events sent from UI thread to WASM thread.
pub enum InputEvent {
    Key { index: i32, pressed: bool },
    Rotary(i32),
    Touch { x: i32, y: i32, down: bool },
    Switch { index: i32, state: i32 },
    Trim { index: i32, pressed: bool },
    Analog { index: i32, value: u16 },
    Quit,
}

/// Script key name to simulator index mapping.
pub fn script_key_index(name: &str) -> Option<i32> {
    let name = name.strip_prefix("KEY_").unwrap_or(name);
    match name {
        "MENU" => Some(0),
        "EXIT" => Some(1),
        "ENTER" => Some(2),
        "PAGEUP" => Some(3),
        "PAGEDN" => Some(4),
        "UP" => Some(5),
        "DOWN" => Some(6),
        "LEFT" => Some(7),
        "RIGHT" => Some(8),
        "PLUS" => Some(9),
        "MINUS" => Some(10),
        "MODEL" => Some(11),
        "TELE" => Some(12),
        "SYS" => Some(13),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_script_key_index() {
        assert_eq!(script_key_index("ENTER"), Some(2));
        assert_eq!(script_key_index("EXIT"), Some(1));
        assert_eq!(script_key_index("MENU"), Some(0));
        assert_eq!(script_key_index("UNKNOWN"), None);
        // KEY_ prefix stripping
        assert_eq!(script_key_index("KEY_ENTER"), Some(2));
        assert_eq!(script_key_index("KEY_SYS"), Some(13));
        assert_eq!(script_key_index("KEY_UNKNOWN"), None);
    }
}
