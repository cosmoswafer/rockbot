use serde::Deserialize;

use crate::client::WebDavClient;
use crate::error::{Result, WebDavError};
use crate::validated::{DavRoot, DavUrl};

fn default_dav_path() -> String {
    "/remote.php/dav".to_string()
}

#[derive(Debug, Clone, Deserialize)]
pub struct WebDavConfig {
    pub url: DavUrl,
    pub username: String,
    pub password: String,
    pub root: DavRoot,
    #[serde(default)]
    pub calendar_name: Option<String>,
    #[serde(default = "default_dav_path")]
    pub dav_path: String,
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
        let url: &str = &self.url;
        let url = url.trim_end_matches('/');
        let dav = self.dav_path.trim_matches('/');
        let root: &str = &self.root;
        let root = root.trim_matches('/');
        format!("{url}/{dav}/files/{}/{root}", self.username)
    }

    pub fn caldav_base_url(&self, calendar_name: &str) -> String {
        let url: &str = &self.url;
        let url = url.trim_end_matches('/');
        let dav = self.dav_path.trim_matches('/');
        format!(
            "{url}/{dav}/calendars/{}/{}/",
            self.username, calendar_name
        )
    }

    pub fn into_client(self) -> Result<WebDavClient> {
        WebDavClient::new(self.base_url(), &self.username, &self.password)
    }

    pub fn create_client(&self) -> Result<WebDavClient> {
        WebDavClient::new(self.base_url(), &self.username, &self.password)
    }
}
