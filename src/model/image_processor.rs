use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, OnceLock,
    },
    thread,
};

use color_eyre::eyre::{eyre, Result};
use opencv::{
    core::{self, MatTraitConst, MatTraitConstManual, ModifyInplace},
    imgproc,
};

use crate::{
    model::{Image, MatImage},
    util::cv_ext::{cv_make_type, MatExt},
};

#[derive(PartialEq, Clone)]
pub enum MeanDim {
    All,
    Column,
    Row,
}

pub struct MeanProcessor {
    // Cached integral image for the current MatImage (built asynchronously).
    integral_image: Arc<Mutex<OnceLock<core::Mat>>>,
    // Set once precompute starts; used to avoid repeated spawns.
    is_precompute_begin: AtomicBool,

    // Tracks which image the cache belongs to.
    last_image_id: u64,
}

impl MeanProcessor {
    pub fn new() -> Self {
        Self {
            integral_image: Arc::new(Mutex::new(OnceLock::new())),
            is_precompute_begin: AtomicBool::new(false),
            last_image_id: u64::MAX,
        }
    }

    // Build integral image for fast mean queries.
    // Note: cost is paid once per image; results are reused by fast_compute().
    fn precompute(mat: &core::Mat) -> Result<core::Mat> {
        #[cfg(debug_assertions)]
        let _timer = crate::util::timer::ScopedTimer::new("MeanProcessor::precompute");

        let mut integral_image = core::Mat::default();
        imgproc::integral(mat, &mut integral_image, core::CV_64F)?;
        Ok(integral_image)
    }

    // Fast mean using precomputed integral image.
    // Returns error if the integral image is not ready.
    fn fast_compute(&self, mat: &core::Mat, rect: core::Rect, dim: MeanDim) -> Result<Vec<f64>> {
        #[cfg(debug_assertions)]
        let _timer = crate::util::timer::ScopedTimer::new("MeanProcessor::fast_compute");

        let channels = mat.channels() as usize;
        let width = rect.width as usize;
        let height = rect.height as usize;

        let integral_image_lock = self.integral_image.lock().unwrap();

        let integral_image_mat = integral_image_lock
            .get()
            .ok_or_else(|| eyre!("Integral image not computed yet"))?;

        let step = integral_image_mat.cols() as usize;
        let x = rect.x as usize;
        let y = rect.y as usize;

        match dim {
            MeanDim::All => {
                let bytes = integral_image_mat.data_bytes()?;
                let (head, f32s, tail) = unsafe { bytes.align_to::<f64>() };
                if !head.is_empty() || !tail.is_empty() {
                    return Err(eyre!("Integral image data is not aligned to f32"));
                }

                let tl_idx = (y * step + x) * channels;
                let tr_idx = (y * step + (x + width)) * channels;
                let bl_idx = ((y + height) * step + x) * channels;
                let br_idx = ((y + height) * step + (x + width)) * channels;

                let mut means = vec![0f64; channels];
                for c in 0..channels {
                    let sum = f32s[br_idx + c] - f32s[bl_idx + c] - f32s[tr_idx + c] + f32s[tl_idx + c];
                    means[c] = sum / (width * height) as f64;
                }
                Ok(means)
            }
            MeanDim::Column => {
                let x = x as i32;
                let y = y as i32;
                let width = width as i32;
                let height = height as i32;

                let top_row = integral_image_mat.row(y)?;
                let bot_row = integral_image_mat.row(y + height)?;

                let r_right = core::Range::new(x + 1, x + width + 1)?;
                let r_left = core::Range::new(x, x + width)?;

                let top_right = top_row.col_range(&r_right)?;
                let bot_right = bot_row.col_range(&r_right)?;
                let top_left = top_row.col_range(&r_left)?;
                let bot_left = bot_row.col_range(&r_left)?;

                let no_mask = core::no_array();

                let mut d_right = core::Mat::default();
                core::subtract(&bot_right, &top_right, &mut d_right, &no_mask, -1)?;

                let mut d_left = core::Mat::default();
                core::subtract(&bot_left, &top_left, &mut d_left, &no_mask, -1)?;

                let mut col_sums = core::Mat::default();
                core::subtract(&d_right, &d_left, &mut col_sums, &no_mask, -1)?;

                unsafe {
                    let f = 1.0 / (height as f64);
                    col_sums.modify_inplace(|i, o| core::multiply_def(i, &core::Scalar::new(f, f, f, f), o))?;
                }

                Ok(col_sums.reshape(1, 0)?.data_typed::<f64>()?.to_vec())
            }
            MeanDim::Row => {
                let x = x as i32;
                let y = y as i32;
                let width = width as i32;
                let height = height as i32;

                let left_col = integral_image_mat.col(x)?;
                let right_col = integral_image_mat.col(x + width)?;

                let r_bottom = core::Range::new(y + 1, y + height + 1)?;
                let r_top = core::Range::new(y, y + height)?;

                let top_right = left_col.row_range(&r_bottom)?;
                let bot_right = right_col.row_range(&r_bottom)?;
                let top_left = left_col.row_range(&r_top)?;
                let bot_left = right_col.row_range(&r_top)?;

                let no_mask = core::no_array();

                let mut d_bot = core::Mat::default();
                core::subtract(&bot_right, &top_right, &mut d_bot, &no_mask, -1)?;

                let mut d_top = core::Mat::default();
                core::subtract(&bot_left, &top_left, &mut d_top, &no_mask, -1)?;

                let mut row_sums = core::Mat::default();
                core::subtract(&d_bot, &d_top, &mut row_sums, &no_mask, -1)?;

                unsafe {
                    let f = 1.0 / (width as f64);
                    row_sums.modify_inplace(|i, o| core::multiply_def(i, &core::Scalar::new(f, f, f, f), o))?;
                }

                Ok(row_sums.reshape(1, 0)?.data_typed::<f64>()?.to_vec())
            }
        }
    }

    // Slow path for first frame or if integral image is unavailable.
    // Uses OpenCV reduce/mean on the ROI.
    fn fallback_compute(mat: &core::Mat, rect: core::Rect, dim: MeanDim) -> Result<Vec<f64>> {
        #[cfg(debug_assertions)]
        let _timer = crate::util::timer::ScopedTimer::new("MeanProcessor::fallback_compute");

        let size = mat.size().unwrap();
        let channels = mat.channels();
        if rect.x < 0 || rect.y < 0 || rect.x + rect.width > size.width || rect.y + rect.height > size.height {
            return Err(eyre!("Rect out of bounds"));
        }
        let roi = core::Mat::roi(mat, rect)?;

        match dim {
            MeanDim::All => {
                let mean = core::mean(&roi, &core::no_array())?;
                Ok(mean[..channels as usize].to_vec())
            }
            MeanDim::Column => {
                let mut dst = core::Mat::default();
                core::reduce(&roi, &mut dst, 0, core::REDUCE_AVG, cv_make_type(core::CV_64F, channels))?;
                Ok(dst.reshape(1, 0)?.data_typed::<f64>()?.to_vec())
            }
            MeanDim::Row => {
                let mut dst = core::Mat::default();
                core::reduce(&roi, &mut dst, 1, core::REDUCE_AVG, cv_make_type(core::CV_64F, channels))?;
                Ok(dst.reshape(1, 0)?.data_typed::<f64>()?.to_vec())
            }
        }
    }

    // Compute mean, preferring fast path when precompute is ready.
    // If precompute has not started, spawn it and return fallback result.
    fn compute_mat(&self, mat: &core::Mat, rect: core::Rect, dim: MeanDim) -> Result<Vec<f64>> {
        let width = rect.width;
        let height = rect.height;

        if width <= 0 || height <= 0 {
            return Ok(vec![]);
        }

        if self.is_precompute_begin.load(Ordering::Relaxed) {
            if self.integral_image.lock().unwrap().get().is_some() {
                self.fast_compute(mat, rect, dim)
            } else {
                Self::fallback_compute(mat, rect, dim)
            }
        } else {
            self.is_precompute_begin.store(true, Ordering::Relaxed);
            let mat_clone = mat.shallow_clone()?;
            let slot = Arc::clone(&self.integral_image);
            thread::spawn(move || {
                if let Ok(ii) = Self::precompute(&mat_clone) {
                    let _ = slot.lock().unwrap().set(ii);
                }
            });
            Self::fallback_compute(mat, rect, dim)
        }
    }

    // Public entry point for computing means. Resets cache when image changes.
    // Note: first call may still hit the fallback path.
    pub fn compute(&mut self, image: &MatImage, rect: core::Rect, dim: MeanDim) -> Result<Vec<f64>> {
        let image_id = image.id();
        let last_image_id = self.last_image_id;
        if image_id != last_image_id {
            self.last_image_id = image_id;

            if last_image_id != u64::MAX {
                self.is_precompute_begin.store(false, Ordering::Relaxed);
                let _ = self.integral_image.lock().unwrap().take();
            }
        }
        self.compute_mat(&image.mat(), rect, dim)
    }

    // Kick off integral image computation without blocking.
    // Useful to hide the first-frame cost before any marquee interaction.
    // Thread-safety: concurrent callers may race but will converge on one cache.
    pub fn precompute_async(&mut self, image: &MatImage) {
        let image_id = image.id();
        if image_id != self.last_image_id {
            self.last_image_id = image_id;
            self.is_precompute_begin.store(false, Ordering::Relaxed);
            let _ = self.integral_image.lock().unwrap().take();
        }

        if self.is_precompute_begin.load(Ordering::Relaxed) {
            return;
        }

        self.is_precompute_begin.store(true, Ordering::Relaxed);
        let mat_clone = match image.mat().shallow_clone() {
            Ok(mat) => mat,
            Err(_) => return,
        };
        let slot = Arc::clone(&self.integral_image);
        thread::spawn(move || {
            if let Ok(ii) = Self::precompute(&mat_clone) {
                let _ = slot.lock().unwrap().set(ii);
            }
        });
    }
}
