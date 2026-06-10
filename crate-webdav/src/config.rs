use serde::Deserialize;

use crate::client::WebDavClient;
use crate::error::{Result, WebDavError};

#[derive(Debug, Clone, Deserialize)]
pub struct WebDavConfig {
    pub url: String,
    pub username: String,
    pub password: String,
    pub root: String,
}

impl WebDavConfig {
    pub fn from_file(path: &str) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let table: toml::Table = toml::from_str(&content)?;
        let webdav = table.get("webdav").ok_or_else(|| {
            WebDavError::ConfigMissing("missing [webdav] section in config file".into())
        })?;
        let config: Self = webdav.clone().try_into()?;
        Ok(config)
    }

    pub fn from_toml(content: &str) -> Result<Self> {
        let config: Self = toml::from_str(content)?;
        Ok(config)
    }

    fn base_url(&self) -> String {
        let url = self.url.trim_end_matches('/');
        let root = self.root.trim_matches('/');
        format!("{url}/{root}")
    }

    pub fn into_client(self) -> Result<WebDavClient> {
        WebDavClient::new(self.base_url(), &self.username, &self.password)
    }

    pub fn create_client(&self) -> Result<WebDavClient> {
        WebDavClient::new(self.base_url(), &self.username, &self.password)
    }
}
