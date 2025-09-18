use std::path::PathBuf;

use color_eyre::eyre::Result;
use eframe::egui::{pos2, vec2, Rect};

use crate::{
    model::{Image, MatImage},
    ui::gl::ShaderParams,
    util::math_ext::Vec2i,
};

pub struct AppState {
    pub path: Option<PathBuf>,
    pub display: Option<MatImage>,
    pub shader_params: ShaderParams,
    pub cursor_pos: Option<Vec2i>,
    pub marquee_rect: Option<Rect>,

    pub channel_index: i32,
    pub colormap_rgb: String,
    pub colormap_mono: String,
    pub colormap_rgb_list: Vec<String>,
    pub colormap_mono_list: Vec<String>,

    pub display_min_value: f32,
    pub display_max_value: f32,
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
            display: None,
            shader_params: ShaderParams::default(),
            cursor_pos: None,
            marquee_rect: None,
            channel_index: -1,
            colormap_rgb: String::from("rgb"),
            colormap_mono: String::from("gray"),
            colormap_rgb_list: list_colormaps(&PathBuf::from("assets/colormap/rgb")),
            colormap_mono_list: list_colormaps(&PathBuf::from("assets/colormap/mono")),
            display_min_value: 0.0,
            display_max_value: 1.0,
        }
    }

    pub fn load_from_path(&mut self, path: PathBuf) -> Result<()> {
        self.display = Some(MatImage::load_from_path(&path)?);
        self.path = Some(path);
        Ok(())
    }

    pub fn set_marquee_rect(&mut self, rect: Option<Rect>) {
        // check marquee rect exceed image bounds
        if let Some(d) = &self.display {
            if let Some(r) = rect {
                let spec = d.spec();
                let img_rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(spec.width as f32, spec.height as f32));
                let r = r.intersect(img_rect);
                self.marquee_rect = Some(r);
                return;
            } else {
                self.marquee_rect = None;
                return;
            }
        } else {
            self.marquee_rect = None;
        }
    }
}
