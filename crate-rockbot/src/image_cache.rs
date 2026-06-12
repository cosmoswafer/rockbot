use std::collections::HashMap;
use std::sync::Mutex;

pub struct GeneratedImage {
    pub webdav_path: String,
    pub data_uri: String,
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

pub fn image_markdown(description: &str, data_uri: &str) -> String {
    format!("![{}]({})", description, data_uri)
}
