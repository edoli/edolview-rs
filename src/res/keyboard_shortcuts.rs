use eframe::egui::{Key, KeyboardShortcut, ModifierNames, Modifiers};

pub const IS_MAC: bool = cfg!(target_os = "macos");

pub const SELECT_ALL_SC: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::A);
pub const SELECT_NONE_SC: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::Escape);
pub const COPY_SC: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::C);

pub const RESET_VIEW: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::R);

pub const FULLSCREEN_TOGGLE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::F11);

pub const ZOOM_IN: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::Plus);
pub const ZOOM_OUT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::Minus);

pub const NAVIGATE_PREV: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::ArrowLeft);
pub const NAVIGATE_NEXT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::ArrowRight);
pub const NAVIGATE_ASSET_PREV: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::PageUp);
pub const NAVIGATE_ASSET_NEXT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::PageDown);

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
