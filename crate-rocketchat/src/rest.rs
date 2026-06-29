use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_valid::Validate;
use tracing::{debug, warn};

use crate::config::RocketChatConfig;
use crate::error::{Result, RocketChatError};

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct RoomInfo {
    #[serde(rename = "_id")]
    #[validate(min_length = 1)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub fname: String,
    #[serde(default)]
    pub t: String,
}

#[derive(Debug, Clone, Deserialize, Validate)]
struct RoomsGetResponse {
    update: Vec<RoomInfo>,
    #[serde(default)]
    success: bool,
}

#[derive(Debug, Clone, Deserialize, Validate)]
struct RoomInfoResponse {
    #[validate]
    room: RoomInfo,
    success: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct SendMessageResponse {
    message: serde_json::Value,
    success: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct GetMessageResponse {
    message: serde_json::Value,
    success: bool,
}

pub struct RestApiClient {
    host: String,
    use_tls: bool,
    user_id: String,
    auth_token: String,
    http: reqwest::Client,
    room_name_cache: HashMap<String, String>,
}

impl RestApiClient {
    pub fn new(config: &RocketChatConfig, user_id: String, auth_token: String) -> Self {
        assert!(!user_id.is_empty(), "RestApiClient: user_id must not be empty");
        assert!(!auth_token.is_empty(), "RestApiClient: auth_token must not be empty");
        let host = config.host().to_string();
        let use_tls = config.server.use_tls;
        Self {
            host,
            use_tls,
            user_id,
            auth_token,
            http: reqwest::Client::new(),
            room_name_cache: HashMap::new(),
        }
    }

    fn api_url(&self, path: &str) -> String {
        let protocol = if self.use_tls { "https" } else { "http" };
        format!("{}://{}/api/v1/{}", protocol, self.host, path)
    }

    pub fn headers(&self) -> reqwest::header::HeaderMap {
        use reqwest::header::{HeaderMap, HeaderValue};
        let mut headers = HeaderMap::new();
        headers.insert("X-Auth-Token", HeaderValue::from_str(&self.auth_token).unwrap());
        headers.insert("X-User-Id", HeaderValue::from_str(&self.user_id).unwrap());
        headers.insert("Content-Type", HeaderValue::from_static("application/json"));
        headers
    }

    pub async fn get_rooms(&mut self) -> Result<Vec<RoomInfo>> {
        let url = self.api_url("rooms.get");
        debug!("REST GET {}", url);

        let resp = self
            .http
            .get(&url)
            .headers(self.headers())
            .send()
            .await
            .map_err(|e| RocketChatError::Protocol(format!("HTTP request failed: {e}")))?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(RocketChatError::Protocol(format!(
                "rooms.get returned {status}: {body}"
            )));
        }

        let data: RoomsGetResponse = resp.json().await.map_err(|e| {
            RocketChatError::Protocol(format!("Failed to parse rooms.get response: {e}"))
        })?;

        if !data.success {
            warn!("rooms.get returned success=false, treating as empty");
            return Ok(vec![]);
        }

        for room in &data.update {
            if !room.fname.is_empty() {
                self.room_name_cache
                    .insert(room.id.clone(), room.fname.clone());
            }
        }

        debug!("REST rooms.get: {} rooms", data.update.len());
        Ok(data.update)
    }

    pub async fn get_room_info(&mut self, room_id: &str) -> Result<Option<RoomInfo>> {
        let query = format!("rooms.info?roomId={room_id}");
        let url = self.api_url(&query);
        debug!("REST GET {}", url);

        let resp = self
            .http
            .get(&url)
            .headers(self.headers())
            .send()
            .await
            .map_err(|e| RocketChatError::Protocol(format!("HTTP request failed: {e}")))?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            warn!("rooms.info returned {status}: {body}");
            return Ok(None);
        }

        let data: RoomInfoResponse = resp.json().await.map_err(|e| {
            RocketChatError::Protocol(format!("Failed to parse rooms.info response: {e}"))
        })?;

        if data.success && !data.room.fname.is_empty() {
            self.room_name_cache
                .insert(data.room.id.clone(), data.room.fname.clone());
        }

        Ok(if data.success { Some(data.room) } else { None })
    }

    pub async fn resolve_room_fname(&mut self, room_id: &str) -> Option<String> {
        if let Some(fname) = self.room_name_cache.get(room_id) {
            return Some(fname.clone());
        }

        if let Ok(Some(room)) = self.get_room_info(room_id).await {
            if !room.fname.is_empty() {
                return Some(room.fname);
            }
        }

        if let Ok(rooms) = self.get_rooms().await {
            for room in &rooms {
                if room.id == room_id && !room.fname.is_empty() {
                    return Some(room.fname.clone());
                }
            }
        }

        None
    }

    pub async fn send_message(
        &self,
        room_id: &str,
        text: &str,
        alias: Option<&str>,
    ) -> Result<String> {
        let url = self.api_url("chat.sendMessage");
        let mut body = serde_json::json!({
            "message": {
                "rid": room_id,
                "msg": text
            }
        });

        if let Some(a) = alias {
            body["message"]["alias"] = serde_json::Value::String(a.to_string());
        }

        debug!("REST POST {} <- alias={:?}", url, alias);

        let resp = self
            .http
            .post(&url)
            .headers(self.headers())
            .json(&body)
            .send()
            .await
            .map_err(|e| RocketChatError::Protocol(format!("HTTP request failed: {e}")))?;

        let status = resp.status();
        if !status.is_success() {
            let resp_body = resp.text().await.unwrap_or_default();
            return Err(RocketChatError::Protocol(format!(
                "chat.sendMessage returned {status}: {resp_body}"
            )));
        }

        let data: SendMessageResponse = resp.json().await.map_err(|e| {
            RocketChatError::Protocol(format!("Failed to parse chat.sendMessage response: {e}"))
        })?;

        if data.success {
            let msg_id = data.message["_id"]
                .as_str()
                .unwrap_or("unknown")
                .to_string();
            Ok(msg_id)
        } else {
            Err(RocketChatError::Protocol(
                "chat.sendMessage returned success=false".into(),
            ))
        }
    }

    pub async fn get_message(&self, msg_id: &str) -> Result<Option<serde_json::Value>> {
        let url = self.api_url(&format!("chat.getMessage?msgId={msg_id}"));
        debug!("REST GET {}", url);

        let resp = self
            .http
            .get(&url)
            .headers(self.headers())
            .send()
            .await
            .map_err(|e| RocketChatError::Protocol(format!("HTTP request failed: {e}")))?;

        let status = resp.status();
        if !status.is_success() {
            return Ok(None);
        }

        let data: GetMessageResponse = resp.json().await.map_err(|e| {
            RocketChatError::Protocol(format!("Failed to parse chat.getMessage response: {e}"))
        })?;

        Ok(if data.success {
            Some(data.message)
        } else {
            None
        })
    }

    pub async fn set_avatar(&self, avatar_url: &str) -> Result<()> {
        let url = self.api_url("users.setAvatar");
        let body = serde_json::json!({
            "avatarUrl": avatar_url
        });
        debug!("REST POST {} <- avatarUrl={}", url, avatar_url);

        let resp = self
            .http
            .post(&url)
            .headers(self.headers())
            .json(&body)
            .send()
            .await
            .map_err(|e| RocketChatError::Protocol(format!("HTTP request failed: {e}")))?;

        let status = resp.status();
        if !status.is_success() {
            let resp_body = resp.text().await.unwrap_or_default();
            return Err(RocketChatError::Protocol(format!(
                "users.setAvatar returned {status}: {resp_body}"
            )));
        }

        Ok(())
    }

    pub async fn upload_file_to_room(
        &self,
        room_id: &str,
        file_name: &str,
        file_bytes: Vec<u8>,
        mime_type: &str,
    ) -> Result<String> {
        let url = self.api_url(&format!("rooms.upload/{}", room_id));
        debug!("REST POST {} (upload file: {})", url, file_name);

        let part = reqwest::multipart::Part::bytes(file_bytes)
            .file_name(file_name.to_string())
            .mime_str(mime_type)
            .map_err(|e| RocketChatError::Protocol(format!("MIME parse error: {e}")))?;

        let form = reqwest::multipart::Form::new()
            .part("file", part)
            .text("msg", format!("{} uploaded", file_name));

        let resp = self
            .http
            .post(&url)
            .headers({
                let mut headers = self.headers();
                headers.remove("Content-Type");
                headers
            })
            .multipart(form)
            .send()
            .await
            .map_err(|e| RocketChatError::Protocol(format!("rooms.upload HTTP error: {e}")))?;

        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();

        if !status.is_success() {
            return Err(RocketChatError::Protocol(format!(
                "rooms.upload returned {status}: {body}"
            )));
        }

        let data: serde_json::Value = serde_json::from_str(&body).map_err(|e| {
            RocketChatError::Protocol(format!("Failed to parse rooms.upload response: {e}"))
        })?;

        if data.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
            // Return the file attachment URL path (e.g. /file-upload/uuid/filename)
            let file_url = data
                .get("files")
                .and_then(|f| f.get(0))
                .and_then(|f| f.get("url"))
                .and_then(|u| u.as_str())
                .map(String::from)
                .unwrap_or_default();
            debug!("REST rooms.upload ok, file_url: {}", file_url);
            Ok(file_url)
        } else {
            Err(RocketChatError::Protocol(format!(
                "rooms.upload failed: {body}"
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ServerConfig;

    use crate::validated::{Password, ServerUrl, Username};

    fn test_config() -> RocketChatConfig {
        RocketChatConfig {
            server: ServerConfig {
                url: ServerUrl::try_new("chat.example.com".into()).unwrap(),
                username: Username::try_new("bot".into()).unwrap(),
                password: Password::try_new("pw".into()).unwrap(),
                use_tls: true,
            },
        }
    }

    #[test]
    fn test_rest_client_new() {
        let config = test_config();
        let client =
            RestApiClient::new(&config, "user123".to_string(), "token456".to_string());
        assert_eq!(client.host, "chat.example.com");
        assert!(client.use_tls);
        assert_eq!(client.user_id, "user123");
        assert_eq!(client.auth_token, "token456");
    }

    #[test]
    fn test_api_url_construction() {
        let config = test_config();
        let client =
            RestApiClient::new(&config, "user123".into(), "token456".into());
        assert_eq!(
            client.api_url("rooms.get"),
            "https://chat.example.com/api/v1/rooms.get"
        );
        assert_eq!(
            client.api_url("chat.sendMessage"),
            "https://chat.example.com/api/v1/chat.sendMessage"
        );
    }

    #[test]
    fn test_headers_contain_auth() {
        let config = test_config();
        let client =
            RestApiClient::new(&config, "user123".into(), "token456".into());
        let headers = client.headers();
        assert_eq!(headers.get("X-Auth-Token").unwrap(), "token456");
        assert_eq!(headers.get("X-User-Id").unwrap(), "user123");
    }

    #[test]
    fn test_room_info_deserialize() {
        let json = serde_json::json!({
            "room": {
                "_id": "room1",
                "name": "shit",
                "fname": "💩💩💩SHIT屎",
                "t": "p"
            },
            "success": true
        });
        let data: RoomInfoResponse = serde_json::from_value(json).unwrap();
        assert!(data.success);
        assert_eq!(data.room.id, "room1");
        assert_eq!(data.room.fname, "💩💩💩SHIT屎");
    }

    #[test]
    fn test_rooms_get_deserialize() {
        let json = serde_json::json!({
            "update": [
                {"_id": "room1", "name": "general", "fname": "", "t": "c"},
                {"_id": "room2", "name": "shit", "fname": "💩💩💩SHIT屎", "t": "p"}
            ],
            "success": true
        });
        let data: RoomsGetResponse = serde_json::from_value(json).unwrap();
        assert_eq!(data.update.len(), 2);
        assert_eq!(data.update[1].fname, "💩💩💩SHIT屎");
    }
}
