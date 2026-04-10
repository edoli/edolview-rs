use eframe::egui::{Key, KeyboardShortcut, ModifierNames, Modifiers};

pub const IS_MAC: bool = cfg!(target_os = "macos");

pub const SELECT_ALL_SC: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::A);
pub const SELECT_NONE_SC: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::Escape);
pub const COPY_SC: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::C);
pub const SAVE_IMAGE_SC: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);

pub const RESET_VIEW: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::R);

pub const FULLSCREEN_TOGGLE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::F11);

pub const ZOOM_IN: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::Equals);
pub const ZOOM_OUT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::Minus);

pub const NAVIGATE_PREV: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::ArrowLeft);
pub const NAVIGATE_NEXT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::ArrowRight);
pub const NAVIGATE_ASSET_PREV: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::PageUp);
pub const NAVIGATE_ASSET_NEXT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::PageDown);

fn preset_key(slot: usize) -> Key {
    match slot {
        0 => Key::Num1,
        1 => Key::Num2,
        2 => Key::Num3,
        3 => Key::Num4,
        4 => Key::Num5,
        5 => Key::Num6,
        6 => Key::Num7,
        7 => Key::Num8,
        8 => Key::Num9,
        _ => unreachable!("view preset slot out of range"),
    }
}

pub fn preset_save_shortcut(slot: usize) -> KeyboardShortcut {
    KeyboardShortcut::new(Modifiers::COMMAND, preset_key(slot))
}

pub fn preset_apply_shortcut(slot: usize) -> KeyboardShortcut {
    KeyboardShortcut::new(Modifiers::ALT, preset_key(slot))
}

pub const MODIFIER_NAMES: ModifierNames = ModifierNames {
    is_short: false,
    alt: "Alt",
    ctrl: "Ctrl",
    shift: "Shift",
    mac_cmd: "⌘",
    mac_alt: "Option",
    concat: "+",
};

pub trait KeyboardShortcutExt {
    fn format_sys(&self) -> String;
}

impl KeyboardShortcutExt for KeyboardShortcut {
    fn format_sys(&self) -> String {
        self.format(&MODIFIER_NAMES, IS_MAC)
    }
}
