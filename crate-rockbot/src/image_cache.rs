use std::collections::HashMap;
use std::sync::Mutex;

use base64::Engine;

use crate::validated::NonEmptyString;

#[derive(Clone)]
pub struct GeneratedImage {
    pub webdav_path: NonEmptyString,
    pub image_bytes: Vec<u8>,
    pub mime_type: NonEmptyString,
    pub share_url: Option<String>,
}

impl GeneratedImage {
    pub fn data_uri(&self) -> String {
        let b64 = base64::engine::general_purpose::STANDARD.encode(&self.image_bytes);
        format!("data:{};base64,{}", self.mime_type.as_str(), b64)
    }

    pub fn file_extension(&self) -> &str {
        match self.mime_type.as_str() {
            "image/jpeg" | "image/jpg" => "jpg",
            "image/png" => "png",
            "image/webp" => "webp",
            "image/svg+xml" => "svg",
            _ => "png",
        }
    }
}

pub struct ImageCache {
    entries: Mutex<HashMap<String, GeneratedImage>>,
}

impl Default for ImageCache {
    fn default() -> Self {
        Self::new()
    }
}

impl ImageCache {
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
        }
    }

    pub fn store(&self, key: &str, image: GeneratedImage) {
        self.entries
            .lock()
            .unwrap()
            .insert(key.to_string(), image);
    }

    pub fn take(&self, key: &str) -> Option<GeneratedImage> {
        self.entries.lock().unwrap().remove(key)
    }

    pub fn get(&self, key: &str) -> Option<GeneratedImage> {
        self.entries.lock().unwrap().get(key).cloned()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.lock().unwrap().is_empty()
    }
}

pub fn image_markdown(description: &str, url: &str) -> String {
    format!("![{}]({})", description, url)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(mime: &str) -> GeneratedImage {
        GeneratedImage {
            webdav_path: NonEmptyString::try_new("r-test/image".to_string()).unwrap(),
            image_bytes: Vec::new(),
            mime_type: NonEmptyString::try_new(mime.to_string()).unwrap(),
            share_url: None,
        }
    }

    #[test]
    fn file_extension_maps_supported_mime_types() {
        assert_eq!(image("image/jpeg").file_extension(), "jpg");
        assert_eq!(image("image/jpg").file_extension(), "jpg");
        assert_eq!(image("image/png").file_extension(), "png");
        assert_eq!(image("image/webp").file_extension(), "webp");
        assert_eq!(image("image/svg+xml").file_extension(), "svg");
        assert_eq!(image("application/octet-stream").file_extension(), "png");
    }
}
