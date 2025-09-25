use crate::model::{Image, MatImage};

pub trait Asset<T: Image> {
    fn name(&self) -> &str;
    fn image(&self) -> &T;
    fn hash(&self) -> &str;
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
}

pub struct ClipboardAsset {
    name: String,
    image: MatImage,
}

pub struct SocketAsset {
    url: String,
    image: MatImage,
}

pub struct UrlAsset {
    url: String,
    image: MatImage,
}
