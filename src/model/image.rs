use crate::util::cv_ext::CvIntExt;
use opencv::prelude::*;

pub struct ImageSpec {
	pub width: i32,
	pub height: i32,
	pub channels: i32,
	pub cv_type: i32, // OpenCV type, e.g. CV_8U, CV_32F
}

impl ImageSpec {
	pub fn new(mat: &opencv::core::Mat) -> Self {
		Self {
			width: mat.cols(),
			height: mat.rows(),
			channels: mat.channels(),
			cv_type: mat.depth(),
		}
	}

	pub fn total_bytes(&self) -> usize {
		(self.width as usize) * (self.height as usize) * (self.channels as usize) * self.cv_type.cv_type_bytes()
	}
	
	pub fn bytes_per_pixel(&self) -> usize {
		(self.channels as usize) * self.cv_type.cv_type_bytes()
	}
}