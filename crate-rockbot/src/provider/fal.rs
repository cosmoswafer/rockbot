use tracing::{debug, info, warn};
use crate::config::ProviderConfig;
use crate::error::{Result, RockBotError};
#[allow(unused_imports)]
use crate::types::{ImageGenParams, ImageSizeValue};
use crate::validated::NonEmptyString;

struct SubmittedRequest {
    request_id: NonEmptyString,
    status_url: NonEmptyString,
    response_url: NonEmptyString,
}

fn build_request_body(params: &ImageGenParams, model_id: &str) -> serde_json::Value {
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

    let is_seedream5 = model_id.contains("seedream/v5");

    if let Some(enable_safety) = params.enable_safety_checker {
        if is_seedream5 {
            body.insert("enable_safety_checker".into(), serde_json::json!(enable_safety));
        }
    }

    serde_json::Value::Object(body)
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
            http_client: super::default_http_client(),
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
        let model_id = params.model_id.as_deref().unwrap_or(&self.model_id);
        let body = build_request_body(params, model_id);
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
            let detail = resp_body
                .get("detail")
                .and_then(|d| d.as_str())
                .unwrap_or("Unknown error");
            warn!(
                "fal.ai submit failed (HTTP {}): model={} detail={}",
                status.as_u16(), model_id, detail
            );
            return Err(RockBotError::Provider(format!(
                "fal.ai submit failed: {}",
                detail
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

        debug!("fal.ai request submitted: request_id={} status_url={}", request_id, status_url);
        Ok(SubmittedRequest {
            request_id: NonEmptyString::try_new(request_id)
                .map_err(|e| RockBotError::Provider(format!("fal.ai response: {}", e)))?,
            status_url: NonEmptyString::try_new(status_url)
                .map_err(|e| RockBotError::Provider(format!("fal.ai response: {}", e)))?,
            response_url: NonEmptyString::try_new(response_url)
                .map_err(|e| RockBotError::Provider(format!("fal.ai response: {}", e)))?,
        })
    }

    async fn poll_status(&self, req: &SubmittedRequest) -> Result<String> {
        let max_attempts: u32 = 300;
        let delay_ms: u64 = 2000;
        let poll_start = std::time::Instant::now();

        for attempt in 0..max_attempts {
            let response = self
                .http_client
                .get(req.status_url.as_str())
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
                    http_status.as_u16(), req.request_id.as_str(), detail
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

            if attempt % 5 == 0 {
                debug!(
                    "fal.ai poll progress: request_id={} attempt={}/{} status={}",
                    req.request_id.as_str(), attempt, max_attempts, status
                );
            }

            match status {
                "COMPLETED" => {
                    info!(
                        "fal.ai request completed: request_id={} attempts={} elapsed_ms={}",
                        req.request_id.as_str(),
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
                    warn!("fal.ai request failed: request_id={} error={}", req.request_id.as_str(), error);
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

        warn!("fal.ai request timed out: request_id={}", req.request_id.as_str());
        Err(RockBotError::Provider("fal.ai request timed out".into()))
    }

    async fn fetch_result(&self, req: &SubmittedRequest) -> Result<String> {
        debug!("fal.ai fetch_result: request_id={} url={}", req.request_id.as_str(), req.response_url.as_str());
        let response = self
            .http_client
                .get(req.response_url.as_str())
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
                "fal.ai fetch_result failed (HTTP {}): request_id={} detail={}",
                http_status.as_u16(), req.request_id.as_str(), detail
            );
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
            Some(url) => {
                debug!("fal.ai fetch_result: got image url={}", url);
                Ok(url.to_string())
            }
            None => {
                warn!("fal.ai fetch_result: missing image URL in response body={:?}", body);
                Err(RockBotError::Provider(
                    "fal.ai result missing image URL".into(),
                ))
            }
        }
    }

    pub async fn generate_image_url(&self, params: &ImageGenParams) -> Result<String> {
        params.validate_dimensions()?;
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
        debug!("fal.ai upload init: filename={} storage_url={}", filename, self.storage_url);
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
            let detail = body.get("detail").and_then(|d| d.as_str()).unwrap_or("Unknown error");
            warn!("fal.ai upload init failed (HTTP {}): detail={}", status.as_u16(), detail);
            return Err(RockBotError::Provider(format!(
                "fal.ai upload init failed: {}",
                detail
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
        debug!("fal.ai upload init ok: file_url={}", file_url);

        // Step 2: PUT the file binary
        debug!("fal.ai upload PUT: sending {}B to presigned URL", data.len());
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
            let detail = put_body.get("detail").and_then(|d| d.as_str()).unwrap_or("Unknown error");
            warn!("fal.ai upload PUT failed (HTTP {}): detail={}", put_status.as_u16(), detail);
            return Err(RockBotError::Provider(format!(
                "fal.ai upload PUT failed (HTTP {}): {}",
                put_status, detail
            )));
        }

        debug!("fal.ai upload PUT ok: file_url={}", file_url);
        Ok(file_url.to_string())
    }
}

#[async_trait::async_trait]
impl crate::provider::ImageProvider for FalAiProvider {
    async fn generate_image(&self, params: &ImageGenParams) -> Result<Vec<u8>> {
        let url = self.generate_image_url(params).await?;
        let response = self
            .http_client
            .get(&url)
            .timeout(std::time::Duration::from_secs(600))
            .send()
            .await
            .map_err(|e| RockBotError::Provider(format!("Failed to download generated image: {e}")))?;
        let status = response.status();
        if !status.is_success() {
            return Err(RockBotError::Provider(format!(
                "Failed to download generated image: HTTP {}",
                status
            )));
        }
        response
            .bytes()
            .await
            .map(|b| b.to_vec())
            .map_err(|e| RockBotError::Provider(format!("Failed to read image bytes: {e}")))
    }

    async fn upload_file(&self, data: &[u8], content_type: &str) -> Result<String> {
        FalAiProvider::upload_file(self, data, content_type).await
    }

    fn provider_name(&self) -> &str {
        FalAiProvider::provider_name(self)
    }

    fn model_id(&self) -> &str {
        FalAiProvider::model_id(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProviderConfig;
    use crate::validated::{ConfigUrl, ProviderName};
    use std::collections::HashMap;

    fn make_config(api_key: &str) -> ProviderConfig {
        ProviderConfig {
            name: ProviderName::try_new("fal".to_string()).unwrap(),
            api_key: api_key.into(),
            base_url: ConfigUrl::try_new("https://queue.fal.run".to_string()).unwrap(),
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
        assert!(params.enable_safety_checker.is_none());
    }

    #[test]
    fn test_image_gen_params_with_all_fields() {
        let params = ImageGenParams {
            prompt: "test".into(),
            quality: Some("medium".into()),
            image_size: Some(ImageSizeValue::Preset("landscape_16_9".into())),
            size_tier: None,
            output_format: Some("png".into()),
            num_images: Some(2),
            model_id: Some("openai/gpt-image-2".into()),
            image_urls: Some(vec!["https://example.com/img.png".into()]),
            enable_safety_checker: None,
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
            size_tier: None,
            output_format: None,
            num_images: None,
            model_id: None,
            image_urls: None,
            enable_safety_checker: None,
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
            size_tier: None,
            output_format: None,
            num_images: None,
            model_id: None,
            image_urls: None,
            enable_safety_checker: None,
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
            size_tier: None,
            output_format: None, num_images: None, model_id: None, image_urls: None, enable_safety_checker: None,
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
            size_tier: None,
            output_format: None, num_images: None, model_id: None, image_urls: None, enable_safety_checker: None,
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
            size_tier: None,
            output_format: None, num_images: None, model_id: None, image_urls: None, enable_safety_checker: None,
        };
        let r = p.resolve_image_size().unwrap();
        assert_eq!(r, serde_json::json!("auto"));
    }

    #[test]
    fn test_resolve_image_size_aspect_ratio_string() {
        let p = ImageGenParams {
            prompt: "t".into(), quality: None,
            image_size: Some(ImageSizeValue::Preset("16:9".into())),
            size_tier: None,
            output_format: None, num_images: None, model_id: None, image_urls: None, enable_safety_checker: None,
        };
        let r = p.resolve_image_size().unwrap();
        assert_eq!(r["width"], 3840);
        assert_eq!(r["height"], 2160);
    }

    #[test]
    fn test_resolve_image_size_aspect_ratio_portrait() {
        let p = ImageGenParams {
            prompt: "t".into(), quality: None,
            image_size: Some(ImageSizeValue::Preset("2:3".into())),
            size_tier: None,
            output_format: None, num_images: None, model_id: None, image_urls: None, enable_safety_checker: None,
        };
        let r = p.resolve_image_size().unwrap();
        assert_eq!(r["width"], 2336);
        assert_eq!(r["height"], 3504);
    }

    #[test]
    fn test_resolve_image_size_aspect_ratio_1_1() {
        let p = ImageGenParams {
            prompt: "t".into(), quality: None,
            image_size: Some(ImageSizeValue::Preset("1:1".into())),
            size_tier: None,
            output_format: None, num_images: None, model_id: None, image_urls: None, enable_safety_checker: None,
        };
        let r = p.resolve_image_size().unwrap();
        assert_eq!(r["width"], 2880);
        assert_eq!(r["height"], 2880);
    }

    #[test]
    fn test_resolve_image_size_portrait_16_9() {
        let p = ImageGenParams {
            prompt: "t".into(), quality: None,
            image_size: Some(ImageSizeValue::Preset("portrait_16_9".into())),
            size_tier: None,
            output_format: None, num_images: None, model_id: None, image_urls: None, enable_safety_checker: None,
        };
        let r = p.resolve_image_size().unwrap();
        assert_eq!(r["width"], 2160);
        assert_eq!(r["height"], 3840);
    }

    #[test]
    fn test_resolve_image_size_9_16() {
        let p = ImageGenParams {
            prompt: "t".into(), quality: None,
            image_size: Some(ImageSizeValue::Preset("9:16".into())),
            size_tier: None,
            output_format: None, num_images: None, model_id: None, image_urls: None, enable_safety_checker: None,
        };
        let r = p.resolve_image_size().unwrap();
        assert_eq!(r["width"], 2160);
        assert_eq!(r["height"], 3840);
    }

    #[test]
    fn test_resolve_image_size_portrait_4_3() {
        let p = ImageGenParams {
            prompt: "t".into(), quality: None,
            image_size: Some(ImageSizeValue::Preset("portrait_4_3".into())),
            size_tier: None,
            output_format: None, num_images: None, model_id: None, image_urls: None, enable_safety_checker: None,
        };
        let r = p.resolve_image_size().unwrap();
        assert_eq!(r["width"], 2480);
        assert_eq!(r["height"], 3312);
    }

    #[test]
    fn test_resolve_image_size_3_4() {
        let p = ImageGenParams {
            prompt: "t".into(), quality: None,
            image_size: Some(ImageSizeValue::Preset("3:4".into())),
            size_tier: None,
            output_format: None, num_images: None, model_id: None, image_urls: None, enable_safety_checker: None,
        };
        let r = p.resolve_image_size().unwrap();
        assert_eq!(r["width"], 2480);
        assert_eq!(r["height"], 3312);
    }

    #[test]
    fn test_resolve_image_size_landscape_3_2() {
        let p = ImageGenParams {
            prompt: "t".into(), quality: None,
            image_size: Some(ImageSizeValue::Preset("landscape_3_2".into())),
            size_tier: None,
            output_format: None, num_images: None, model_id: None, image_urls: None, enable_safety_checker: None,
        };
        let r = p.resolve_image_size().unwrap();
        assert_eq!(r["width"], 3504);
        assert_eq!(r["height"], 2336);
    }

    #[test]
    fn test_resolve_image_size_3_2() {
        let p = ImageGenParams {
            prompt: "t".into(), quality: None,
            image_size: Some(ImageSizeValue::Preset("3:2".into())),
            size_tier: None,
            output_format: None, num_images: None, model_id: None, image_urls: None, enable_safety_checker: None,
        };
        let r = p.resolve_image_size().unwrap();
        assert_eq!(r["width"], 3504);
        assert_eq!(r["height"], 2336);
    }

    #[test]
    fn test_resolve_image_size_square() {
        let p = ImageGenParams {
            prompt: "t".into(), quality: None,
            image_size: Some(ImageSizeValue::Preset("square".into())),
            size_tier: None,
            output_format: None, num_images: None, model_id: None, image_urls: None, enable_safety_checker: None,
        };
        let r = p.resolve_image_size().unwrap();
        assert_eq!(r["width"], 512);
        assert_eq!(r["height"], 512);
    }

    #[test]
    fn test_build_request_body_seedream5_sends_safety_checker() {
        let mut params = ImageGenParams::new("a cat");
        params.enable_safety_checker = Some(false);
        params.output_format = Some("png".into());

        let body = build_request_body(&params, "fal-ai/bytedance/seedream/v5/pro/text-to-image");
        assert_eq!(body["prompt"], "a cat");
        assert_eq!(body["output_format"], "png");
        assert_eq!(body["enable_safety_checker"], false);
    }

    #[test]
    fn test_build_request_body_seedream5_sends_safety_checker_true() {
        let mut params = ImageGenParams::new("a cat");
        params.enable_safety_checker = Some(true);
        params.output_format = Some("png".into());

        let body = build_request_body(&params, "fal-ai/bytedance/seedream/v5/pro/text-to-image");
        assert_eq!(body["enable_safety_checker"], true);
    }

    #[test]
    fn test_build_request_body_non_seedream5_omits_safety_checker() {
        let mut params = ImageGenParams::new("a cat");
        params.enable_safety_checker = Some(false);
        params.output_format = Some("png".into());

        let body = build_request_body(&params, "fal-ai/flux/schnell");
        assert_eq!(body["prompt"], "a cat");
        assert_eq!(body["output_format"], "png");
        assert!(body.get("enable_safety_checker").is_none(),
            "enable_safety_checker should NOT be sent for non-seedream5 models");
    }

    #[test]
    fn test_build_request_body_seedream5_edit_includes_safety_checker() {
        let mut params = ImageGenParams::new("edit this");
        params.enable_safety_checker = Some(false);

        let body = build_request_body(&params, "fal-ai/bytedance/seedream/v5/pro/edit");
        assert_eq!(body["enable_safety_checker"], false,
            "enable_safety_checker is also sent for seedream5_edit (same guard, API accepts it)");
    }

    #[test]
    fn test_build_request_body_safety_checker_none_no_field() {
        let params = ImageGenParams::new("a cat");

        let body = build_request_body(&params, "fal-ai/bytedance/seedream/v5/pro/text-to-image");
        assert!(body.get("enable_safety_checker").is_none(),
            "enable_safety_checker should not be present when ImageGenParams has None");
    }
}
