use std::{fs, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::ui::gl::ShaderParams;

pub const VIEW_PRESET_COUNT: usize = 9;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalOpenMode {
    NewWindow,
    ExistingWindow,
}

impl ExternalOpenMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::NewWindow => "Open in a new window",
            Self::ExistingWindow => "Send to the last active window",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppSettings {
    #[serde(default)]
    pub external_open_mode: ExternalOpenMode,
    #[serde(default)]
    pub ui_state: PersistentUiState,
    #[serde(default = "default_view_presets")]
    pub view_presets: Vec<Option<ViewPreset>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ViewPreset {
    pub colormap_rgb: String,
    pub colormap_mono: String,
    pub shader_params: ShaderParams,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistentUiState {
    pub is_show_background: bool,
    pub is_show_pixel_value: bool,
    pub is_show_crosshair: bool,
    pub is_show_sidebar: bool,
    pub is_show_statusbar: bool,
    pub copy_use_original_size: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            external_open_mode: ExternalOpenMode::NewWindow,
            ui_state: PersistentUiState::default(),
            view_presets: default_view_presets(),
        }
    }
}

impl Default for PersistentUiState {
    fn default() -> Self {
        Self {
            is_show_background: true,
            is_show_pixel_value: true,
            is_show_crosshair: false,
            is_show_sidebar: true,
            is_show_statusbar: true,
            copy_use_original_size: true,
        }
    }
}

impl Default for ExternalOpenMode {
    fn default() -> Self {
        Self::NewWindow
    }
}

impl AppSettings {
    pub fn load() -> Result<Self, String> {
        let path = settings_path();
        if !path.exists() {
            return Ok(Self::default());
        }

        let body =
            fs::read_to_string(&path).map_err(|e| format!("Failed to read settings file '{}': {e}", path.display()))?;
        let mut settings: Self = serde_json::from_str(&body)
            .map_err(|e| format!("Failed to parse settings file '{}': {e}", path.display()))?;
        settings.normalize();
        Ok(settings)
    }

    pub fn save(&self) -> Result<(), String> {
        let path = settings_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create settings directory '{}': {e}", parent.display()))?;
        }

        let body = serde_json::to_string_pretty(self).map_err(|e| format!("Failed to serialize settings: {e}"))?;
        fs::write(&path, body).map_err(|e| format!("Failed to write settings file '{}': {e}", path.display()))
    }

    pub fn view_preset(&self, slot: usize) -> Option<&ViewPreset> {
        self.view_presets.get(slot).and_then(|preset| preset.as_ref())
    }

    pub fn set_view_preset(&mut self, slot: usize, preset: ViewPreset) {
        self.normalize();
        if let Some(entry) = self.view_presets.get_mut(slot) {
            *entry = Some(preset);
        }
    }

    fn normalize(&mut self) {
        self.view_presets.resize(VIEW_PRESET_COUNT, None);
        self.view_presets.truncate(VIEW_PRESET_COUNT);
    }
}

fn settings_path() -> PathBuf {
    crate::util::path_ext::app_config_dir().join("settings.json")
}

fn default_view_presets() -> Vec<Option<ViewPreset>> {
    vec![None; VIEW_PRESET_COUNT]
}
