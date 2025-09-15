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
}

impl AppState {
    pub fn empty() -> Self {
        Self {
            path: None,
            display: None,
            shader_params: ShaderParams::default(),
            cursor_pos: None,
            marquee_rect: None,
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
                let img_rect = Rect::from_min_size(
                    pos2(0.0, 0.0),
                    vec2(spec.width as f32, spec.height as f32),
                );
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
