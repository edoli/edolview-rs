/// Bit shift for the channel part inside OpenCV Mat type flags.
const CV_CN_SHIFT: i32 = 3; // from OpenCV core.hpp
/// Maximum number of channels representable (OpenCV uses up to 512, but mask below limits calc).
/// We only need mask constants for extraction; keeping them small & simple.
const CV_MAT_DEPTH_MASK: i32 = (1 << CV_CN_SHIFT) - 1; // = 7

use std::ffi::c_void;

// Public re-exports of depth constants for ergonomic use (so callers only import this module).
pub use opencv::core;
use opencv::core::MatTraitConst;

/// Return the (0..7) depth code stored in a Mat type flag.
#[inline]
pub fn cv_depth_of(cv_type: i32) -> i32 {
    cv_type & CV_MAT_DEPTH_MASK
}

#[inline]
pub fn cv_channels_of(cv_type: i32) -> i32 {
    ((cv_type >> CV_CN_SHIFT) & 0x1F) + 1
}

#[inline]
pub fn cv_depth_bytes_of(depth: i32) -> usize {
    match depth {
        core::CV_8U | core::CV_8S => 1,
        core::CV_16U | core::CV_16S => 2,
        core::CV_32S | core::CV_32F => 4,
        core::CV_64F => 8,
        _ => 1, // Default to 1 byte per channel for unknown types
    }
}

#[inline]
pub fn cv_bytes_of(cv_type: i32) -> usize {
    let depth = cv_depth_of(cv_type);
    let channels = cv_channels_of(cv_type);
    let bytes_per_channel = cv_depth_bytes_of(depth);
    bytes_per_channel * (channels as usize)
}

/// Construct a Mat type code from depth (e.g. `CV_8U`) and channel count.
#[inline]
pub fn cv_make_type(depth: i32, channels: i32) -> i32 {
    (depth & CV_MAT_DEPTH_MASK) + ((channels - 1) << CV_CN_SHIFT)
}

pub trait CvIntExt {
    fn cv_type_depth(self) -> i32;
    fn cv_type_channels(self) -> i32;
    fn cv_type_with_precision(self, new_depth: i32) -> i32;
    fn cv_type_with_channels(self, new_channels: i32) -> i32;
    fn cv_type_bytes(self) -> usize;
    fn cv_type_depth_bytes(self) -> usize;
    fn cv_type_is_floating(self) -> bool;
    fn cv_type_name(self) -> &'static str;
    fn alpha(self) -> f64;
}

impl CvIntExt for i32 {
    fn cv_type_depth(self) -> i32 {
        cv_depth_of(self)
    }

    fn cv_type_channels(self) -> i32 {
        cv_channels_of(self)
    }

    fn cv_type_with_precision(self, new_depth: i32) -> i32 {
        let ch = cv_channels_of(self);
        cv_make_type(new_depth, ch)
    }

    fn cv_type_with_channels(self, new_channels: i32) -> i32 {
        let depth = cv_depth_of(self);
        cv_make_type(depth, new_channels)
    }

    fn cv_type_bytes(self) -> usize {
        cv_bytes_of(self)
    }

    fn cv_type_depth_bytes(self) -> usize {
        cv_depth_bytes_of(self.cv_type_depth())
    }

    fn cv_type_is_floating(self) -> bool {
        matches!(self.cv_type_depth(), core::CV_32F | core::CV_64F | core::CV_16F)
    }

    fn cv_type_name(self) -> &'static str {
        match self {
            core::CV_8U => "uint8",
            core::CV_8S => "int8",
            core::CV_16U => "uint16",
            core::CV_16S => "int16",
            core::CV_32S => "int32",
            core::CV_32F => "float32",
            core::CV_64F => "float64",
            core::CV_16F => "float16",
            _ => "unknown",
        }
    }

    fn alpha(self) -> f64 {
        match self {
            core::CV_8U => 255.0,
            core::CV_8S => 127.0,
            core::CV_16U => 65535.0,
            core::CV_16S => 32767.0,
            core::CV_32S => 2147483647.0,
            core::CV_32F => 1.0,
            core::CV_64F => 1.0,
            _ => 1.0,
        }
    }
}

pub fn parse_cv_depth(s: &str) -> i32 {
    match s {
        "float64" => core::CV_64F,
        "float32" => core::CV_32F,
        "float16" => core::CV_16F,
        "uint16" => core::CV_16U,
        "uint8" => core::CV_8U,
        "int32" => core::CV_32S,
        "int16" => core::CV_16S,
        "int8" => core::CV_8S,
        _ => -1,
    }
}

pub fn parse_cv_type(s: &str, channels: i32) -> i32 {
    parse_cv_depth(s).cv_type_with_channels(channels)
}

pub trait MatExt {
    fn shallow_clone(&self) -> Result<opencv::core::Mat, opencv::Error>;
}

impl MatExt for opencv::core::Mat {
    fn shallow_clone(&self) -> Result<opencv::core::Mat, opencv::Error> {
        unsafe {
            let data = self.data();
            let rows = self.rows();
            let cols = self.cols();
            let typ = self.typ();

            opencv::core::Mat::new_rows_cols_with_data_unsafe_def(rows, cols, typ, data as *mut c_void)
        }
    }
}
