use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use color_eyre::eyre::Result;
use indexmap::IndexMap;

use crate::{
    model::{Asset, AssetType, ClipboardAsset, FileAsset, Image, MatImage, Recti, SocketState},
    ui::gl::ShaderParams,
    util::math_ext::{vec2i, Vec2i},
};

pub type SharedAsset = Arc<dyn Asset<MatImage>>;

pub struct AppState {
    pub path: Option<PathBuf>,
    pub asset: Option<SharedAsset>,
    pub shader_params: ShaderParams,
    pub cursor_pos: Option<Vec2i>,
    pub marquee_rect: Recti,

    pub channel_index: i32,
    pub colormap_rgb: String,
    pub colormap_mono: String,
    pub colormap_rgb_list: Vec<String>,
    pub colormap_mono_list: Vec<String>,

    pub is_show_background: bool,
    pub is_show_pixel_tooltip: bool,
    pub is_show_pixel_value: bool,
    pub is_show_crosshair: bool,
    pub is_show_sidebar: bool,
    pub is_show_statusbar: bool,

    pub is_server_running: bool,
    pub is_server_receiving: bool,
    pub image_server_port: u16,

    // Copy behavior: when true, Ctrl+C copies marquee at original pixel size regardless of zoom.
    pub copy_use_original_size: bool,

    // File navigation + watcher
    pub file_nav: crate::model::FileNav,

    pub socket_state: Arc<SocketState>,

    pub assets: IndexMap<String, SharedAsset>,
}

fn list_colormaps(dir: &PathBuf) -> Vec<String> {
    // get file names ending with .glsl in the directory; silently ignore IO errors
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry_res in entries {
            if let Ok(entry) = entry_res {
                if let Some(name) = entry.file_name().to_str() {
                    if name.ends_with(".glsl") {
                        if let Some(stem) = entry.path().file_stem().and_then(|s| s.to_str()) {
                            files.push(stem.to_string());
                        }
                    }
                }
            }
        }
    }
    files.sort();
    files
}

impl AppState {
    pub fn empty() -> Self {
        Self {
            path: None,
            asset: None,
            shader_params: ShaderParams::default(),
            cursor_pos: None,
            marquee_rect: Recti::ZERO,
            channel_index: -1,
            colormap_rgb: String::from("rgb"),
            colormap_mono: String::from("gray"),
            colormap_rgb_list: list_colormaps(&PathBuf::from("colormap/rgb")),
            colormap_mono_list: list_colormaps(&PathBuf::from("colormap/mono")),
            is_show_background: true,
            is_show_pixel_tooltip: true,
            is_show_pixel_value: true,
            is_show_crosshair: false,
            is_show_sidebar: true,
            is_show_statusbar: true,
            is_server_running: false,
            is_server_receiving: false,
            image_server_port: 21734,
            copy_use_original_size: true,
            file_nav: crate::model::FileNav::new(),
            socket_state: Arc::new(SocketState::new()),
            assets: IndexMap::new(),
        }
    }

    pub fn load_from_path(&mut self, path: PathBuf) -> Result<()> {
        #[cfg(debug_assertions)]
        let _timer = crate::util::timer::ScopedTimer::new("Total image load time [from path]");

        let path_str = path.to_string_lossy().to_string();

        if self.assets.contains_key(&path_str) {
            self.set_asset_by_hash(&path_str);
        } else {
            self.set_asset(Arc::new(FileAsset::new(path_str, MatImage::load_from_path(&path)?)));
        }

        self.path = Some(path.clone());

        #[cfg(debug_assertions)]
        let _timer_path = crate::util::timer::ScopedTimer::new("Path scan");

        // Refresh directory listing and select current index
        if let Some(dir) = path.parent() {
            self.file_nav.refresh_dir_listing_for(dir.to_path_buf());
            self.file_nav.select_index_for_path(&path);
            let _ = self.file_nav.start_dir_watcher(dir.to_path_buf());
        } else {
            self.file_nav.clear();
        }
        Ok(())
    }

    pub fn load_from_clipboard(&mut self) -> Result<()> {
        #[cfg(debug_assertions)]
        let _timer = crate::util::timer::ScopedTimer::new("Total image load time [from clipboard]");

        let image = MatImage::load_from_clipboard()
            .or_else(|_| MatImage::load_from_url(arboard::Clipboard::new().unwrap().get_text()?.as_str()))?;
        self.set_asset(Arc::new(ClipboardAsset::new(image)));

        self.path = None;
        self.file_nav.clear();

        Ok(())
    }

    pub fn set_asset_by_hash(&mut self, hash: &str) {
        self.asset = self.assets.get(hash).cloned();
    }

    pub fn set_asset(&mut self, asset: SharedAsset) {
        let hash = asset.hash().to_string();

        self.assets.entry(hash.clone()).or_insert_with(|| asset.clone());

        self.asset = self.assets.get(&hash).cloned();
    }

    pub fn clear_asset(&mut self) {
        self.asset = None;
        self.path = None;
        self.file_nav.clear();
        self.marquee_rect = Recti::ZERO;
    }

    pub fn set_marquee_rect(&mut self, rect: Recti) {
        // check marquee rect exceed image bounds
        if let Some(asset) = &self.asset {
            let spec = asset.image().spec();
            let img_rect = Recti::from_min_size(vec2i(0, 0), vec2i(spec.width, spec.height));
            let r = rect.validate().intersect(img_rect);
            self.marquee_rect = r;
        } else {
            self.marquee_rect = Recti::ZERO;
        }
    }

    pub fn validate_marquee_rect(&mut self) {
        if let Some(asset) = &self.asset {
            let spec = asset.image().spec();
            let img_rect = Recti::from_min_size(vec2i(0, 0), vec2i(spec.width, spec.height));
            self.marquee_rect = self.marquee_rect.validate().intersect(img_rect);
        } else {
            self.marquee_rect = Recti::ZERO;
        }
    }

    pub fn reset_marquee_rect(&mut self) {
        self.marquee_rect = Recti::ZERO;
    }

    pub fn navigate_next(&mut self) -> Result<()> {
        if let Some(asset) = &self.asset {
            match asset.asset_type() {
                AssetType::File => {
                    if let Some(path) = self.file_nav.navigate_next() {
                        self.load_from_path(path)?;
                    }
                }
                _ => return Ok(()),
            }
        }
        Ok(())
    }

    pub fn navigate_prev(&mut self) -> Result<()> {
        if let Some(asset) = &self.asset {
            match asset.asset_type() {
                AssetType::File => {
                    if let Some(path) = self.file_nav.navigate_prev() {
                        self.load_from_path(path)?;
                    }
                }
                _ => return Ok(()),
            }
        }
        Ok(())
    }

    pub fn process_watcher_events(&mut self) {
        self.file_nav.process_watcher_events();
        // Keep current index in sync when list commits
        if let Some(cur_path) = self.path.clone() {
            // Ensures index stays aligned if list changed.
            self.file_nav.select_index_for_path(&cur_path);
        }
    }
}
