use crate::model::{gpu_compute, GpuImageTexture, MeanDim, MeanProcessor, Recti};
use crate::util::cv_ext::CvIntExt;
use color_eyre::eyre::{eyre, Result};
use half::f16;
use opencv::core::Size;
use opencv::prelude::*;
use opencv::{core, imgcodecs};
use std::f64;
use std::sync::{Arc, LazyLock, Mutex};
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
    pub fn new(width: i32, height: i32, channels: i32, dtype: i32) -> Self {
        Self {
            width,
            height,
            channels,
            dtype,
        }
    }

    pub fn total_bytes(&self) -> usize {
        (self.width as usize) * (self.height as usize) * (self.channels as usize) * mem::size_of::<f32>()
    }

    pub fn bytes_per_pixel(&self) -> usize {
        (self.channels as usize) * mem::size_of::<f32>()
    }

    pub fn pixel_values_to_string<T: Into<f64> + Copy>(&self, vals: &[T]) -> String {
        let alpha = self.dtype.alpha();
        let is_float = self.dtype.cv_type_is_floating();
        let mut parts: Vec<String> = Vec::with_capacity(vals.len());
        for &v in vals.iter() {
            let vf: f64 = v.into();
            if is_float {
                parts.push(format!("{:.4}", vf * alpha));
            } else {
                parts.push(format!("{:.0}", vf * alpha));
            }
        }
        parts.join(", ")
    }
}

pub trait Image {
    fn id(&self) -> u64;
    fn spec(&self) -> ImageSpec;
    fn data(&self) -> Option<&[f32]>;
    fn gpu_texture(&self) -> Result<Arc<GpuImageTexture>>;
    fn get_pixel_at(&self, x: i32, y: i32) -> Result<PixelValues<'_>> {
        let spec = self.spec();
        if x < 0 || x >= spec.width || y < 0 || y >= spec.height {
            return Err(color_eyre::eyre::eyre!("Coordinates out of bounds"));
        }
        let bytes_per_pixel = spec.bytes_per_pixel();
        let row_bytes = (spec.width as usize) * bytes_per_pixel;
        let start = (y as usize) * row_bytes + (x as usize) * bytes_per_pixel;
        let end = start + bytes_per_pixel;
        self.data()
            .map(|data| PixelValues::Borrowed(&data[start / 4..end / 4]))
            .ok_or_else(|| eyre!("CPU pixel data is unavailable for this GPU-derived image"))
    }
}

pub static MEAN_PROCESSOR: LazyLock<MeanProcessor> = LazyLock::new(MeanProcessor::new);

pub enum PixelValues<'a> {
    Borrowed(&'a [f32]),
    Owned { values: [f32; 4], len: usize },
}

impl std::ops::Deref for PixelValues<'_> {
    type Target = [f32];

    fn deref(&self) -> &Self::Target {
        match self {
            Self::Borrowed(values) => values,
            Self::Owned { values, len } => &values[..*len],
        }
    }
}

#[derive(Clone)]
pub struct MinMaxTotal {
    mins: Vec<f32>,
    maxs: Vec<f32>,

    total_min: OnceLock<f32>,
    total_max: OnceLock<f32>,
}

pub const EMPTY_MINMAX: MinMaxTotal = MinMaxTotal {
    mins: Vec::new(),
    maxs: Vec::new(),
    total_min: OnceLock::new(),
    total_max: OnceLock::new(),
};

impl MinMaxTotal {
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

    #[inline]
    pub fn min_abs(&self, channel: usize) -> f32 {
        let channel_min = self.min(channel);
        let channel_max = self.max(channel);

        if f32::signum(channel_min) != f32::signum(channel_max) {
            0.0
        } else {
            f32::min(channel_min.abs(), channel_max.abs())
        }
    }

    #[inline]
    pub fn max_abs(&self, channel: usize) -> f32 {
        if channel >= self.maxs.len() {
            0.0
        } else {
            f32::max(self.mins[channel].abs(), self.maxs[channel].abs())
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

    #[inline]
    pub fn total_min_abs(&self) -> f32 {
        let total_min = self.total_min();
        let total_max = self.total_max();
        if f32::signum(total_min) != f32::signum(total_max) {
            0.0
        } else {
            f32::min(total_min.abs(), total_max.abs())
        }
    }

    #[inline]
    pub fn total_max_abs(&self) -> f32 {
        f32::max(self.total_min().abs(), self.total_max().abs())
    }
}

#[derive(Clone)]
pub struct ImageData(Arc<ImageDataInner>);

struct ImageDataInner {
    id: u64,
    spec: ImageSpec,
    storage: ImageStorage,
    gpu: OnceLock<Arc<GpuImageTexture>>,
    gpu_init: Mutex<()>,
    hist: OnceLock<Vec<Vec<f32>>>,
    minmax: OnceLock<MinMaxTotal>,
}

enum ImageStorage {
    Cpu(Arc<[f32]>),
    Derived(DerivedImage),
    Empty,
}

struct DerivedImage {
    primary: ImageData,
    secondary: ImageData,
    mode: crate::model::ComparisonMode,
    blend_alpha: f32,
    channel_strategy: u32,
}

impl ImageData {
    pub fn from_f32(spec: ImageSpec, pixels: Vec<f32>) -> Result<Self> {
        let expected = spec.width.max(0) as usize * spec.height.max(0) as usize * spec.channels.max(0) as usize;
        if pixels.len() != expected {
            return Err(eyre!("Unexpected image data length: {} != {expected}", pixels.len()));
        }
        Ok(Self(Arc::new(ImageDataInner {
            id: new_id(),
            spec,
            storage: ImageStorage::Cpu(pixels.into()),
            gpu: OnceLock::new(),
            gpu_init: Mutex::new(()),
            hist: OnceLock::new(),
            minmax: OnceLock::new(),
        })))
    }

    pub fn empty(dtype: i32) -> Self {
        Self(Arc::new(ImageDataInner {
            id: new_id(),
            spec: ImageSpec::new(0, 0, 0, dtype),
            storage: ImageStorage::Empty,
            gpu: OnceLock::new(),
            gpu_init: Mutex::new(()),
            hist: OnceLock::new(),
            minmax: OnceLock::new(),
        }))
    }

    pub fn derived_comparison(
        primary: ImageData,
        secondary: ImageData,
        spec: ImageSpec,
        mode: crate::model::ComparisonMode,
        blend_alpha: f32,
        channel_strategy: u32,
    ) -> Self {
        Self(Arc::new(ImageDataInner {
            id: new_id(),
            spec,
            storage: ImageStorage::Derived(DerivedImage {
                primary,
                secondary,
                mode,
                blend_alpha,
                channel_strategy,
            }),
            gpu: OnceLock::new(),
            gpu_init: Mutex::new(()),
            hist: OnceLock::new(),
            minmax: OnceLock::new(),
        }))
    }

    pub fn pixels(&self) -> Option<&[f32]> {
        match &self.0.storage {
            ImageStorage::Cpu(pixels) => Some(pixels),
            ImageStorage::Derived(_) | ImageStorage::Empty => None,
        }
    }

    pub fn mean_value_in_rect(&self, rect: Recti, dim: MeanDim) -> Result<Vec<f64>> {
        #[cfg(debug_assertions)]
        let _timer = match dim {
            MeanDim::All => crate::util::timer::ScopedTimer::new("Compute mean all"),
            MeanDim::Column => crate::util::timer::ScopedTimer::new("Compute mean column"),
            MeanDim::Row => crate::util::timer::ScopedTimer::new("Compute mean row"),
        };

        MEAN_PROCESSOR.compute(self, rect, dim)
    }

    pub fn compute_hist(&self) -> Vec<Vec<f32>> {
        #[cfg(debug_assertions)]
        let _timer = crate::util::timer::ScopedTimer::new("Compute hist");

        let spec = self.spec();
        if spec.width == 0 || spec.height == 0 || spec.channels <= 0 {
            return vec![];
        }
        self.gpu_texture()
            .and_then(|texture| gpu_compute()?.histogram(&texture))
            .unwrap_or_else(|error| {
                eprintln!("GPU histogram failed: {error}");
                Vec::new()
            })
    }

    pub fn hist(&self) -> &Vec<Vec<f32>> {
        self.0.hist.get_or_init(|| self.compute_hist())
    }

    fn compute_minmax(&self) -> MinMaxTotal {
        #[cfg(debug_assertions)]
        let _timer = crate::util::timer::ScopedTimer::new("Compute minmax");

        let spec = self.spec();
        if spec.width == 0 || spec.height == 0 || spec.channels <= 0 {
            return MinMaxTotal::new(vec![], vec![]);
        }

        match self
            .gpu_texture()
            .and_then(|texture| gpu_compute()?.minmax(&texture, Recti::ZERO))
        {
            Ok((mins, maxs)) => MinMaxTotal::new(mins, maxs),
            Err(error) => {
                eprintln!("GPU min/max failed: {error}");
                MinMaxTotal::new(vec![0.0; spec.channels as usize], vec![0.0; spec.channels as usize])
            }
        }
    }

    pub fn minmax(&self) -> &MinMaxTotal {
        self.0.minmax.get_or_init(|| self.compute_minmax())
    }

    pub fn from_bytes_size_type(bytes: &[u8], size: Size, dtype: i32) -> Result<ImageData> {
        let depth = dtype.cv_type_depth();
        let channels = dtype.cv_type_channels();
        let element_count = size.width.max(0) as usize * size.height.max(0) as usize * channels.max(0) as usize;
        let expected_bytes = element_count * depth.cv_type_depth_bytes();
        if bytes.len() != expected_bytes {
            return Err(eyre!("Unexpected raw image size: {} != {expected_bytes}", bytes.len()));
        }
        let factor = (1.0 / depth.alpha()) as f32;
        let mut pixels = Vec::with_capacity(element_count);
        for index in 0..element_count {
            pixels.push(read_cv_scalar(bytes, index, depth)? * factor);
        }
        Self::from_f32(ImageSpec::new(size.width, size.height, channels, depth), pixels)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<ImageData> {
        let bytes_mat = core::Mat::new_rows_cols_with_data(1, bytes.len() as i32, bytes)?;
        let mat = imgcodecs::imdecode(&bytes_mat, imgcodecs::IMREAD_UNCHANGED)?;

        if mat.empty() {
            return Err(eyre!("Failed to load image"));
        }

        Self::from_decoded_mat(mat, 1.0, true)
    }

    pub fn load_from_path(path: &PathBuf) -> Result<ImageData> {
        if !path.exists() {
            return Err(eyre!("Image does not exist: {:?}", path));
        }

        // Determine extension for special handling (e.g., PFM)
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();

        let mut scale_abs = 1.0f64;
        let bgr_convert: bool;

        let mat = {
            #[cfg(debug_assertions)]
            let _timer = crate::util::timer::ScopedTimer::new("Image read");

            let is_unicode_path = crate::util::path_ext::is_ascii_path(path);

            if is_unicode_path
                && ext != "pfm"
                && ext != "flo"
                && !crate::supported_image::is_heif_extension(ext.as_str())
            {
                // Read image using imread fails on paths with non-ASCII characters.
                bgr_convert = true;
                imgcodecs::imread(path.to_string_lossy().as_ref(), imgcodecs::IMREAD_UNCHANGED)?
            } else if ext == "exr" {
                // Copy file to temp file with ASCII path and read it
                let temp_dir = crate::util::path_ext::safe_temp_dir();
                let temp_path = temp_dir.join("edolview_temp.exr");
                {
                    #[cfg(debug_assertions)]
                    let _timer_copy = crate::util::timer::ScopedTimer::new("Image file temp copy");

                    fs::copy(&path, &temp_path).map_err(|e| eyre!("Failed to copy file: {e}"))?;
                }
                bgr_convert = true;
                imgcodecs::imread(temp_path.to_string_lossy().as_ref(), imgcodecs::IMREAD_UNCHANGED)?
            } else if crate::supported_image::is_heif_extension(ext.as_str()) {
                #[cfg(not(feature = "heif"))]
                return Err(eyre!("HEIF support is not enabled"));

                #[cfg(feature = "heif")]
                {
                    bgr_convert = false;
                    unsafe { crate::model::image_io::load_heif(path)? }
                }
            } else {
                let mut bytes = fs::read(&path).map_err(|e| eyre!("Failed to read file bytes: {e}"))?;

                if ext == "pfm" {
                    (bytes, scale_abs) = fix_pfm_header_and_parse_scale(&bytes);
                } else if ext == "flo" {
                    return decode_flo(&bytes);
                }
                let bytes_mat = core::Mat::new_rows_cols_with_data(1, bytes.len() as i32, &bytes)?;
                bgr_convert = true;
                imgcodecs::imdecode(&bytes_mat, imgcodecs::IMREAD_UNCHANGED)?
            }
        };

        if mat.empty() {
            return Err(eyre!("Failed to load image"));
        }

        Self::from_decoded_mat(mat, scale_abs, bgr_convert)
    }

    pub fn load_from_clipboard() -> Result<ImageData> {
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

        let pixels = bytes.into_iter().map(|value| value as f32 / 255.0).collect();
        Self::from_f32(ImageSpec::new(width, height, channels, core::CV_8U), pixels)
    }

    pub fn load_from_url(url: &str) -> Result<ImageData> {
        #[cfg(debug_assertions)]
        let _timer = crate::util::timer::ScopedTimer::new("Image download");

        let bytes = ureq::get(url).call()?.body_mut().read_to_vec()?;

        let bytes_mat = core::Mat::new_rows_cols_with_data(1, bytes.len() as i32, &bytes)?;
        let mat = imgcodecs::imdecode(&bytes_mat, imgcodecs::IMREAD_UNCHANGED)?;
        if mat.empty() {
            return Err(eyre!("Failed to load image"));
        }

        Self::from_decoded_mat(mat, 1.0, true)
    }

    pub fn from_decoded_mat(mat: core::Mat, scale: f64, bgr_convert: bool) -> Result<ImageData> {
        #[cfg(debug_assertions)]
        let _timer = crate::util::timer::ScopedTimer::new("Image read postprocess");

        if mat.empty() {
            return Err(eyre!("Failed to load image"));
        }
        let dtype = mat.depth();
        let width = mat.cols();
        let height = mat.rows();
        let channels = mat.channels();
        if !(1..=4).contains(&channels) {
            return Err(eyre!("Unsupported image channels: {channels}"));
        }
        let mat = if mat.is_continuous() { mat } else { mat.try_clone()? };
        let bytes = mat.data_bytes()?;
        let element_count = width as usize * height as usize * channels as usize;
        let factor = (scale / dtype.alpha()) as f32;
        let mut pixels = Vec::with_capacity(element_count);
        for index in 0..element_count {
            pixels.push(read_cv_scalar(bytes, index, dtype)? * factor);
        }
        if bgr_convert && channels >= 3 {
            for pixel in pixels.chunks_exact_mut(channels as usize) {
                pixel.swap(0, 2);
            }
        }
        Self::from_f32(ImageSpec::new(width, height, channels, dtype), pixels)
    }

    pub fn gpu_texture(&self) -> Result<Arc<GpuImageTexture>> {
        if let Some(texture) = self.0.gpu.get() {
            return Ok(Arc::clone(texture));
        }
        let _init_guard = self.0.gpu_init.lock().unwrap();
        if let Some(texture) = self.0.gpu.get() {
            return Ok(Arc::clone(texture));
        }
        let compute = gpu_compute()?;
        let texture = match &self.0.storage {
            ImageStorage::Cpu(pixels) => compute.upload(&self.0.spec, pixels)?,
            ImageStorage::Derived(derived) => {
                let primary = derived.primary.gpu_texture()?;
                let secondary = derived.secondary.gpu_texture()?;
                compute.compare(
                    &primary,
                    &secondary,
                    &self.0.spec,
                    derived.mode,
                    derived.blend_alpha,
                    derived.channel_strategy,
                )?
            }
            ImageStorage::Empty => return Err(eyre!("Image is empty")),
        };
        let _ = self.0.gpu.set(Arc::clone(&texture));
        Ok(self.0.gpu.get().cloned().unwrap_or(texture))
    }

    fn pixel_values(&self, x: i32, y: i32) -> Result<Vec<f32>> {
        let spec = self.spec();
        if x < 0 || x >= spec.width || y < 0 || y >= spec.height {
            return Err(eyre!("Coordinates out of bounds"));
        }
        match &self.0.storage {
            ImageStorage::Cpu(pixels) => {
                let start = ((y * spec.width + x) * spec.channels) as usize;
                Ok(pixels[start..start + spec.channels as usize].to_vec())
            }
            ImageStorage::Derived(derived) => {
                let mut lhs = derived.primary.pixel_values(x, y)?;
                let mut rhs = derived.secondary.pixel_values(x, y)?;
                normalize_pixel_channels(&mut lhs, &mut rhs, derived.channel_strategy, spec.channels as usize);
                Ok(lhs
                    .iter()
                    .zip(rhs.iter())
                    .take(spec.channels as usize)
                    .map(|(&a, &b)| match derived.mode {
                        crate::model::ComparisonMode::Diff => a - b,
                        crate::model::ComparisonMode::Blend => {
                            a * (1.0 - derived.blend_alpha) + b * derived.blend_alpha
                        }
                        crate::model::ComparisonMode::Split => a,
                    })
                    .collect())
            }
            ImageStorage::Empty => Err(eyre!("Image is empty")),
        }
    }

    pub(crate) fn scalar_at(&self, pixel_index: usize, channel: usize) -> Option<f32> {
        let spec = self.spec();
        if pixel_index >= spec.width as usize * spec.height as usize || channel >= spec.channels as usize {
            return None;
        }
        match &self.0.storage {
            ImageStorage::Cpu(pixels) => pixels.get(pixel_index * spec.channels as usize + channel).copied(),
            ImageStorage::Derived(derived) => {
                let primary_channel = if derived.channel_strategy == 1 { 0 } else { channel };
                let secondary_channel = if derived.channel_strategy == 2 { 0 } else { channel };
                let lhs = derived.primary.scalar_at(pixel_index, primary_channel)?;
                let rhs = derived.secondary.scalar_at(pixel_index, secondary_channel)?;
                Some(match derived.mode {
                    crate::model::ComparisonMode::Diff => lhs - rhs,
                    crate::model::ComparisonMode::Blend => {
                        lhs * (1.0 - derived.blend_alpha) + rhs * derived.blend_alpha
                    }
                    crate::model::ComparisonMode::Split => lhs,
                })
            }
            ImageStorage::Empty => None,
        }
    }
}

impl Image for ImageData {
    fn id(&self) -> u64 {
        self.0.id
    }

    fn spec(&self) -> ImageSpec {
        self.0.spec.clone()
    }

    fn data(&self) -> Option<&[f32]> {
        self.pixels()
    }

    fn gpu_texture(&self) -> Result<Arc<GpuImageTexture>> {
        ImageData::gpu_texture(self)
    }

    fn get_pixel_at(&self, x: i32, y: i32) -> Result<PixelValues<'_>> {
        if let Some(pixels) = self.pixels() {
            let spec = self.spec();
            if x < 0 || x >= spec.width || y < 0 || y >= spec.height {
                return Err(eyre!("Coordinates out of bounds"));
            }
            let start = ((y * spec.width + x) * spec.channels) as usize;
            return Ok(PixelValues::Borrowed(&pixels[start..start + spec.channels as usize]));
        }
        let spec = self.spec();
        let pixel = self.pixel_values(x, y)?;
        let mut values = [0.0; 4];
        values[..pixel.len()].copy_from_slice(&pixel);
        Ok(PixelValues::Owned {
            values,
            len: spec.channels as usize,
        })
    }
}

fn read_cv_scalar(bytes: &[u8], index: usize, dtype: i32) -> Result<f32> {
    let value = match dtype {
        core::CV_8U => bytes[index] as f32,
        core::CV_8S => (bytes[index] as i8) as f32,
        core::CV_16U => u16::from_ne_bytes(bytes[index * 2..index * 2 + 2].try_into()?) as f32,
        core::CV_16S => i16::from_ne_bytes(bytes[index * 2..index * 2 + 2].try_into()?) as f32,
        core::CV_32S => i32::from_ne_bytes(bytes[index * 4..index * 4 + 4].try_into()?) as f32,
        core::CV_32F => f32::from_ne_bytes(bytes[index * 4..index * 4 + 4].try_into()?),
        core::CV_64F => f64::from_ne_bytes(bytes[index * 8..index * 8 + 8].try_into()?) as f32,
        core::CV_16F => half::f16::from_bits(u16::from_ne_bytes(bytes[index * 2..index * 2 + 2].try_into()?)).to_f32(),
        _ => return Err(eyre!("Unsupported OpenCV image depth: {dtype}")),
    };
    Ok(value)
}

fn normalize_pixel_channels(lhs: &mut Vec<f32>, rhs: &mut Vec<f32>, strategy: u32, output_channels: usize) {
    match strategy {
        1 => lhs.resize(output_channels, lhs[0]),
        2 => rhs.resize(output_channels, rhs[0]),
        _ => {
            lhs.truncate(output_channels);
            rhs.truncate(output_channels);
        }
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

// Decode Middlebury .flo optical flow directly into the application's image storage.
// Format (little-endian):
// - magic: f32 (202021.25)
// - width: i32
// - height: i32
// - data: width * height * 2 f32 (u, v) in row-major, interleaved
fn decode_flo(bytes: &[u8]) -> Result<ImageData> {
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

    let mut pixels = Vec::with_capacity(num_floats);
    for chunk in bytes[12..12 + data_bytes].chunks_exact(4) {
        pixels.push(f32::from_le_bytes(chunk.try_into().unwrap()));
    }
    ImageData::from_f32(ImageSpec::new(width, height, 2, core::CV_32F), pixels)
}
