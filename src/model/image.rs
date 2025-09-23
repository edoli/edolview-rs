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

#[derive(Clone)]
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
    fn spec(&self) -> ImageSpec;
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

    pub fn mean_value_in_rect(&self, rect: opencv::core::Rect) -> Result<Vec<f64>> {
        let spec = self.spec();
        if rect.x < 0 || rect.y < 0 || rect.x + rect.width > spec.width || rect.y + rect.height > spec.height {
            return Err(eyre!("Rect out of bounds"));
        }
        let roi = core::Mat::roi(&self.mat, rect)?;
        let mean = core::mean(&roi, &core::no_array())?;
        let mut mean_vec = Vec::with_capacity(spec.channels as usize);
        for i in 0..spec.channels as usize {
            mean_vec.push(mean[i]);
        }
        Ok(mean_vec)
    }
}

impl MatImage {
    pub fn load_from_path(path: &PathBuf) -> Result<MatImage> {
        if !path.exists() {
            return Err(eyre!("Image does not exist: {:?}", path));
        }

        // Determine extension for special handling (e.g., PFM)
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        
        let mut scale_abs = 1.0f64;

        let mat = {
            let _timer = util::timer::ScopedTimer::new("imdecode");
            // Read image using imread fails on paths with non-ASCII characters.
            // imgcodecs::imread(path.to_string_lossy().as_ref(), imgcodecs::IMREAD_UNCHANGED)?

            let mut data = fs::read(&path).map_err(|e| eyre!("Failed to read file bytes: {e}"))?;

            if ext == "pfm" {
                (data, scale_abs) = fix_pfm_header_and_parse_scale(&data);
            }
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

        tmp.convert_to(&mut mat_f32, core::CV_32F, scale_abs / dtype.alpha(), 0.0)?;

        Ok(MatImage::new(mat_f32, dtype))
    }
}

impl Image for MatImage {
    fn id(&self) -> u64 {
        self.id
    }

    fn spec(&self) -> ImageSpec {
        self.spec.clone()
    }

    fn data_ptr(&self) -> Result<&[u8]> {
        Ok(self.mat.data_bytes()?)
    }
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn new_id() -> u64 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

// Fix PFM header quirks for OpenCV and parse scale (3rd line)
// - Trim a single trailing space just before the newline for the first three lines
// - Return fixed bytes and |scale| value parsed from the 3rd line (defaults to 1.0)
fn fix_pfm_header_and_parse_scale(input: &[u8]) -> (Vec<u8>, f64) {
    let mut out: Vec<u8> = Vec::with_capacity(input.len());
    let mut i = 0usize;
    let mut nl = 0u8;

    while i < input.len() && nl < 3 {
        let b = input[i];
        if b == b'\n' {
            if !out.is_empty() && *out.last().unwrap() == b' ' {
                // Replace trailing space with newline, do not push current newline
                let last = out.len() - 1;
                out[last] = b'\n';
            } else {
                out.push(b'\n');
            }
            nl += 1;
        } else {
            out.push(b);
        }
        i += 1;
    }

    // Parse absolute scale from the third line of the (possibly fixed) header
    let mut scale_abs = 1.0f64;
    if let Ok(header_str) = std::str::from_utf8(&out) {
        let mut lines = header_str.lines();
        let _magic = lines.next();
        let _dim = lines.next();
        if let Some(scale_line) = lines.next() {
            if let Ok(v) = scale_line.trim().parse::<f64>() {
                scale_abs = v.abs();
            }
        }
    }

    // Append rest of the data unchanged
    if i < input.len() {
        out.extend_from_slice(&input[i..]);
    }

    (out, scale_abs)
}
