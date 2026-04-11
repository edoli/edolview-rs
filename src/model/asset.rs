use std::{path::PathBuf, sync::Arc};

use opencv::core::{MatTrait, MatTraitConst};

use crate::model::{Image, MatImage};
use crate::util::cv_ext::{CvIntExt, CvMatExt};

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
    Split,
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

#[derive(Clone, Copy)]
enum ChannelComparisonStrategy {
    Match,
    DuplicatePrimaryToChannels(i32),
    DuplicateSecondaryToChannels(i32),
    UsePrimaryFirstChannels(i32),
    UseSecondaryFirstChannels(i32),
    Unsupported,
}

impl ChannelComparisonStrategy {
    fn from_channels(channels1: i32, channels2: i32) -> Self {
        match (channels1, channels2) {
            (c1, c2) if c1 == c2 => Self::Match,
            (1, 2) | (1, 3) | (1, 4) => Self::DuplicatePrimaryToChannels(channels2),
            (2, 1) | (3, 1) | (4, 1) => Self::DuplicateSecondaryToChannels(channels1),
            (2, 3) | (2, 4) => Self::UsePrimaryFirstChannels(2),
            (3, 2) | (4, 2) => Self::UseSecondaryFirstChannels(2),
            (3, 4) => Self::UsePrimaryFirstChannels(3),
            (4, 3) => Self::UseSecondaryFirstChannels(3),
            _ => Self::Unsupported,
        }
    }

    fn notice(self, channels1: i32, channels2: i32) -> Option<String> {
        match self {
            Self::Match => None,
            Self::DuplicatePrimaryToChannels(_) => Some(format!(
                "Channel mismatch: comparing the single channel image against each channel of the {channels2}-channel image."
            )),
            Self::DuplicateSecondaryToChannels(_) => Some(format!(
                "Channel mismatch: comparing each channel of the {channels1}-channel image against the single channel image."
            )),
            Self::UsePrimaryFirstChannels(2) => {
                Some(format!("Channel mismatch: showing the first two channels only from the {channels2}-channel image."))
            }
            Self::UseSecondaryFirstChannels(2) => {
                Some(format!("Channel mismatch: showing the first two channels only from the {channels1}-channel image."))
            }
            Self::UsePrimaryFirstChannels(3) | Self::UseSecondaryFirstChannels(3) => {
                Some("Channel mismatch: showing RGB comparison only; the 4-channel image alpha is ignored.".to_string())
            }
            Self::UsePrimaryFirstChannels(_) | Self::UseSecondaryFirstChannels(_) => None,
            Self::Unsupported => Some(format!(
                "Channel mismatch: unsupported comparison between {} and {} channels.",
                channels1, channels2
            )),
        }
    }

    fn normalize(
        self,
        mat1: &opencv::core::Mat,
        mat2: &opencv::core::Mat,
    ) -> Option<(opencv::core::Mat, opencv::core::Mat)> {
        match self {
            Self::Match => Some((mat1.clone(), mat2.clone())),
            Self::DuplicatePrimaryToChannels(channels) => {
                Some((mat1.duplicate_mono_to_channels(channels), mat2.clone()))
            }
            Self::DuplicateSecondaryToChannels(channels) => {
                Some((mat1.clone(), mat2.duplicate_mono_to_channels(channels)))
            }
            Self::UsePrimaryFirstChannels(channels) => Some((mat1.clone(), mat2.take_first_channels(channels))),
            Self::UseSecondaryFirstChannels(channels) => Some((mat1.take_first_channels(channels), mat2.clone())),
            Self::Unsupported => None,
        }
    }
}

fn build_comparison_notices(
    strategy: ChannelComparisonStrategy,
    spec1: &crate::model::ImageSpec,
    spec2: &crate::model::ImageSpec,
) -> Vec<String> {
    let mut notices = Vec::new();
    if let Some(notice) = strategy.notice(spec1.channels, spec2.channels) {
        notices.push(notice);
    }
    if spec1.dtype != spec2.dtype {
        notices.push(format!(
            "Data type mismatch: comparing {} against {}.",
            spec1.dtype.cv_type_name(),
            spec2.dtype.cv_type_name()
        ));
    }
    notices
}

impl ComparisonAsset {
    pub fn new(
        asset_primary: SharedAsset,
        asset_secondary: SharedAsset,
        mode: ComparisonMode,
        blend_alpha: f32,
    ) -> (Self, Option<String>) {
        let name = format!(
            "Comparison ({:?}): {} vs {}",
            mode,
            asset_primary.name(),
            asset_secondary.name()
        );

        let img1 = asset_primary.image();
        let img2 = asset_secondary.image();
        let spec1 = img1.spec();
        let spec2 = img2.spec();

        let mat1 = img1.mat();
        let mat2 = img2.mat();
        let strategy = ChannelComparisonStrategy::from_channels(mat1.channels(), mat2.channels());
        let comparison_notices = build_comparison_notices(strategy, &spec1, &spec2);

        if mode == ComparisonMode::Split {
            return (
                Self {
                    name,
                    image: MatImage::new(mat1.clone(), spec1.dtype),
                },
                (!comparison_notices.is_empty()).then(|| comparison_notices.join("\n")),
            );
        }

        let Some((mat1, mat2)) = strategy.normalize(mat1, mat2) else {
            return (
                Self {
                    name,
                    image: MatImage::new(opencv::core::Mat::default(), img1.spec().dtype),
                },
                (!comparison_notices.is_empty()).then(|| comparison_notices.join("\n")),
            );
        };

        let mut mat = mat1.clone();
        let roi_rect = opencv::core::Rect::new(0, 0, mat1.cols().min(mat2.cols()), mat1.rows().min(mat2.rows()));

        if roi_rect.width > 0 && roi_rect.height > 0 {
            let mat1_roi = mat1.roi(roi_rect).unwrap();
            let mat2_roi = mat2.roi(roi_rect).unwrap();
            let mut mat_roi = mat.roi_mut(roi_rect).unwrap();

            match mode {
                ComparisonMode::Diff => {
                    opencv::core::subtract(&mat1_roi, &mat2_roi, &mut mat_roi, &opencv::core::no_array(), -1).unwrap();
                }
                ComparisonMode::Blend => {
                    let alpha = blend_alpha.clamp(0.0, 1.0) as f64;
                    opencv::core::add_weighted(&mat1_roi, 1.0 - alpha, &mat2_roi, alpha, 0.0, &mut mat_roi, -1)
                        .unwrap();
                }
                ComparisonMode::Split => unreachable!("split comparison is rendered directly by the viewer"),
            }
        }

        let comparison_image = MatImage::new(mat, spec1.dtype);

        (
            Self {
                name,
                image: comparison_image,
            },
            (!comparison_notices.is_empty()).then(|| comparison_notices.join("\n")),
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
