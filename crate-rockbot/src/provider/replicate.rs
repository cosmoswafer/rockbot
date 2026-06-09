use crate::config::ProviderConfig;
use crate::error::{Result, RockBotError};

pub struct ReplicateProvider {
    api_key: String,
    base_url: String,
    model: String,
    http_client: reqwest::Client,
}

impl std::fmt::Debug for ReplicateProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReplicateProvider")
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .finish()
    }
}

impl ReplicateProvider {
    pub fn new(config: &ProviderConfig, model: impl Into<String>) -> Result<Self> {
        config.validate_api_key()?;

        Ok(Self {
            api_key: config.api_key.clone(),
            base_url: config.base_url.trim_end_matches('/').to_string(),
            model: model.into(),
            http_client: reqwest::Client::new(),
        })
    }

    pub fn with_client(
        config: &ProviderConfig,
        model: impl Into<String>,
        client: reqwest::Client,
    ) -> Result<Self> {
        config.validate_api_key()?;

        Ok(Self {
            api_key: config.api_key.clone(),
            base_url: config.base_url.trim_end_matches('/').to_string(),
            model: model.into(),
            http_client: client,
        })
    }

    pub fn model_name(&self) -> &str {
        &self.model
    }

    pub fn provider_name(&self) -> &str {
        "replicate"
    }

    pub async fn create_prediction(&self, prompt: &str) -> Result<String> {
        let body = serde_json::json!({
            "version": self.model,
            "input": {
                "prompt": prompt,
                "output_format": "png",
                "aspect_ratio": "1:1",
                "output_quality": 80
            }
        });

        let response = self
            .http_client
            .post(format!("{}/predictions", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        let status = response.status();
        let response_body: serde_json::Value = response.json().await?;

        if !status.is_success() {
            let msg = response_body
                .get("error")
                .and_then(|e| e.as_str())
                .unwrap_or("Unknown error");
            return Err(RockBotError::Provider(format!(
                "Replicate prediction failed: {}",
                msg
            )));
        }

        let prediction_id = response_body
            .get("id")
            .and_then(|i| i.as_str())
            .ok_or_else(|| RockBotError::Provider("Replicate response missing id".into()))?;

        Ok(prediction_id.to_string())
    }

    pub async fn wait_for_prediction(&self, prediction_id: &str) -> Result<String> {
        let url = format!("{}/predictions/{}", self.base_url, prediction_id);
        let max_attempts: u32 = 60;
        let delay_ms: u64 = 2000;

        for _ in 0..max_attempts {
            let response = self
                .http_client
                .get(&url)
                .header("Authorization", format!("Bearer {}", self.api_key))
                .send()
                .await?;

            let body: serde_json::Value = response.json().await?;

            let status = body
                .get("status")
                .and_then(|s| s.as_str())
                .unwrap_or("unknown");

            match status {
                "succeeded" => {
                    let output = body.get("output");
                    let image_url = output
                        .and_then(|o| o.as_array())
                        .and_then(|arr| arr.first())
                        .and_then(|u| u.as_str())
                        .or_else(|| output.and_then(|o| o.as_str()));

                    return match image_url {
                        Some(url) => Ok(url.to_string()),
                        None => Err(RockBotError::Provider(
                            "Replicate prediction succeeded but no output URL found".into(),
                        )),
                    };
                }
                "failed" | "canceled" => {
                    let error = body
                        .get("error")
                        .and_then(|e| e.as_str())
                        .unwrap_or("Unknown error");
                    return Err(RockBotError::Provider(format!(
                        "Replicate prediction {}: {}",
                        status, error
                    )));
                }
                _ => {
                    tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                }
            }
        }

        Err(RockBotError::Provider(
            "Replicate prediction timed out".into(),
        ))
    }

    pub async fn generate_image(&self, prompt: &str) -> Result<String> {
        let prediction_id = self.create_prediction(prompt).await?;
        self.wait_for_prediction(&prediction_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProviderConfig;
    use std::collections::HashMap;

    fn make_config(api_key: &str) -> ProviderConfig {
        ProviderConfig {
            name: "replicate".into(),
            api_key: api_key.into(),
            base_url: "https://api.replicate.com/v1".into(),
            basecf_url: None,
            chat_path: None,
            draw_path: None,
            models: HashMap::new(),
        }
    }

    #[test]
    fn test_new_success() {
        let config = make_config("r8_test_key");
        let provider = ReplicateProvider::new(&config, "black-forest-labs/flux-1.1-pro-ultra");
        assert!(provider.is_ok());
    }

    #[test]
    fn test_new_missing_api_key_editme() {
        let config = make_config("EDITME");
        let provider = ReplicateProvider::new(&config, "model");
        assert!(provider.is_err());
    }

    #[test]
    fn test_new_empty_api_key() {
        let config = make_config("");
        let provider = ReplicateProvider::new(&config, "model");
        assert!(provider.is_err());
    }

    #[test]
    fn test_provider_names() {
        let config = make_config("r8_test");
        let provider = ReplicateProvider::new(&config, "test-model").unwrap();
        assert_eq!(provider.provider_name(), "replicate");
        assert_eq!(provider.model_name(), "test-model");
    }
}
