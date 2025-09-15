use std::path::PathBuf;

use color_eyre::eyre::Result;

use crate::{
    model::MatImage,
    ui::gl::ShaderParams,
    util::math_ext::Vec2i,
};

pub struct AppState {
    pub path: Option<PathBuf>,
    pub display: Option<MatImage>,
    pub shader_params: ShaderParams,
    pub cursor_pos: Option<Vec2i>,
}

impl AppState {
    pub fn empty() -> Self {
        Self {
            path: None,
            display: None,
            shader_params: ShaderParams::default(),
            cursor_pos: None,
        }
    }

    pub fn load_from_path(&mut self, path: PathBuf) -> Result<()> {
        self.display = Some(MatImage::load_from_path(&path)?);
        self.path = Some(path);
        Ok(())
    }
}
