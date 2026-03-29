use std::{fs, path::PathBuf};

use serde::{Deserialize, Serialize};

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
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            external_open_mode: ExternalOpenMode::NewWindow,
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
        serde_json::from_str(&body).map_err(|e| format!("Failed to parse settings file '{}': {e}", path.display()))
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
}

fn settings_path() -> PathBuf {
    crate::util::path_ext::app_config_dir().join("settings.json")
}
