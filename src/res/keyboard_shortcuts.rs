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

pub const PRESET_KEYS: [Key; 9] = [
    Key::Num1,
    Key::Num2,
    Key::Num3,
    Key::Num4,
    Key::Num5,
    Key::Num6,
    Key::Num7,
    Key::Num8,
    Key::Num9,
];

const fn preset_shortcuts(modifiers: Modifiers) -> [KeyboardShortcut; PRESET_KEYS.len()] {
    let mut shortcuts = [KeyboardShortcut::new(Modifiers::NONE, Key::Num0); PRESET_KEYS.len()];
    let mut index = 0;
    while index < PRESET_KEYS.len() {
        shortcuts[index] = KeyboardShortcut::new(modifiers, PRESET_KEYS[index]);
        index += 1;
    }
    shortcuts
}

pub const PRESET_SAVE_SHORTCUTS: [KeyboardShortcut; PRESET_KEYS.len()] = preset_shortcuts(Modifiers::COMMAND);
pub const PRESET_APPLY_SHORTCUTS: [KeyboardShortcut; PRESET_KEYS.len()] = preset_shortcuts(Modifiers::ALT);

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
