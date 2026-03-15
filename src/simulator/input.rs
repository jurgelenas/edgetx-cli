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

/// Keyboard shortcut mapping.
pub struct KeyMapping {
    pub key: egui::Key,
    pub index: i32,
    pub label: &'static str,
}

/// Keyboard shortcuts matching Companion's simulateduiwidget.cpp.
pub static KEYBOARD_SHORTCUTS: &[KeyMapping] = &[
    KeyMapping { key: egui::Key::S, index: 13, label: "SYS" },
    KeyMapping { key: egui::Key::M, index: 11, label: "MODEL" },
    KeyMapping { key: egui::Key::T, index: 12, label: "TELE" },
    KeyMapping { key: egui::Key::PageUp, index: 3, label: "PAGE UP" },
    KeyMapping { key: egui::Key::PageDown, index: 4, label: "PAGE DN" },
    KeyMapping { key: egui::Key::ArrowUp, index: 5, label: "UP" },
    KeyMapping { key: egui::Key::ArrowDown, index: 6, label: "DOWN" },
    KeyMapping { key: egui::Key::ArrowLeft, index: 7, label: "LEFT" },
    KeyMapping { key: egui::Key::ArrowRight, index: 8, label: "RIGHT" },
    KeyMapping { key: egui::Key::Plus, index: 9, label: "PLUS" },
    KeyMapping { key: egui::Key::Minus, index: 10, label: "MINUS" },
    KeyMapping { key: egui::Key::Enter, index: 2, label: "ENTER" },
    KeyMapping { key: egui::Key::Escape, index: 1, label: "EXIT" },
];

/// Map an egui key to simulator key index.
pub fn egui_key_to_index(key: &egui::Key) -> Option<i32> {
    KEYBOARD_SHORTCUTS
        .iter()
        .find(|ks| &ks.key == key)
        .map(|ks| ks.index)
}

/// Format keyboard shortcuts for display.
pub fn print_keyboard_shortcuts() -> String {
    let mut lines = String::from("Keyboard shortcuts:\n");
    for ks in KEYBOARD_SHORTCUTS {
        lines += &format!("  {:?} -> {}\n", ks.key, ks.label);
    }
    lines += "  Scroll wheel -> Rotary encoder\n";
    lines += "  Mouse click on LCD -> Touch\n";
    lines
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

    #[test]
    fn test_egui_key_mapping() {
        assert_eq!(egui_key_to_index(&egui::Key::Enter), Some(2));
        assert_eq!(egui_key_to_index(&egui::Key::Escape), Some(1));
        assert_eq!(egui_key_to_index(&egui::Key::A), None);
    }
}
