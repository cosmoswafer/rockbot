use serde::Deserialize;

use crate::client::WebDavClient;
use crate::error::{Result, WebDavError};

#[derive(Debug, Clone, Deserialize)]
pub struct WebDavConfig {
    pub url: String,
    pub username: String,
    pub password: String,
    pub root: String,
    #[serde(default)]
    pub calendar_name: Option<String>,
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

    fn server_origin(&self) -> String {
        let url = self.url.trim_end_matches('/');
        // Strip the WebDAV path suffix (/remote.php/dav/files/...) to get the server base
        if let Some(pos) = url.find("/remote.php/dav/") {
            url[..pos].to_string()
        } else {
            url.to_string()
        }
    }

    pub fn caldav_base_url(&self, calendar_name: &str) -> String {
        let origin = self.server_origin();
        format!(
            "{}/remote.php/dav/calendars/{}/{}/",
            origin, self.username, calendar_name
        )
    }

    pub fn into_client(self) -> Result<WebDavClient> {
        WebDavClient::new(self.base_url(), &self.username, &self.password)
    }

    pub fn create_client(&self) -> Result<WebDavClient> {
        WebDavClient::new(self.base_url(), &self.username, &self.password)
    }
}
