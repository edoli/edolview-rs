use crate::util::cv_ext::CvIntExt;
use color_eyre::eyre::Result;
use opencv::prelude::*;
use std::sync::atomic::{AtomicU64, Ordering};

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