use std::collections::HashMap;
use std::sync::Mutex;

use base64::Engine;

pub struct GeneratedImage {
    pub webdav_path: String,
    pub image_bytes: Vec<u8>,
    pub mime_type: String,
    pub share_url: Option<String>,
}

impl GeneratedImage {
    pub fn data_uri(&self) -> String {
        let b64 = base64::engine::general_purpose::STANDARD.encode(&self.image_bytes);
        format!("data:{};base64,{}", self.mime_type, b64)
    }

    pub fn file_extension(&self) -> &str {
        match self.mime_type.as_str() {
            "image/jpeg" | "image/jpg" => "jpg",
            "image/png" => "png",
            "image/webp" => "webp",
            _ => "png",
        }
    }
}

pub struct ImageCache {
    entries: Mutex<HashMap<String, GeneratedImage>>,
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

    pub fn is_empty(&self) -> bool {
        self.entries.lock().unwrap().is_empty()
    }
}

pub fn image_markdown(description: &str, url: &str) -> String {
    format!("![{}]({})", description, url)
}
