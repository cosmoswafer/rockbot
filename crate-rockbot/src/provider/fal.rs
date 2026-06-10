use crate::config::ProviderConfig;
use crate::error::{Result, RockBotError};

pub struct FalAiProvider {
    api_key: String,
    base_url: String,
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

        Ok(Self {
            api_key: config.api_key.clone(),
            base_url: config.base_url.trim_end_matches('/').to_string(),
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

        Ok(Self {
            api_key: config.api_key.clone(),
            base_url: config.base_url.trim_end_matches('/').to_string(),
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

    async fn submit_request(&self, prompt: &str) -> Result<String> {
        let response = self
            .http_client
            .post(format!("{}/{}", self.base_url, self.model_id))
            .header("Authorization", format!("Key {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&serde_json::json!({ "prompt": prompt }))
            .send()
            .await?;

        let status = response.status();
        let body: serde_json::Value = response.json().await?;

        if !status.is_success() {
            return Err(RockBotError::Provider(format!(
                "fal.ai submit failed: {}",
                body.get("detail")
                    .and_then(|d| d.as_str())
                    .unwrap_or("Unknown error")
            )));
        }

        body.get("request_id")
            .and_then(|r| r.as_str())
            .map(String::from)
            .ok_or_else(|| RockBotError::Provider("fal.ai response missing request_id".into()))
    }

    async fn poll_status(&self, request_id: &str) -> Result<String> {
        let url = format!(
            "{}/{}/requests/{}/status",
            self.base_url, self.model_id, request_id
        );
        let max_attempts: u32 = 90;
        let delay_ms: u64 = 2000;

        for _ in 0..max_attempts {
            let response = self
                .http_client
                .get(&url)
                .header("Authorization", format!("Key {}", self.api_key))
                .send()
                .await?;

            let body: serde_json::Value = response.json().await?;

            let status = body
                .get("status")
                .and_then(|s| s.as_str())
                .unwrap_or("unknown");

            match status {
                "COMPLETED" => {
                    return self.fetch_result(request_id).await;
                }
                "FAILED" => {
                    let error = body
                        .get("error")
                        .and_then(|e| e.as_str())
                        .unwrap_or("Unknown error");
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

        Err(RockBotError::Provider("fal.ai request timed out".into()))
    }

    async fn fetch_result(&self, request_id: &str) -> Result<String> {
        let url = format!(
            "{}/{}/requests/{}",
            self.base_url, self.model_id, request_id
        );

        let response = self
            .http_client
            .get(&url)
            .header("Authorization", format!("Key {}", self.api_key))
            .send()
            .await?;

        let body: serde_json::Value = response.json().await?;

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

    pub async fn generate_image(&self, prompt: &str) -> Result<String> {
        let request_id = self.submit_request(prompt).await?;
        self.poll_status(&request_id).await
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
}
