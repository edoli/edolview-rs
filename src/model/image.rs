use crate::util::cv_ext::CvIntExt;
use color_eyre::eyre::Result;
use opencv::prelude::*;
use half::f16;
use std::sync::atomic::{AtomicU64, Ordering};

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

impl ImageSpec {
    pub fn new(mat: &opencv::core::Mat) -> Self {
        Self {
            width: mat.cols(),
            height: mat.rows(),
            channels: mat.channels(),
            dtype: mat.depth(),
        }
    }

    pub fn total_bytes(&self) -> usize {
        (self.width as usize) * (self.height as usize) * (self.channels as usize) * self.dtype.cv_type_bytes()
    }

    pub fn bytes_per_pixel(&self) -> usize {
        (self.channels as usize) * self.dtype.cv_type_bytes()
    }
}

pub trait Image {
    fn id(&self) -> u64;
    fn spec(&self) -> &ImageSpec;
    fn data_ptr(&self) -> Result<&[u8]>;
    fn get_pixel_at(&self, x: i32, y: i32) -> Result<&[u8]> {
        let spec = self.spec();
        if x < 0 || x >= spec.width || y < 0 || y >= spec.height {
            return Err(color_eyre::eyre::eyre!("Coordinates out of bounds"));
        }
        let bytes_per_pixel = spec.bytes_per_pixel();
        let row_bytes = (spec.width as usize) * bytes_per_pixel;
        let start = (y as usize) * row_bytes + (x as usize) * bytes_per_pixel;
        let end = start + bytes_per_pixel;
        let data = self.data_ptr()?;
        Ok(&data[start..end])
    }
}

pub struct MatImage {
    id: u64,
    mat: opencv::core::Mat,
    spec: ImageSpec,
}

impl MatImage {
    pub fn new(mat: opencv::core::Mat) -> Self {
        let spec = ImageSpec::new(&mat);
        Self {
            mat,
            spec,
            id: new_id(),
        }
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
