use color_eyre::eyre::Result;
use std::{path::PathBuf, sync::Arc};

use crate::model::{Image, ImageData};

pub type SharedAsset = Arc<dyn Asset<ImageData>>;

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
    image: ImageData,
}

impl FileAsset {
    pub fn new(path: String, hash: String, image: ImageData) -> Self {
        Self { path, hash, image }
    }

    pub fn hash_from_path(path: &PathBuf) -> Result<String> {
        let meta = std::fs::metadata(path)?;
        let modified = meta.modified()?;
        let duration = modified.duration_since(std::time::UNIX_EPOCH).unwrap();

        let modified_ns = duration.as_nanos();
        let len = meta.len();

        // key: "path|mtime_ns|len"
        Ok(format!("{}|{}|{}", path.to_string_lossy(), modified_ns, len))
    }
}

impl Asset<ImageData> for FileAsset {
    fn name(&self) -> &str {
        &self.path
    }

    fn image(&self) -> &ImageData {
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
    image: ImageData,
}

static CLIPBOARD_COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

impl ClipboardAsset {
    pub fn new(image: ImageData) -> Self {
        Self {
            name: format!(
                "Clipboard {}",
                CLIPBOARD_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            ),
            image,
        }
    }
}

impl Asset<ImageData> for ClipboardAsset {
    fn name(&self) -> &str {
        &self.name
    }

    fn image(&self) -> &ImageData {
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
    image: ImageData,
}

impl SocketAsset {
    pub fn new(name: String, image: ImageData) -> Self {
        Self { name, image }
    }
}

impl Asset<ImageData> for SocketAsset {
    fn name(&self) -> &str {
        &self.name
    }

    fn image(&self) -> &ImageData {
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
    image: ImageData,
}

impl UrlAsset {
    pub fn new(url: String, image: ImageData) -> Self {
        Self { url, image }
    }
}

impl Asset<ImageData> for UrlAsset {
    fn name(&self) -> &str {
        &self.url
    }

    fn image(&self) -> &ImageData {
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
    image: ImageData,
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

    fn output_channels(self, channels1: i32, channels2: i32) -> Option<i32> {
        match self {
            Self::Match => Some(channels1),
            Self::DuplicatePrimaryToChannels(_) => Some(channels2),
            Self::DuplicateSecondaryToChannels(_) => Some(channels1),
            Self::UsePrimaryFirstChannels(channels) | Self::UseSecondaryFirstChannels(channels) => Some(channels),
            Self::Unsupported => None,
        }
    }

    fn gpu_code(self) -> u32 {
        match self {
            Self::DuplicatePrimaryToChannels(_) => 1,
            Self::DuplicateSecondaryToChannels(_) => 2,
            _ => 0,
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
            spec1.dtype.name(),
            spec2.dtype.name()
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

        let strategy = ChannelComparisonStrategy::from_channels(spec1.channels, spec2.channels);
        let comparison_notices = build_comparison_notices(strategy, &spec1, &spec2);

        if mode == ComparisonMode::Split {
            return (
                Self {
                    name,
                    image: img1.clone(),
                },
                (!comparison_notices.is_empty()).then(|| comparison_notices.join("\n")),
            );
        }

        let Some(output_channels) = strategy.output_channels(spec1.channels, spec2.channels) else {
            return (
                Self {
                    name,
                    image: ImageData::empty(img1.spec().dtype),
                },
                (!comparison_notices.is_empty()).then(|| comparison_notices.join("\n")),
            );
        };

        let comparison_image = ImageData::derived_comparison(
            img1.clone(),
            img2.clone(),
            crate::model::ImageSpec::new(spec1.width, spec1.height, output_channels, spec1.dtype),
            mode,
            blend_alpha,
            strategy.gpu_code(),
        );

        (
            Self {
                name,
                image: comparison_image,
            },
            (!comparison_notices.is_empty()).then(|| comparison_notices.join("\n")),
        )
    }
}

impl Asset<ImageData> for ComparisonAsset {
    fn name(&self) -> &str {
        &self.name
    }

    fn image(&self) -> &ImageData {
        &self.image
    }

    fn hash(&self) -> &str {
        &self.name
    }

    fn asset_type(&self) -> AssetType {
        AssetType::Comparison
    }
}
