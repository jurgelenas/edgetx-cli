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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_egui_key_mapping() {
        assert_eq!(egui_key_to_index(&egui::Key::Enter), Some(2));
        assert_eq!(egui_key_to_index(&egui::Key::Escape), Some(1));
        assert_eq!(egui_key_to_index(&egui::Key::A), None);
    }
}
