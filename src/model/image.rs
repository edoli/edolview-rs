use crate::util::{self, cv_ext::CvIntExt};
use color_eyre::eyre::{eyre, Result};
use half::f16;
use opencv::prelude::*;
use opencv::{core, imgcodecs, imgproc};
use std::{
    fs, mem, path::PathBuf, sync::atomic::{AtomicU64, Ordering}
};

pub unsafe trait DataType: Copy {
    fn typ() -> i32;
}

macro_rules! data_type {
    ($rust_type: ty, $type: expr) => {
        unsafe impl DataType for $rust_type {
            #[inline]
            fn typ() -> i32 {
                $type
            }
        }
    };
}

// int
data_type!(u8, 0);
data_type!(i8, 1);
data_type!(u16, 2);
data_type!(i16, 3);
data_type!(i32, 4);
data_type!(f32, 5);
data_type!(f64, 6);
data_type!(f16, 7);

pub struct ImageSpec {
    pub width: i32,
    pub height: i32,
    pub channels: i32,
    pub dtype: i32, // OpenCV type, e.g. CV_8U, CV_32F
}

// data of ImageSpec should be always f32
impl ImageSpec {
    pub fn new(mat: &opencv::core::Mat, dtype: i32) -> Self {
        Self {
            width: mat.cols(),
            height: mat.rows(),
            channels: mat.channels(),
            dtype,
        }
    }

    pub fn total_bytes(&self) -> usize {
        (self.width as usize) * (self.height as usize) * (self.channels as usize) * mem::size_of::<f32>()
    }

    pub fn bytes_per_pixel(&self) -> usize {
        (self.channels as usize) * mem::size_of::<f32>()
    }
}

pub trait Image {
    fn id(&self) -> u64;
    fn spec(&self) -> &ImageSpec;
    fn data_ptr(&self) -> Result<&[u8]>;
    fn get_pixel_at(&self, x: i32, y: i32) -> Result<&[f32]> {
        let spec = self.spec();
        if x < 0 || x >= spec.width || y < 0 || y >= spec.height {
            return Err(color_eyre::eyre::eyre!("Coordinates out of bounds"));
        }
        let bytes_per_pixel = spec.bytes_per_pixel();
        let row_bytes = (spec.width as usize) * bytes_per_pixel;
        let start = (y as usize) * row_bytes + (x as usize) * bytes_per_pixel;
        let end = start + bytes_per_pixel;
        let data = self.data_ptr()?;
        let (_, f32s, _) = unsafe { data[start..end].align_to::<f32>() };
        Ok(&f32s)
    }
}

// data of MatImage should be always f32. dtype of spec is not dtype of mat, but the original dtype before conversion to f32.
pub struct MatImage {
    id: u64,
    mat: opencv::core::Mat,
    spec: ImageSpec,
}

impl MatImage {
    pub fn new(mat: opencv::core::Mat, dtype: i32) -> Self {
        let spec = ImageSpec::new(&mat, dtype);
        assert!(mat.depth() == f32::typ());
        Self {
            mat,
            spec,
            id: new_id(),
        }
    }
}

impl MatImage {
    pub fn load_from_path(path: &PathBuf) -> Result<MatImage> {
        if !path.exists() {
            return Err(eyre!("Image does not exist: {:?}", path));
        }

        let mat = {
            let _timer = util::timer::ScopedTimer::new("imdecode");
            // Read image using imread fails on paths with non-ASCII characters.
            // imgcodecs::imread(path.to_string_lossy().as_ref(), imgcodecs::IMREAD_UNCHANGED)?

            let data = fs::read(&path).map_err(|e| eyre!("Failed to read file bytes: {e}"))?;
            let data_mat = core::Mat::new_rows_cols_with_data(1, data.len() as i32, &data)?;
            imgcodecs::imdecode(&data_mat, imgcodecs::IMREAD_UNCHANGED)?
        };

        if mat.empty() {
            return Err(eyre!("Failed to load image"));
        }
        
        let mut mat_f32 = core::Mat::default();
        let mut tmp = core::Mat::default();

        let channels = mat.channels();
        let color_convert = match mat.channels() {
            1 => -1,
            3 => imgproc::COLOR_BGR2RGB,
            4 => imgproc::COLOR_BGRA2RGBA,
            _ => {
                return Err(eyre!("Unsupported image channels: {}", channels));
            }
        };

        if color_convert != -1 {
            imgproc::cvt_color(&mat, &mut tmp, color_convert, 0, core::AlgorithmHint::ALGO_HINT_DEFAULT)?;
        } else {
            tmp = mat.clone();
        }

        let dtype = tmp.depth();

        tmp.convert_to(&mut mat_f32, core::CV_32F, 1.0 / dtype.alpha(), 0.0)?;

        Ok(MatImage::new(mat_f32, dtype))
    }
}

impl Image for MatImage {
    fn id(&self) -> u64 {
        self.id
    }

    fn spec(&self) -> &ImageSpec {
        &self.spec
    }

    fn data_ptr(&self) -> Result<&[u8]> {
        Ok(self.mat.data_bytes()?)
    }
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn new_id() -> u64 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}
