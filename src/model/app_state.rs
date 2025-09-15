use std::{fs, path::PathBuf};

use color_eyre::eyre::{eyre, Result};
use opencv::{core, imgcodecs, imgproc, prelude::*};

use crate::{model::MatImage, ui::gl::ShaderParams, util};

pub struct AppState {
    pub path: Option<PathBuf>,
    pub display: Option<MatImage>,
    pub shader_params: ShaderParams,
}

impl AppState {
    pub fn empty() -> Self {
        Self {
            path: None,
            display: None,
            shader_params: ShaderParams::default(),
        }
    }

    pub fn load_from_path(&mut self, path: PathBuf) -> Result<()> {
        if !path.exists() {
            return Err(eyre!("Image does not exist: {:?}", path));
        }

        let img = {
            let _timer = util::timer::ScopedTimer::new("imdecode");
            // Read image using imread fails on paths with non-ASCII characters.
            // imgcodecs::imread(path.to_string_lossy().as_ref(), imgcodecs::IMREAD_UNCHANGED)?

            let data = fs::read(&path).map_err(|e| eyre!("Failed to read file bytes: {e}"))?;
            let data_mat = core::Mat::new_rows_cols_with_data(1, data.len() as i32, &data)?;
            imgcodecs::imdecode(&data_mat, imgcodecs::IMREAD_UNCHANGED)?
        };

        if img.empty() {
            return Err(eyre!("Failed to load image"));
        }
        self.path = Some(path);
        self.display = Some(self.process(&img)?);
        Ok(())
    }

    fn process(&self, mat: &core::Mat) -> Result<MatImage> {
        let mut mat_f32 = core::Mat::default();
        {
            let mut tmp = core::Mat::default();

            let channels = mat.channels();
            let color_convert = match mat.channels() {
                1 => imgproc::COLOR_BGR2RGB,
                3 => imgproc::COLOR_BGR2RGB,
                4 => imgproc::COLOR_BGRA2RGBA,
                _ => {
                    return Err(eyre!("Unsupported image channels: {}", channels));
                }
            };

            imgproc::cvt_color(&mat, &mut tmp, color_convert, 0, core::AlgorithmHint::ALGO_HINT_DEFAULT)?;

            let dtype = tmp.depth();
            match dtype {
                core::CV_8U => {
                    tmp.convert_to(&mut mat_f32, core::CV_32F, 1.0 / 255.0, 0.0)?;
                }
                core::CV_8S => {
                    tmp.convert_to(&mut mat_f32, core::CV_32F, 1.0 / 127.0, 0.0)?;
                }
                core::CV_16U => {
                    tmp.convert_to(&mut mat_f32, core::CV_32F, 1.0 / 65535.0, 0.0)?;
                }
                core::CV_16S => {
                    tmp.convert_to(&mut mat_f32, core::CV_32F, 1.0 / 32767.0, 0.0)?;
                }
                core::CV_32S => {
                    tmp.convert_to(&mut mat_f32, core::CV_32F, 1.0 / 2147483647.0, 0.0)?;
                }
                core::CV_32F => {
                    mat_f32 = tmp;
                }
                core::CV_64F => {
                    tmp.convert_to(&mut mat_f32, core::CV_32F, 1.0, 0.0)?;
                }
                _ => {
                    return Err(eyre!("Unsupported image depth: {}", dtype));
                }
            }
        }
        Ok(MatImage::new(mat_f32))
    }
}
