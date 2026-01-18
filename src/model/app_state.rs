use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use clipboard_rs::{Clipboard, ClipboardContext, ContentFormat};
use color_eyre::eyre::{eyre, Result};
use indexmap::IndexMap;

use crate::{
    model::{
        AssetType, ClipboardAsset, ComparisonAsset, ComparisonMode, FileAsset, Image, MatImage, Recti, SharedAsset,
        SocketInfo, SocketState, Statistics,
    },
    ui::gl::ShaderParams,
    util::math_ext::{vec2i, Vec2i},
};

pub struct AppState {
    pub path: Option<PathBuf>,
    pub asset: Option<SharedAsset>,
    pub asset_primary: Option<SharedAsset>,
    pub asset_secondary: Option<SharedAsset>,
    pub comparison_mode: ComparisonMode,
    pub comparison_blend: f32,
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

    // Copy behavior: when true, Ctrl+C copies marquee at original pixel size regardless of zoom.
    pub copy_use_original_size: bool,

    // File navigation + watcher
    pub file_nav: crate::model::FileNav,

    pub statistics: Statistics,

    pub socket_state: Arc<SocketState>,
    pub socket_info: Arc<Mutex<SocketInfo>>,

    pub assets: IndexMap<String, SharedAsset>,
}

fn list_colormaps(rel_dir: &str) -> Vec<String> {
    // get file names ending with .glsl in the directory; silently ignore IO errors
    let base_dir = crate::util::path_ext::exe_dir_or_cwd();
    let dir = base_dir.join(rel_dir);
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
            asset_primary: None,
            asset_secondary: None,
            comparison_mode: ComparisonMode::Diff,
            comparison_blend: 0.5,
            shader_params: ShaderParams::default(),
            cursor_pos: None,
            marquee_rect: Recti::ZERO,
            channel_index: -1,
            colormap_rgb: String::from("rgb"),
            colormap_mono: String::from("gray"),
            colormap_rgb_list: list_colormaps("colormap/rgb"),
            colormap_mono_list: list_colormaps("colormap/mono"),
            is_show_background: true,
            is_show_pixel_tooltip: true,
            is_show_pixel_value: true,
            is_show_crosshair: false,
            is_show_sidebar: true,
            is_show_statusbar: true,
            copy_use_original_size: true,
            file_nav: crate::model::FileNav::new(),
            statistics: Statistics::default(),
            socket_state: Arc::new(SocketState::new()),
            socket_info: Arc::new(Mutex::new(SocketInfo::new())),
            assets: IndexMap::new(),
        }
    }

    pub fn load_from_path(&mut self, path: PathBuf) -> Result<()> {
        #[cfg(debug_assertions)]
        let _timer = crate::util::timer::ScopedTimer::new("Total image load time [from path]");

        let path_str = path.to_string_lossy().to_string();
        let hash_str = FileAsset::hash_from_path(&path)?;

        if self.assets.contains_key(&hash_str) {
            self.set_asset_primary_by_hash(&hash_str);
        } else {
            self.set_primary_asset(Arc::new(FileAsset::new(path_str, hash_str, MatImage::load_from_path(&path)?)));
        }

        self.path = Some(path.clone());

        #[cfg(debug_assertions)]
        let _timer_path = crate::util::timer::ScopedTimer::new("Path scan");

        // Refresh directory listing and select current index
        if let Some(dir) = path.parent() {
            if self.file_nav.check_is_current_dir(dir) {
                // Same directory, no need to refresh
                return Ok(());
            }
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
            .or_else(|_| MatImage::load_from_url(arboard::Clipboard::new().unwrap().get_text()?.as_str()));

        if let Err(_) = &image {
            let ctx = ClipboardContext::new().unwrap();

            if !ctx.has(ContentFormat::Files) {
                return Err(eyre!("Clipboard does not contain files"));
            }
            let uris = ctx.get_files().unwrap();
            if uris.is_empty() {
                return Err(eyre!("Clipboard does not contain image or file data"));
            }

            let paths: Vec<PathBuf> = uris.iter().map(|u| PathBuf::from(u)).collect();

            for path in &paths {
                if path.exists() && path.is_file() {
                    let image = MatImage::load_from_path(path)?;
                    let path_str = path.to_string_lossy().to_string();
                    let hash_str = FileAsset::hash_from_path(path)?;
                    self.set_primary_asset(Arc::new(FileAsset::new(path_str, hash_str, image)));
                }
            }
        } else {
            self.set_primary_asset(Arc::new(ClipboardAsset::new(image?)));
        }

        self.path = None;
        self.file_nav.clear();

        Ok(())
    }

    pub fn set_asset_primary_by_hash(&mut self, hash: &str) {
        self.asset_primary = self.assets.get(hash).cloned();

        self.update_asset();
        self.validate_marquee_rect();
    }

    pub fn set_primary_asset(&mut self, asset: SharedAsset) {
        let hash = asset.hash().to_string();

        self.assets.insert(hash.clone(), asset.clone());

        self.asset_primary = self.assets.get(&hash).cloned();

        self.update_asset();
        self.validate_marquee_rect();
    }

    pub fn set_asset_secondary_by_hash(&mut self, hash: &str) {
        self.asset_secondary = self.assets.get(hash).cloned();

        self.update_asset();
        self.validate_marquee_rect();
    }

    pub fn set_secondary_asset(&mut self, asset: Option<SharedAsset>) {
        if let Some(asset) = asset {
            let hash = asset.hash().to_string();

            self.assets.insert(hash.clone(), asset.clone());

            self.asset_secondary = self.assets.get(&hash).cloned();
        } else {
            self.asset_secondary = None;
        }

        self.update_asset();
        self.validate_marquee_rect();
    }

    pub fn is_comparison(&self) -> bool {
        if let Some(asset) = &self.asset {
            asset.asset_type() == AssetType::Comparison
        } else {
            false
        }
    }

    pub fn update_asset(&mut self) {
        if let Some(asset_primary) = &self.asset_primary {
            if let Some(asset_secondary) = &self.asset_secondary {
                if asset_primary.hash() == asset_secondary.hash() {
                    self.asset = Some(asset_primary.clone());
                } else {
                    // Different assets, create a comparison asset
                    let comp_asset = ComparisonAsset::new(
                        asset_primary.clone(),
                        asset_secondary.clone(),
                        self.comparison_mode,
                        self.comparison_blend,
                    );
                    self.asset = Some(Arc::new(comp_asset));
                }
            } else {
                self.asset = Some(asset_primary.clone());
            }

            let num_channels = asset_primary.image().spec().channels as i32;

            if num_channels == 1 {
                self.channel_index = -1;
            } else if self.channel_index >= num_channels {
                self.channel_index = num_channels - 1;
            }

            if let Some(asset) = &self.asset {
                crate::model::image::MEAN_PROCESSOR
                    .lock()
                    .unwrap()
                    .precompute_async(asset.image());
            }
        } else {
            self.asset = None;
        }
    }

    pub fn clear_asset(&mut self) {
        self.asset = None;
        self.asset_primary = None;
        self.asset_secondary = None;
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
