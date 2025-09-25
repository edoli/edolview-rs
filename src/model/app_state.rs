use std::path::PathBuf;

use color_eyre::eyre::Result;

use crate::{
    model::{Asset, FileAsset, Image, MatImage, Recti},
    ui::gl::ShaderParams,
    util::math_ext::{vec2i, Vec2i},
};

pub struct AppState {
    pub path: Option<PathBuf>,
    pub asset: Option<Box<dyn Asset<MatImage>>>,
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
        }
    }

    pub fn load_from_path(&mut self, path: PathBuf) -> Result<()> {
        #[cfg(debug_assertions)]
        let _timer1 = crate::util::timer::ScopedTimer::new("Total image load time");

        self.asset = Some(Box::new(FileAsset::new(
            path.to_string_lossy().to_string(),
            MatImage::load_from_path(&path)?,
        )));
        self.path = Some(path.clone());

        #[cfg(debug_assertions)]
        let _timer2 = crate::util::timer::ScopedTimer::new("Path scan");

        // Refresh directory listing and select current index
        if let Some(dir) = path.parent() {
            self.file_nav.refresh_dir_listing_for(dir.to_path_buf());
            self.file_nav.select_index_for_path(&path);
            let _ = self.file_nav.start_dir_watcher(dir.to_path_buf());
        } else {
            self.file_nav.stop_dir_watcher();
            self.file_nav.dir_path = None;
            self.file_nav.files_in_dir.clear();
            self.file_nav.current_file_index = None;
        }
        Ok(())
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
        if let Some(path) = self.file_nav.navigate_next() {
            self.load_from_path(path)?;
        }
        Ok(())
    }

    pub fn navigate_prev(&mut self) -> Result<()> {
        if let Some(path) = self.file_nav.navigate_prev() {
            self.load_from_path(path)?;
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
