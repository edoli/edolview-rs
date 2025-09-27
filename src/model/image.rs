use crate::model::MeanProcessor;
use crate::util::cv_ext::CvIntExt;
use color_eyre::eyre::{eyre, Result};
use half::f16;
use opencv::core::Size;
use opencv::prelude::*;
use opencv::{core, imgcodecs, imgproc};
use std::ffi::c_void;
use std::{
    fs, mem,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    sync::OnceLock,
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

#[derive(Clone)]
pub struct MinMax {
    mins: Vec<f32>,
    maxs: Vec<f32>,

    total_min: OnceLock<f32>,
    total_max: OnceLock<f32>,
}

pub const EMPTY_MINMAX: MinMax = MinMax {
    mins: Vec::new(),
    maxs: Vec::new(),
    total_min: OnceLock::new(),
    total_max: OnceLock::new(),
};

impl MinMax {
    pub fn new(mins: Vec<f32>, maxs: Vec<f32>) -> Self {
        Self {
            mins,
            maxs,
            total_min: OnceLock::new(),
            total_max: OnceLock::new(),
        }
    }

    pub fn min(&self, channel: usize) -> f32 {
        if channel >= self.mins.len() {
            0.0
        } else {
            self.mins[channel]
        }
    }

    pub fn max(&self, channel: usize) -> f32 {
        if channel >= self.maxs.len() {
            0.0
        } else {
            self.maxs[channel]
        }
    }

    pub fn total_min(&self) -> f32 {
        self.total_min
            .get_or_init(|| {
                if self.mins.is_empty() {
                    0.0
                } else {
                    self.mins.iter().copied().fold(f32::INFINITY, f32::min)
                }
            })
            .to_owned()
    }

    pub fn total_max(&self) -> f32 {
        self.total_max
            .get_or_init(|| {
                if self.maxs.is_empty() {
                    0.0
                } else {
                    self.maxs.iter().copied().fold(f32::NEG_INFINITY, f32::max)
                }
            })
            .to_owned()
    }
}

// data of MatImage should be always f32. dtype of spec is not dtype of mat, but the original dtype before conversion to f32.
pub struct MatImage {
    id: u64,
    mat: opencv::core::Mat,
    spec: ImageSpec,

    minmax: OnceLock<MinMax>,

    mean_processor: OnceLock<MeanProcessor>,
}

impl MatImage {
    pub fn new(mat: opencv::core::Mat, dtype: i32) -> Self {
        let spec = ImageSpec::new(&mat, dtype);
        assert!(mat.depth() == f32::typ());
        Self {
            mat,
            spec,
            id: new_id(),
            minmax: OnceLock::new(),
            mean_processor: OnceLock::new(),
        }
    }

    pub fn mean_value_in_rect(&self, rect: opencv::core::Rect) -> Result<Vec<f64>> {
        #[cfg(debug_assertions)]
        let _timer = crate::util::timer::ScopedTimer::new("Compute mean in rect");

        self.mean_processor
            .get_or_init(|| MeanProcessor::new())
            .compute(&self.mat, rect)
    }

    fn compute_minmax(&self) -> MinMax {
        #[cfg(debug_assertions)]
        let _timer = crate::util::timer::ScopedTimer::new("Compute minmax");

        let spec = self.spec();
        if spec.width == 0 || spec.height == 0 || spec.channels <= 0 {
            return MinMax::new(vec![], vec![]);
        }

        let mut mins = vec![f32::INFINITY; spec.channels as usize];
        let mut maxs = vec![f32::NEG_INFINITY; spec.channels as usize];

        let channels = self.mat.channels();
        for ch in 0..channels {
            let mut dst = core::Mat::default();
            core::extract_channel(&self.mat, &mut dst, ch).expect("extract_channel failed");
            let mut min_val = 0f64;
            let mut max_val = 0f64;
            let _ = core::min_max_loc(&dst, Some(&mut min_val), Some(&mut max_val), None, None, &core::no_array());
            mins[ch as usize] = min_val as f32;
            maxs[ch as usize] = max_val as f32;
        }

        MinMax::new(mins, maxs)
    }

    pub fn minmax(&self) -> &MinMax {
        self.minmax.get_or_init(|| self.compute_minmax())
    }

    pub fn reset(&mut self) {
        self.mean_processor.take();
    }
}

impl MatImage {
    pub fn from_bytes_size_type(bytes: &Vec<u8>, size: Size, dtype: i32) -> Result<MatImage> {
        let channels = dtype.cv_type_channels();
        let mat = unsafe {
            core::Mat::new_rows_cols_with_data_unsafe_def(
                size.height,
                size.width,
                dtype,
                bytes.as_ptr().cast::<c_void>().cast_mut(),
            )?
        };
        let mat = mat.reshape(channels, size.height)?.clone_pointee().clone();
        if mat.empty() {
            return Err(eyre!("Failed to load image"));
        }

        let mat_f32 = Self::postprocess(mat, 1.0, false)?;

        Ok(MatImage::new(mat_f32, dtype))
    }

    pub fn from_bytes(bytes: &Vec<u8>) -> Result<MatImage> {
        let bytes_mat = core::Mat::new_rows_cols_with_data(1, bytes.len() as i32, bytes)?;
        let mat = imgcodecs::imdecode(&bytes_mat, imgcodecs::IMREAD_UNCHANGED)?;

        if mat.empty() {
            return Err(eyre!("Failed to load image"));
        }

        let dtype = mat.depth();
        let mat_f32 = Self::postprocess(mat, 1.0, true)?;

        Ok(MatImage::new(mat_f32, dtype))
    }

    pub fn load_from_path(path: &PathBuf) -> Result<MatImage> {
        if !path.exists() {
            return Err(eyre!("Image does not exist: {:?}", path));
        }

        // Determine extension for special handling (e.g., PFM)
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();

        let mut scale_abs = 1.0f64;

        let mat = {
            #[cfg(debug_assertions)]
            let _timer = crate::util::timer::ScopedTimer::new("Image read");
            // Read image using imread fails on paths with non-ASCII characters.
            // imgcodecs::imread(path.to_string_lossy().as_ref(), imgcodecs::IMREAD_UNCHANGED)?

            if ext == "exr" {
                // Copy file to temp file with ASCII path and read it
                let temp_dir = std::env::temp_dir();
                let temp_path = temp_dir.join("edolview_temp.exr");
                {
                    #[cfg(debug_assertions)]
                    let _timer_copy = crate::util::timer::ScopedTimer::new("Image temp copy");

                    fs::copy(&path, &temp_path).map_err(|e| eyre!("Failed to copy file: {e}"))?;
                }
                imgcodecs::imread(temp_path.to_string_lossy().as_ref(), imgcodecs::IMREAD_UNCHANGED)?
            } else {
                let mut bytes = fs::read(&path).map_err(|e| eyre!("Failed to read file bytes: {e}"))?;

                if ext == "pfm" {
                    (bytes, scale_abs) = fix_pfm_header_and_parse_scale(&bytes);
                } else if ext == "flo" {
                    // Optical flow (.flo) file: decode directly to CV_32FC2 Mat
                    return Ok(MatImage::new(decode_flo_to_mat(&bytes)?, core::CV_32F));
                }
                let bytes_mat = core::Mat::new_rows_cols_with_data(1, bytes.len() as i32, &bytes)?;
                imgcodecs::imdecode(&bytes_mat, imgcodecs::IMREAD_UNCHANGED)?
            }
        };

        if mat.empty() {
            return Err(eyre!("Failed to load image"));
        }

        let dtype = mat.depth();
        let mat_f32 = Self::postprocess(mat, scale_abs, true)?;

        Ok(MatImage::new(mat_f32, dtype))
    }

    pub fn load_from_clipboard() -> Result<MatImage> {
        #[cfg(debug_assertions)]
        let _timer = crate::util::timer::ScopedTimer::new("Clipboard read");

        let mut clipboard = arboard::Clipboard::new().map_err(|e| eyre!("Failed to open clipboard: {e}"))?;
        let image_data = clipboard
            .get_image()
            .map_err(|e| eyre!("Failed to get image from clipboard: {e}"))?;

        let width = image_data.width as i32;
        let height = image_data.height as i32;
        let bytes = std::borrow::Cow::into_owned(image_data.bytes);
        let channels = (bytes.len() / (width as usize) / (height as usize)) as i32;
        if width <= 0 || height <= 0 || channels <= 0 || channels > 4 {
            return Err(eyre!("Invalid clipboard image dimensions or channels"));
        }

        let mat = core::Mat::new_rows_cols_with_data(height, width * channels, &bytes)?.clone_pointee();
        let mat = mat.reshape(channels, height)?.clone_pointee().clone();

        if mat.empty() {
            return Err(eyre!("Failed to load image"));
        }

        let dtype = mat.depth();
        let mat_f32 = Self::postprocess(mat, 1.0, false)?;

        Ok(MatImage::new(mat_f32, dtype))
    }

    pub fn load_from_url(url: &str) -> Result<MatImage> {
        #[cfg(debug_assertions)]
        let _timer = crate::util::timer::ScopedTimer::new("Image download");

        let bytes = ureq::get(url).call()?.body_mut().read_to_vec()?;

        let bytes_mat = core::Mat::new_rows_cols_with_data(1, bytes.len() as i32, &bytes)?;
        let mat = imgcodecs::imdecode(&bytes_mat, imgcodecs::IMREAD_UNCHANGED)?;
        if mat.empty() {
            return Err(eyre!("Failed to load image"));
        }

        let dtype = mat.depth();
        let mat_f32 = Self::postprocess(mat, 1.0, true)?;

        Ok(MatImage::new(mat_f32, dtype))
    }

    fn postprocess(mat: core::Mat, scale: f64, bgr_convert: bool) -> Result<core::Mat> {
        #[cfg(debug_assertions)]
        let _timer = crate::util::timer::ScopedTimer::new("Image read postprocess");

        let mut mat_f32 = core::Mat::default();

        let dtype = mat.depth();

        if bgr_convert {
            let channels = mat.channels();
            let mut tmp = core::Mat::default();

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
                tmp.convert_to(&mut mat_f32, core::CV_32F, scale / dtype.alpha(), 0.0)?;
            } else {
                mat.convert_to(&mut mat_f32, core::CV_32F, scale / dtype.alpha(), 0.0)?;
            }
        } else {
            mat.convert_to(&mut mat_f32, core::CV_32F, scale / dtype.alpha(), 0.0)?;
        }

        Ok(mat_f32)
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

// Decode Middlebury .flo optical flow file into OpenCV Mat (CV_32FC2)
// Format (little-endian):
// - magic: f32 (202021.25)
// - width: i32
// - height: i32
// - data: width * height * 2 f32 (u, v) in row-major, interleaved
fn decode_flo_to_mat(bytes: &[u8]) -> Result<core::Mat> {
    // Need at least 12 bytes for header
    if bytes.len() < 12 {
        return Err(eyre!(".flo: file too small: {} bytes", bytes.len()));
    }

    let magic = f32::from_le_bytes(bytes[0..4].try_into().unwrap());
    if magic != 202021.25f32 {
        return Err(eyre!(".flo: invalid magic: {}", magic));
    }

    let width = i32::from_le_bytes(bytes[4..8].try_into().unwrap());
    let height = i32::from_le_bytes(bytes[8..12].try_into().unwrap());
    if width <= 0 || height <= 0 {
        return Err(eyre!(".flo: invalid dimensions: {}x{}", width, height));
    }

    let num_pixels = (width as usize)
        .checked_mul(height as usize)
        .ok_or_else(|| eyre!(".flo: image size overflow"))?;
    let num_floats = num_pixels.checked_mul(2).ok_or_else(|| eyre!(".flo: channels overflow"))?;
    let data_bytes = num_floats.checked_mul(4).ok_or_else(|| eyre!(".flo: data size overflow"))?;

    if bytes.len() < 12 + data_bytes {
        return Err(eyre!(".flo: not enough data: have {}, need {}", bytes.len() - 12, data_bytes));
    }

    // Allocate CV_32FC2 Mat
    let mut mat = unsafe { core::Mat::new_rows_cols(height, width, core::CV_32FC2)? };

    // Fill data with proper endianness handling without assuming single-channel typed access
    #[cfg(target_endian = "little")]
    {
        // Direct byte copy on little-endian
        let src = &bytes[12..12 + data_bytes];
        let dst_bytes = mat.data_bytes_mut()?;
        debug_assert!(dst_bytes.len() >= data_bytes);
        dst_bytes[..data_bytes].copy_from_slice(src);
    }

    #[cfg(target_endian = "big")]
    {
        // Convert LE bytes to native f32 values
        let dst_bytes = mat.data_bytes_mut()?;
        let (_, dst_f32, _) = dst_bytes.align_to_mut::<f32>();
        debug_assert_eq!(dst_f32.len(), num_floats);
        let mut off = 12usize;
        for v in dst_f32.iter_mut() {
            let f = f32::from_le_bytes(bytes[off..off + 4].try_into().unwrap());
            *v = f;
            off += 4;
        }
    }

    Ok(mat)
}
