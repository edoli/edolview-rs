use std::{path::PathBuf, sync::Arc};

use opencv::core::{MatTrait, MatTraitConst};

use crate::model::{Image, MatImage, Recti};
use crate::util::{cv_ext::CvMatExt, math_ext::vec2i};

pub type SharedAsset = Arc<dyn Asset<MatImage>>;

#[derive(PartialEq)]
pub enum AssetType {
    File,
    Clipboard,
    Socket,
    Url,
    Comparison,
}

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum ComparisonMode {
    Diff,
    Blend,
    Rect,
}

pub trait Asset<T: Image> {
    fn name(&self) -> &str;
    fn image(&self) -> &T;
    fn hash(&self) -> &str;
    fn asset_type(&self) -> AssetType;
}

pub struct FileAsset {
    path: String,
    hash: String,
    image: MatImage,
}

impl FileAsset {
    pub fn new(path: String, hash: String, image: MatImage) -> Self {
        Self { path, hash, image }
    }

    pub fn hash_from_path(path: &PathBuf) -> std::io::Result<String> {
        let meta = std::fs::metadata(path)?;
        let modified = meta.modified()?;
        let duration = modified.duration_since(std::time::UNIX_EPOCH).unwrap();

        let modified_ns = duration.as_nanos();
        let len = meta.len();

        // key: "path|mtime_ns|len"
        Ok(format!("{}|{}|{}", path.to_string_lossy(), modified_ns, len))
    }
}

impl Asset<MatImage> for FileAsset {
    fn name(&self) -> &str {
        &self.path
    }

    fn image(&self) -> &MatImage {
        &self.image
    }

    fn hash(&self) -> &str {
        &self.hash
    }

    fn asset_type(&self) -> AssetType {
        AssetType::File
    }
}

pub struct ClipboardAsset {
    name: String,
    image: MatImage,
}

static CLIPBOARD_COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

impl ClipboardAsset {
    pub fn new(image: MatImage) -> Self {
        Self {
            name: format!(
                "Clipboard {}",
                CLIPBOARD_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            ),
            image,
        }
    }
}

impl Asset<MatImage> for ClipboardAsset {
    fn name(&self) -> &str {
        &self.name
    }

    fn image(&self) -> &MatImage {
        &self.image
    }

    fn hash(&self) -> &str {
        &self.name
    }

    fn asset_type(&self) -> AssetType {
        AssetType::Clipboard
    }
}

pub struct SocketAsset {
    name: String,
    image: MatImage,
}

impl SocketAsset {
    pub fn new(name: String, image: MatImage) -> Self {
        Self { name, image }
    }
}

impl Asset<MatImage> for SocketAsset {
    fn name(&self) -> &str {
        &self.name
    }

    fn image(&self) -> &MatImage {
        &self.image
    }

    fn hash(&self) -> &str {
        &self.name
    }

    fn asset_type(&self) -> AssetType {
        AssetType::Socket
    }
}

pub struct UrlAsset {
    url: String,
    image: MatImage,
}

impl UrlAsset {
    pub fn new(url: String, image: MatImage) -> Self {
        Self { url, image }
    }
}

impl Asset<MatImage> for UrlAsset {
    fn name(&self) -> &str {
        &self.url
    }

    fn image(&self) -> &MatImage {
        &self.image
    }

    fn hash(&self) -> &str {
        &self.url
    }

    fn asset_type(&self) -> AssetType {
        AssetType::Url
    }
}

pub struct ComparisonAsset {
    name: String,
    image: MatImage,
}

fn normalize_comparison_inputs(
    mat1: &opencv::core::Mat,
    mat2: &opencv::core::Mat,
) -> Option<(opencv::core::Mat, opencv::core::Mat, String)> {
    let channels1 = mat1.channels();
    let channels2 = mat2.channels();

    match (channels1, channels2) {
        (1, 3) | (1, 4) => Some((
            mat1.duplicate_mono_to_channels(channels2),
            mat2.clone(),
            format!(
                "Channel mismatch: comparing the single channel image against each channel of the {channels2}-channel image."
            ),
        )),
        (3, 1) | (4, 1) => Some((
            mat1.clone(),
            mat2.duplicate_mono_to_channels(channels1),
            format!(
                "Channel mismatch: comparing each channel of the {channels1}-channel image against the single channel image."
            ),
        )),
        (3, 4) => Some((
            mat1.clone(),
            mat2.drop_alpha_channel(),
            "Channel mismatch: showing RGB comparison only; the 4-channel image alpha is ignored.".to_string(),
        )),
        (4, 3) => Some((
            mat1.drop_alpha_channel(),
            mat2.clone(),
            "Channel mismatch: showing RGB comparison only; the 4-channel image alpha is ignored.".to_string(),
        )),
        _ => None,
    }
}

impl ComparisonAsset {
    pub fn new(
        asset_primary: SharedAsset,
        asset_secondary: SharedAsset,
        mode: ComparisonMode,
        blend_alpha: f32,
        rect: Option<Recti>,
    ) -> (Self, Option<String>) {
        let name = format!(
            "Comparison ({:?}): {} vs {}",
            mode,
            asset_primary.name(),
            asset_secondary.name()
        );

        let img1 = asset_primary.image();
        let img2 = asset_secondary.image();

        let mat1 = img1.mat();
        let mat2 = img2.mat();

        let (mat1, mat2, comparison_notice) = if mat1.channels() == mat2.channels() {
            (mat1.clone(), mat2.clone(), None)
        } else if let Some((mat1, mat2, notice)) = normalize_comparison_inputs(mat1, mat2) {
            (mat1, mat2, Some(notice))
        } else {
            return (
                Self {
                    name,
                    image: MatImage::new(opencv::core::Mat::default(), img1.spec().dtype),
                },
                Some(format!(
                    "Channel mismatch: unsupported comparison between {} and {} channels.",
                    mat1.channels(),
                    mat2.channels()
                )),
            );
        };

        let mut mat = mat1.clone();
        let full_rect =
            Recti::from_min_size(vec2i(0, 0), vec2i(mat1.cols().min(mat2.cols()), mat1.rows().min(mat2.rows())));

        let roi_rect = match mode {
            ComparisonMode::Rect => rect.unwrap_or(Recti::ZERO).validate().intersect(full_rect),
            _ => full_rect,
        };

        if !roi_rect.empty() {
            let roi_cv = roi_rect.to_cv_rect();
            let mat1_roi = mat1.roi(roi_cv).unwrap();
            let mat2_roi = mat2.roi(roi_cv).unwrap();
            let mut mat_roi = mat.roi_mut(roi_cv).unwrap();

            match mode {
                ComparisonMode::Diff => {
                    opencv::core::subtract(&mat1_roi, &mat2_roi, &mut mat_roi, &opencv::core::no_array(), -1).unwrap();
                }
                ComparisonMode::Blend => {
                    let alpha = blend_alpha.clamp(0.0, 1.0) as f64;
                    opencv::core::add_weighted(&mat1_roi, 1.0 - alpha, &mat2_roi, alpha, 0.0, &mut mat_roi, -1)
                        .unwrap();
                }
                ComparisonMode::Rect => {
                    mat2_roi.copy_to(&mut mat_roi).unwrap();
                }
            }
        }

        let comparison_image = MatImage::new(mat, img1.spec().dtype);

        (
            Self {
                name,
                image: comparison_image,
            },
            comparison_notice,
        )
    }
}

impl Asset<MatImage> for ComparisonAsset {
    fn name(&self) -> &str {
        &self.name
    }

    fn image(&self) -> &MatImage {
        &self.image
    }

    fn hash(&self) -> &str {
        &self.name
    }

    fn asset_type(&self) -> AssetType {
        AssetType::Comparison
    }
}
