use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, OnceLock,
    },
    thread,
};

use color_eyre::eyre::{eyre, Result};
use opencv::{
    core::{self, MatTraitConst, MatTraitConstManual},
    imgproc,
};

use crate::model::{Image, MatImage};

pub struct MeanProcessor {
    integral_image: Arc<Mutex<OnceLock<core::Mat>>>,
    is_precompute_begin: AtomicBool,

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

    fn precompute(mat: &core::Mat) -> Result<core::Mat> {
        #[cfg(debug_assertions)]
        let _timer = crate::util::timer::ScopedTimer::new("MeanProcessor::precompute");

        let mut integral_image = core::Mat::default();
        imgproc::integral(mat, &mut integral_image, core::CV_64F)?;
        Ok(integral_image)
    }

    fn fast_compute(&self, mat: &core::Mat, rect: core::Rect) -> Result<Vec<f64>> {
        #[cfg(debug_assertions)]
        let _timer = crate::util::timer::ScopedTimer::new("MeanProcessor::fast_compute");

        let channels = mat.channels() as usize;
        let width = rect.width as usize;
        let height = rect.height as usize;

        if width <= 0 || height <= 0 {
            return Ok(vec![0.0; channels as usize]);
        }

        let integral_image_lock = self.integral_image.lock().unwrap();

        let integral_image_mat = integral_image_lock
            .get()
            .ok_or_else(|| eyre!("Integral image not computed yet"))?;

        let step = integral_image_mat.cols() as usize;
        let x = rect.x as usize;
        let y = rect.y as usize;

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

    fn fallback_compute(mat: &core::Mat, rect: core::Rect) -> Result<Vec<f64>> {
        #[cfg(debug_assertions)]
        let _timer = crate::util::timer::ScopedTimer::new("MeanProcessor::fallback_compute");

        let size = mat.size().unwrap();
        let channels = mat.channels();
        if rect.x < 0 || rect.y < 0 || rect.x + rect.width > size.width || rect.y + rect.height > size.height {
            return Err(eyre!("Rect out of bounds"));
        }
        let roi = core::Mat::roi(mat, rect)?;
        let mean = core::mean(&roi, &core::no_array())?;
        Ok(mean[..channels as usize].to_vec())
    }

    fn compute_mat(&self, mat: &core::Mat, rect: core::Rect) -> Result<Vec<f64>> {
        if self.is_precompute_begin.load(Ordering::Relaxed) {
            if self.integral_image.lock().unwrap().get().is_some() {
                self.fast_compute(mat, rect)
            } else {
                Self::fallback_compute(mat, rect)
            }
        } else {
            self.is_precompute_begin.store(true, Ordering::Relaxed);
            let mat_clone = mat.clone();
            let slot = Arc::clone(&self.integral_image);
            thread::spawn(move || {
                if let Ok(ii) = Self::precompute(&mat_clone) {
                    let _ = slot.lock().unwrap().set(ii);
                }
            });
            Self::fallback_compute(mat, rect)
        }
    }

    pub fn compute(&mut self, image: &MatImage, rect: core::Rect) -> Result<Vec<f64>> {
        let image_id = image.id();
        if image_id != self.last_image_id {
            self.last_image_id = image_id;
            self.is_precompute_begin.store(false, Ordering::Relaxed);
            let _ = self.integral_image.lock().unwrap().take();
        }
        self.compute_mat(&image.mat(), rect)
    }
}
