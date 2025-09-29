use std::sync::Arc;

use crate::model::{Image, MatImage};

pub type SharedAsset = Arc<dyn Asset<MatImage>>;

pub enum AssetType {
    File,
    Clipboard,
    Socket,
    Url,
    Comparison,
}

pub trait Asset<T: Image> {
    fn name(&self) -> &str;
    fn image(&self) -> &T;
    fn hash(&self) -> &str;
    fn asset_type(&self) -> AssetType;
}

pub struct FileAsset {
    path: String,
    image: MatImage,
}

impl FileAsset {
    pub fn new(path: String, image: MatImage) -> Self {
        Self { path, image }
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
        &self.path
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

impl ComparisonAsset {
    pub fn new(asset_primary: SharedAsset, asset_secondary: SharedAsset) -> Self {
        let name = format!("Comparison: {} vs {}", asset_primary.name(), asset_secondary.name());

        let img1 = asset_primary.image();
        let img2 = asset_secondary.image();

        let mat1 = img1.mat();
        let mat2 = img2.mat();

        let mut mat = opencv::core::Mat::default();

        opencv::core::subtract(&mat1, &mat2, &mut mat, &opencv::core::no_array(), -1).unwrap();

        let comparison_image = MatImage::new(mat, img1.spec().dtype);

        Self {
            name,
            image: comparison_image,
        }
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
