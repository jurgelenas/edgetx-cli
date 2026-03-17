/// Input events sent from UI thread to WASM thread.
#[derive(Debug, Clone, PartialEq)]
pub enum InputEvent {
    Key { index: i32, pressed: bool },
    Rotary(i32),
    Touch { x: i32, y: i32, down: bool },
    Switch { index: i32, state: i32 },
    Trim { index: i32, pressed: bool },
    Analog { index: i32, value: u16 },
    Quit,
}

/// All script key names and their simulator indices.
pub const SCRIPT_KEYS: &[(&str, i32)] = &[
    ("MENU", 0),
    ("EXIT", 1),
    ("ENTER", 2),
    ("PAGEUP", 3),
    ("PAGEDN", 4),
    ("UP", 5),
    ("DOWN", 6),
    ("LEFT", 7),
    ("RIGHT", 8),
    ("PLUS", 9),
    ("MINUS", 10),
    ("MODEL", 11),
    ("TELE", 12),
    ("SYS", 13),
];

/// Script key name to simulator index mapping.
pub fn script_key_index(name: &str) -> Option<i32> {
    let name = name.strip_prefix("KEY_").unwrap_or(name);
    SCRIPT_KEYS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, idx)| *idx)
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
