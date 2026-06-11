use tracing::{debug, warn};
use crate::config::ProviderConfig;
use crate::error::{Result, RockBotError};

#[derive(Debug, Clone)]
pub enum ImageSizeValue {
    Preset(String),
    Custom { width: u32, height: u32 },
}

#[derive(Debug, Clone)]
pub struct ImageGenParams {
    pub prompt: String,
    pub quality: Option<String>,
    pub image_size: Option<ImageSizeValue>,
    pub output_format: Option<String>,
    pub num_images: Option<u32>,
    pub model_id: Option<String>,
    pub image_urls: Option<Vec<String>>,
}

impl ImageGenParams {
    pub fn new(prompt: impl Into<String>) -> Self {
        Self {
            prompt: prompt.into(),
            quality: None,
            image_size: None,
            output_format: None,
            num_images: None,
            model_id: None,
            image_urls: None,
        }
    }

    pub fn resolve_image_size(&self) -> Option<serde_json::Value> {
        match &self.image_size {
            Some(ImageSizeValue::Preset(name)) => Self::lookup_preset(name)
                .map(|(w, h)| serde_json::json!({ "width": w, "height": h }))
                .or_else(|| Some(serde_json::json!(name))),
            Some(ImageSizeValue::Custom { width, height }) => {
                Some(serde_json::json!({ "width": width, "height": height }))
            }
            None => None,
        }
    }

    fn lookup_preset(name: &str) -> Option<(u32, u32)> {
        match name {
            "square_hd" => Some((2880, 2880)),
            "landscape_16_9" => Some((3840, 2160)),
            "portrait_16_9" => Some((2160, 3840)),
            "landscape_4_3" => Some((3312, 2480)),
            "portrait_4_3" => Some((2480, 3312)),
            "landscape_3_2" => Some((3504, 2336)),
            "portrait_2_3" => Some((2336, 3504)),
            "square" => Some((512, 512)),
            _ => None,
        }
    }
}

struct SubmittedRequest {
    request_id: String,
    status_url: String,
    response_url: String,
}

pub struct FalAiProvider {
    api_key: String,
    base_url: String,
    storage_url: String,
    model_id: String,
    http_client: reqwest::Client,
}

impl std::fmt::Debug for FalAiProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FalAiProvider")
            .field("base_url", &self.base_url)
            .field("model_id", &self.model_id)
            .finish()
    }
}

impl FalAiProvider {
    pub fn new(config: &ProviderConfig, model_id: impl Into<String>) -> Result<Self> {
        config.validate_api_key()?;

        let storage_url = config
            .basecf_url
            .as_deref()
            .unwrap_or("https://rest.fal.ai")
            .trim_end_matches('/')
            .to_string();

        Ok(Self {
            api_key: config.api_key.clone(),
            base_url: config.base_url.trim_end_matches('/').to_string(),
            storage_url,
            model_id: model_id.into(),
            http_client: reqwest::Client::new(),
        })
    }

    pub fn with_client(
        config: &ProviderConfig,
        model_id: impl Into<String>,
        client: reqwest::Client,
    ) -> Result<Self> {
        config.validate_api_key()?;

        let storage_url = config
            .basecf_url
            .as_deref()
            .unwrap_or("https://rest.fal.ai")
            .trim_end_matches('/')
            .to_string();

        Ok(Self {
            api_key: config.api_key.clone(),
            base_url: config.base_url.trim_end_matches('/').to_string(),
            storage_url,
            model_id: model_id.into(),
            http_client: client,
        })
    }

    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    pub fn provider_name(&self) -> &str {
        "fal"
    }

    async fn submit_request(&self, params: &ImageGenParams) -> Result<SubmittedRequest> {
        let mut body = serde_json::Map::new();
        body.insert("prompt".into(), serde_json::json!(params.prompt));

        if let Some(quality) = &params.quality {
            body.insert("quality".into(), serde_json::json!(quality));
        }
        if let Some(output_format) = &params.output_format {
            body.insert("output_format".into(), serde_json::json!(output_format));
        }
        if let Some(num_images) = params.num_images {
            body.insert("num_images".into(), serde_json::json!(num_images));
        }
        if let Some(image_size_val) = params.resolve_image_size() {
            body.insert("image_size".into(), image_size_val);
        }
        if let Some(ref image_urls) = params.image_urls {
            if !image_urls.is_empty() {
                body.insert("image_urls".into(), serde_json::json!(image_urls));
            }
        }

        let model_id = params.model_id.as_deref().unwrap_or(&self.model_id);
        let url = format!("{}/{}", self.base_url, model_id);

        debug!(
            "fal.ai submit: model={} prompt_len={} img2img={}",
            model_id,
            params.prompt.len(),
            params.image_urls.as_ref().map(|u| u.len()).unwrap_or(0),
        );

        let response = self
            .http_client
            .post(&url)
            .header("Authorization", format!("Key {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        let status = response.status();
        let resp_body: serde_json::Value = response.json().await?;

        if !status.is_success() {
            return Err(RockBotError::Provider(format!(
                "fal.ai submit failed: {}",
                resp_body
                    .get("detail")
                    .and_then(|d| d.as_str())
                    .unwrap_or("Unknown error")
            )));
        }

        let request_id = resp_body
            .get("request_id")
            .and_then(|r| r.as_str())
            .map(String::from)
            .ok_or_else(|| RockBotError::Provider("fal.ai response missing request_id".into()))?;

        let status_url = resp_body
            .get("status_url")
            .and_then(|u| u.as_str())
            .map(String::from)
            .ok_or_else(|| RockBotError::Provider("fal.ai response missing status_url".into()))?;

        let response_url = resp_body
            .get("response_url")
            .and_then(|u| u.as_str())
            .map(String::from)
            .ok_or_else(|| RockBotError::Provider("fal.ai response missing response_url".into()))?;

        debug!("fal.ai request submitted: request_id={}", request_id);
        Ok(SubmittedRequest {
            request_id,
            status_url,
            response_url,
        })
    }

    async fn poll_status(&self, req: &SubmittedRequest) -> Result<String> {
        let max_attempts: u32 = 300;
        let delay_ms: u64 = 2000;
        let poll_start = std::time::Instant::now();

        for attempt in 0..max_attempts {
            let response = self
                .http_client
                .get(&req.status_url)
                .header("Authorization", format!("Key {}", self.api_key))
                .send()
                .await?;

            let http_status = response.status();
            let body: serde_json::Value = response.json().await?;

            if !http_status.is_success() {
                let detail = body
                    .get("detail")
                    .and_then(|d| d.as_str())
                    .unwrap_or("Unknown error");
                warn!(
                    "fal.ai poll HTTP {}: request_id={} detail={}",
                    http_status.as_u16(), req.request_id, detail
                );
                return Err(RockBotError::Provider(format!(
                    "fal.ai poll failed (HTTP {}): {}",
                    http_status.as_u16(), detail
                )));
            }

            let status = body
                .get("status")
                .and_then(|s| s.as_str())
                .unwrap_or("unknown");

            if attempt > 0 && attempt % 10 == 0 {
                debug!(
                    "fal.ai poll progress: request_id={} attempt={}/{} status={}",
                    req.request_id, attempt, max_attempts, status
                );
            }

            match status {
                "COMPLETED" => {
                    debug!(
                        "fal.ai request completed: request_id={} attempts={} elapsed_ms={}",
                        req.request_id,
                        attempt + 1,
                        poll_start.elapsed().as_millis(),
                    );
                    return self.fetch_result(req).await;
                }
                "FAILED" => {
                    let error = body
                        .get("error")
                        .and_then(|e| e.as_str())
                        .unwrap_or("Unknown error");
                    warn!("fal.ai request failed: request_id={} error={}", req.request_id, error);
                    return Err(RockBotError::Provider(format!(
                        "fal.ai request failed: {}",
                        error
                    )));
                }
                _ => {
                    tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                }
            }
        }

        warn!("fal.ai request timed out: request_id={}", req.request_id);
        Err(RockBotError::Provider("fal.ai request timed out".into()))
    }

    async fn fetch_result(&self, req: &SubmittedRequest) -> Result<String> {
        let response = self
            .http_client
            .get(&req.response_url)
            .header("Authorization", format!("Key {}", self.api_key))
            .send()
            .await?;

        let http_status = response.status();
        let body: serde_json::Value = response.json().await?;

        if !http_status.is_success() {
            let detail = body
                .get("detail")
                .and_then(|d| d.as_str())
                .unwrap_or("Unknown error");
            return Err(RockBotError::Provider(format!(
                "fal.ai fetch result failed (HTTP {}): {}",
                http_status.as_u16(), detail
            )));
        }

        let image_url = body
            .get("images")
            .and_then(|imgs| imgs.as_array())
            .and_then(|arr| arr.first())
            .and_then(|img| img.get("url"))
            .and_then(|u| u.as_str());

        match image_url {
            Some(url) => Ok(url.to_string()),
            None => Err(RockBotError::Provider(
                "fal.ai result missing image URL".into(),
            )),
        }
    }

    pub async fn generate_image(&self, params: &ImageGenParams) -> Result<String> {
        let req = self.submit_request(params).await?;
        self.poll_status(&req).await
    }

    pub async fn upload_file(&self, data: &[u8], content_type: &str) -> Result<String> {
        debug!("fal.ai upload: content_type={} size={}B", content_type, data.len());
        // Step 1: initiate upload
        let init_url = format!("{}/storage/upload/initiate?storage_type=fal-cdn-v3", self.storage_url);
        let ext = content_type.strip_prefix("image/").unwrap_or("png");
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let filename = format!("rockbot-{}.{}", ts, ext);
        let init_body = serde_json::json!({
            "content_type": content_type,
            "file_name": filename,
        });
        let response = self
            .http_client
            .post(&init_url)
            .header("Authorization", format!("Key {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&init_body)
            .send()
            .await?;

        let status = response.status();
        let body: serde_json::Value = response.json().await?;
        if !status.is_success() {
            return Err(RockBotError::Provider(format!(
                "fal.ai upload init failed: {}",
                body.get("detail").and_then(|d| d.as_str()).unwrap_or("Unknown error")
            )));
        }
        let file_url = body
            .get("file_url")
            .and_then(|u| u.as_str())
            .ok_or_else(|| RockBotError::Provider("fal.ai upload init missing file_url".into()))?;
        let upload_url = body
            .get("upload_url")
            .and_then(|u| u.as_str())
            .ok_or_else(|| RockBotError::Provider("fal.ai upload init missing upload_url".into()))?;

        // Step 2: PUT the file binary
        let put_response = self
            .http_client
            .put(upload_url)
            .header("Content-Type", content_type)
            .body(data.to_vec())
            .send()
            .await?;

        let put_status = put_response.status();
        if !put_status.is_success() {
            let put_body: serde_json::Value = put_response.json().await?;
            return Err(RockBotError::Provider(format!(
                "fal.ai upload PUT failed (HTTP {}): {}",
                put_status,
                put_body.get("detail").and_then(|d| d.as_str()).unwrap_or("Unknown error")
            )));
        }

        Ok(file_url.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProviderConfig;
    use std::collections::HashMap;

    fn make_config(api_key: &str) -> ProviderConfig {
        ProviderConfig {
            name: "fal".into(),
            api_key: api_key.into(),
            base_url: "https://queue.fal.run".into(),
            basecf_url: None,
            chat_path: None,
            draw_path: None,
            models: HashMap::new(),
        }
    }

    #[test]
    fn test_new_success() {
        let config = make_config("fal_test_key");
        let provider = FalAiProvider::new(&config, "fal-ai/flux/schnell");
        assert!(provider.is_ok());
    }

    #[test]
    fn test_new_missing_api_key() {
        let config = make_config("EDITME");
        let provider = FalAiProvider::new(&config, "fal-ai/flux/schnell");
        assert!(provider.is_err());
    }

    #[test]
    fn test_new_empty_api_key() {
        let config = make_config("");
        let provider = FalAiProvider::new(&config, "fal-ai/flux/schnell");
        assert!(provider.is_err());
    }

    #[test]
    fn test_provider_names() {
        let config = make_config("fal_test");
        let provider = FalAiProvider::new(&config, "fal-ai/flux/schnell").unwrap();
        assert_eq!(provider.provider_name(), "fal");
        assert_eq!(provider.model_id(), "fal-ai/flux/schnell");
    }

    #[test]
    fn test_image_gen_params_defaults() {
        let params = ImageGenParams::new("a cat");
        assert_eq!(params.prompt, "a cat");
        assert!(params.quality.is_none());
        assert!(params.image_size.is_none());
        assert!(params.output_format.is_none());
        assert!(params.num_images.is_none());
        assert!(params.model_id.is_none());
    }

    #[test]
    fn test_image_gen_params_with_all_fields() {
        let params = ImageGenParams {
            prompt: "test".into(),
            quality: Some("medium".into()),
            image_size: Some(ImageSizeValue::Preset("landscape_16_9".into())),
            output_format: Some("png".into()),
            num_images: Some(2),
            model_id: Some("openai/gpt-image-2".into()),
            image_urls: Some(vec!["https://example.com/img.png".into()]),
        };
        assert_eq!(params.quality.as_deref(), Some("medium"));
        assert_eq!(params.num_images, Some(2));
    }

    #[test]
    fn test_resolve_image_size_preset() {
        let params = ImageGenParams {
            prompt: "test".into(),
            quality: None,
            image_size: Some(ImageSizeValue::Preset("landscape_16_9".into())),
            output_format: None,
            num_images: None,
            model_id: None,
            image_urls: None,
        };
        let resolved = params.resolve_image_size().unwrap();
        assert_eq!(resolved["width"], 3840);
        assert_eq!(resolved["height"], 2160);
    }

    #[test]
    fn test_resolve_image_size_custom() {
        let params = ImageGenParams {
            prompt: "test".into(),
            quality: None,
            image_size: Some(ImageSizeValue::Custom { width: 1920, height: 1080 }),
            output_format: None,
            num_images: None,
            model_id: None,
            image_urls: None,
        };
        let resolved = params.resolve_image_size().unwrap();
        assert_eq!(resolved["width"], 1920);
        assert_eq!(resolved["height"], 1080);
    }

    #[test]
    fn test_resolve_image_size_none() {
        let params = ImageGenParams::new("test");
        assert!(params.resolve_image_size().is_none());
    }

    #[test]
    fn test_resolve_image_size_square_hd() {
        let p = ImageGenParams {
            prompt: "t".into(), quality: None,
            image_size: Some(ImageSizeValue::Preset("square_hd".into())),
            output_format: None, num_images: None, model_id: None, image_urls: None,
        };
        let r = p.resolve_image_size().unwrap();
        assert_eq!(r["width"], 2880);
        assert_eq!(r["height"], 2880);
    }

    #[test]
    fn test_resolve_image_size_landscape_4_3() {
        let p = ImageGenParams {
            prompt: "t".into(), quality: None,
            image_size: Some(ImageSizeValue::Preset("landscape_4_3".into())),
            output_format: None, num_images: None, model_id: None, image_urls: None,
        };
        let r = p.resolve_image_size().unwrap();
        assert_eq!(r["width"], 3312);
        assert_eq!(r["height"], 2480);
    }

    #[test]
    fn test_resolve_image_size_unknown_preset_passes_through() {
        let p = ImageGenParams {
            prompt: "t".into(), quality: None,
            image_size: Some(ImageSizeValue::Preset("auto".into())),
            output_format: None, num_images: None, model_id: None, image_urls: None,
        };
        let r = p.resolve_image_size().unwrap();
        assert_eq!(r, serde_json::json!("auto"));
    }
}
