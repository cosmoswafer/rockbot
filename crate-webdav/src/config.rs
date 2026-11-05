use serde::Deserialize;

use crate::client::WebDavClient;
use crate::error::{Result, WebDavError};
use crate::validated::{DavPassword, DavRoot, DavUrl, DavUsername};

fn default_dav_path() -> String {
    "/remote.php/dav".to_string()
}

fn default_snapshot_prefix() -> String {
    ".snapshots".to_string()
}

fn default_stream_threshold_bytes() -> u64 {
    10 * 1024 * 1024
}

fn default_max_edit_bytes() -> u64 {
    32 * 1024 * 1024
}

fn default_chunk_size_bytes() -> usize {
    5 * 1024 * 1024
}

fn default_read_default_lines() -> usize {
    2000
}

fn default_read_max_lines() -> usize {
    2000
}

fn default_read_max_bytes() -> usize {
    65_536
}

fn default_read_max_line_chars() -> usize {
    2000
}

fn default_index_checkpoint_lines() -> u64 {
    1000
}

#[derive(Debug, Clone, Deserialize)]
pub struct WebDavConfig {
    pub url: DavUrl,
    pub username: DavUsername,
    pub password: DavPassword,
    pub root: DavRoot,
    #[serde(default = "default_dav_path")]
    pub dav_path: String,
    #[serde(default = "default_snapshot_prefix")]
    pub snapshot_prefix: String,
    /// Files at or above this size use streamed, windowed reads.
    #[serde(default = "default_stream_threshold_bytes")]
    pub stream_threshold_bytes: u64,
    /// Largest file the in-RAM edit path will load; above this, edits stream.
    #[serde(default = "default_max_edit_bytes")]
    pub max_edit_bytes: u64,
    /// Nextcloud chunked-upload chunk size (v2 requires >= 5 MiB).
    #[serde(default = "default_chunk_size_bytes")]
    pub chunk_size_bytes: usize,
    #[serde(default = "default_read_default_lines")]
    pub read_default_lines: usize,
    #[serde(default = "default_read_max_lines")]
    pub read_max_lines: usize,
    #[serde(default = "default_read_max_bytes")]
    pub read_max_bytes: usize,
    #[serde(default = "default_read_max_line_chars")]
    pub read_max_line_chars: usize,
    /// Record one line-index checkpoint every N lines.
    #[serde(default = "default_index_checkpoint_lines")]
    pub index_checkpoint_lines: u64,
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
        format!("{url}/{dav}/calendars/{}/{}/", self.username, calendar_name)
    }

    pub fn into_client(self) -> Result<WebDavClient> {
        WebDavClient::new(self.base_url(), &self.username, &self.password)
    }

    pub fn create_client(&self) -> Result<WebDavClient> {
        WebDavClient::new(self.base_url(), &self.username, &self.password)
    }
}
