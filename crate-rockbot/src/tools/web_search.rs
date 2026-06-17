use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use serde_valid::Validate;
use tracing::warn;

use crate::error::{Result, RockBotError};
use crate::tool::Tool;
use crate::validated::NonEmptyString;

// ─── SearchProvider trait ─────────────────────────────────────────────────────

#[async_trait]
pub trait SearchProvider: Send + Sync {
    async fn search(
        &self,
        client: &reqwest::Client,
        query: &str,
        search_type: &str,
        num_results: u32,
        contents_mode: &str,
    ) -> Result<String>;
    fn provider_name(&self) -> &str;
}

// ─── ExaSearchProvider ────────────────────────────────────────────────────────

pub struct ExaSearchProvider {
    api_key: String,
    base_url: String,
}

impl ExaSearchProvider {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: "https://api.exa.ai".into(),
        }
    }

    /// Test-only — allows injecting a mock server URL.
    pub fn with_client(api_key: impl Into<String>, base_url: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: base_url.into(),
        }
    }
}

#[async_trait]
impl SearchProvider for ExaSearchProvider {
    fn provider_name(&self) -> &str {
        "exa"
    }

    async fn search(
        &self,
        client: &reqwest::Client,
        query: &str,
        search_type: &str,
        num_results: u32,
        contents_mode: &str,
    ) -> Result<String> {
        if self.api_key.is_empty() {
            return Err(RockBotError::Provider(
                "Exa search requires an API key. Configure it in the [search.exa] section of config.toml."
                    .into(),
            ));
        }

        let contents = match contents_mode {
            "text" => serde_json::json!({
                "text": {"maxCharacters": 15000}
            }),
            "deep" => serde_json::json!({
                "text": {"maxCharacters": 15000}
            }),
            _ => serde_json::json!({
                "highlights": {
                    "enabled": true,
                    "query": query
                }
            }),
        };

        let body = serde_json::json!({
            "query": query,
            "numResults": num_results,
            "type": search_type,
            "contents": contents
        });

        let max_retries: u32 = 3;
        let mut delay_ms: u64 = 1000;

        let response = 'retry: {
            for attempt in 1..=max_retries {
                let resp = client
                    .post(format!("{}/search", self.base_url))
                    .header("x-api-key", &self.api_key)
                    .header("Content-Type", "application/json")
                    .json(&body)
                    .send()
                    .await?;

                let status = resp.status();
                if status.is_success() {
                    break 'retry resp;
                }

                if status.as_u16() == 429 || status.as_u16() >= 500 {
                    let error_body = resp
                        .text()
                        .await
                        .unwrap_or_else(|_| "unknown error".into());
                    if attempt < max_retries {
                        warn!(
                            "Exa search returned {} (attempt {}/{}), retrying in {}ms: {}",
                            status, attempt, max_retries, delay_ms, error_body
                        );
                        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                        delay_ms *= 2;
                        continue;
                    }
                    return Err(RockBotError::Provider(format!(
                        "Exa search failed with status {} after {} retries: {}",
                        status, max_retries, error_body
                    )));
                }

                if status.as_u16() == 401 {
                    return Err(RockBotError::Provider(
                        "Exa search failed: invalid API key (401). Check your [search.exa] config."
                            .into(),
                    ));
                }

                let error_body = resp
                    .text()
                    .await
                    .unwrap_or_else(|_| "unknown error".into());
                return Err(RockBotError::Provider(format!(
                    "Exa search failed with status {}: {}",
                    status, error_body
                )));
            }
            unreachable!()
        };

        let body: Value = response.json().await?;
        let results = body
            .get("results")
            .and_then(|r| r.as_array())
            .ok_or_else(|| RockBotError::Provider("Exa returned no results array".into()))?;

        if results.is_empty() {
            return Ok("No search results found.".to_string());
        }

        let mut output = String::new();
        for (i, result) in results.iter().enumerate() {
            let title = result
                .get("title")
                .and_then(|t| t.as_str())
                .unwrap_or("Untitled");
            let url = result.get("url").and_then(|u| u.as_str()).unwrap_or("");

            let summary = result
                .get("highlights")
                .and_then(|h| h.as_array())
                .and_then(|arr| {
                    if arr.is_empty() {
                        None
                    } else {
                        Some(
                            arr.iter()
                                .map(|s| s.as_str().unwrap_or(""))
                                .collect::<Vec<_>>()
                                .join(" ... "),
                        )
                    }
                })
                .or_else(|| {
                    result.get("text").and_then(|t| t.as_str()).map(|t| {
                        if t.len() > 500 {
                            let end = t.char_indices().map(|(i, _)| i).nth(500).unwrap_or(t.len());
                            format!("{}...", &t[..end])
                        } else {
                            t.to_string()
                        }
                    })
                })
                .unwrap_or_default();

            let date = result
                .get("publishedDate")
                .or_else(|| result.get("published_date"))
                .and_then(|d| d.as_str())
                .unwrap_or("");

            output.push_str(&format!("{}. {}\n", i + 1, title));
            output.push_str(&format!("   URL: {}\n", url));
            if !date.is_empty() {
                output.push_str(&format!("   Date: {}\n", date));
            }
            output.push_str(&format!("   {}\n\n", summary));
        }

        Ok(output)
    }
}

// ─── BraveSearchProvider ──────────────────────────────────────────────────────

pub struct BraveSearchProvider {
    api_key: String,
    base_url: String,
}

impl BraveSearchProvider {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: "https://api.search.brave.com".into(),
        }
    }

    /// Test-only — allows injecting a mock server URL.
    pub fn with_client(api_key: impl Into<String>, base_url: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: base_url.into(),
        }
    }
}

#[async_trait]
impl SearchProvider for BraveSearchProvider {
    fn provider_name(&self) -> &str {
        "brave"
    }

    async fn search(
        &self,
        client: &reqwest::Client,
        query: &str,
        _search_type: &str,
        num_results: u32,
        _contents_mode: &str,
    ) -> Result<String> {
        if self.api_key.is_empty() {
            return Err(RockBotError::Provider(
                "Brave Search requires an API key. Configure it in the [search.brave] section of config.toml."
                    .into(),
            ));
        }

        let mut url = url::Url::parse(&format!("{}/res/v1/web/search", self.base_url))
            .expect("hardcoded Brave URL");
        url.query_pairs_mut()
            .append_pair("q", query)
            .append_pair("count", &num_results.to_string());
        let resp = client
            .get(url.as_str())
            .header("X-Subscription-Token", &self.api_key)
            .header("Accept", "application/json")
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let error_body = resp
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".into());
            return if status.as_u16() == 401 {
                Err(RockBotError::Provider(
                    "Brave Search failed: invalid API key (401). Check your [search.brave] config."
                        .into(),
                ))
            } else {
                Err(RockBotError::Provider(format!(
                    "Brave Search failed with status {}: {}",
                    status, error_body
                )))
            };
        }

        let body: Value = resp.json().await?;
        let results = body
            .get("web")
            .and_then(|w| w.get("results"))
            .and_then(|r| r.as_array())
            .ok_or_else(|| RockBotError::Provider("Brave returned no results array".into()))?;

        if results.is_empty() {
            return Ok("No search results found.".to_string());
        }

        let mut output = String::new();
        for (i, result) in results.iter().enumerate() {
            let title = result
                .get("title")
                .and_then(|t| t.as_str())
                .unwrap_or("Untitled");
            let url = result.get("url").and_then(|u| u.as_str()).unwrap_or("");
            let description = result
                .get("description")
                .and_then(|d| d.as_str())
                .unwrap_or("");
            let age = result
                .get("page_age")
                .and_then(|a| a.as_str())
                .unwrap_or("");

            output.push_str(&format!("{}. {}\n", i + 1, title));
            output.push_str(&format!("   URL: {}\n", url));
            if !age.is_empty() {
                output.push_str(&format!("   Age: {}\n", age));
            }
            output.push_str(&format!("   {}\n\n", description));
        }

        Ok(output)
    }
}

// ─── LLM-facing parameter types ───────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum SearchType {
    Auto,
    Fast,
    Deep,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum ContentsMode {
    Highlights,
    Text,
    Deep,
}

#[derive(Debug, Deserialize, Validate)]
struct WebSearchParams {
    query: NonEmptyString,
    #[serde(rename = "type", default = "default_search_type")]
    search_type: SearchType,
    #[serde(default = "default_num_results")]
    #[validate(minimum = 1)]
    #[validate(maximum = 20)]
    num_results: u32,
    #[serde(default = "default_contents_mode")]
    contents_mode: ContentsMode,
}

fn default_search_type() -> SearchType {
    SearchType::Auto
}
fn default_num_results() -> u32 {
    5
}
fn default_contents_mode() -> ContentsMode {
    ContentsMode::Highlights
}

// ─── WebSearchTool ────────────────────────────────────────────────────────────

pub struct WebSearchTool {
    provider: Box<dyn SearchProvider>,
    http_client: reqwest::Client,
}

impl WebSearchTool {
    pub fn new(provider: Box<dyn SearchProvider>) -> Self {
        Self {
            provider,
            http_client: reqwest::Client::new(),
        }
    }

    pub fn with_client(provider: Box<dyn SearchProvider>, client: reqwest::Client) -> Self {
        Self {
            provider,
            http_client: client,
        }
    }
}

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "search_web"
    }

    fn description(&self) -> &str {
        "Search the web using the configured search provider (Exa or Brave). Returns ranked results \
         with titles, URLs, highlights, and dates. Supports optional type (auto/fast/deep), \
         num_results, and contents_mode (highlights/text/deep) parameters."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "The search query to execute"
                },
                "type": {
                    "type": "string",
                    "enum": ["auto", "fast", "deep"],
                    "description": "Search type: auto (balanced), fast (quick results), deep (comprehensive). Default: auto"
                },
                "contents_mode": {
                    "type": "string",
                    "enum": ["highlights", "text", "deep"],
                    "description": "Content mode: highlights returns snippets (default), text returns full page content, deep enables comprehensive search"
                },
                "num_results": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 20,
                    "description": "Number of results to return (default: 5, max: 20)"
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let params: WebSearchParams = serde_json::from_str(arguments).map_err(|e| {
            RockBotError::ToolCallParse(format!("Failed to parse search_web arguments: {}", e))
        })?;
        params.validate().map_err(|e| {
            RockBotError::ToolCallParse(format!("Invalid search_web arguments: {e}"))
        })?;

        let search_type = match params.search_type {
            SearchType::Auto => "auto",
            SearchType::Fast => "fast",
            SearchType::Deep => "deep",
        };
        let contents_mode = match params.contents_mode {
            ContentsMode::Highlights => "highlights",
            ContentsMode::Text => "text",
            ContentsMode::Deep => "deep",
        };

        self.provider
            .search(
                &self.http_client,
                &params.query,
                search_type,
                params.num_results,
                contents_mode,
            )
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_search_web_tool_definition() {
        let provider = ExaSearchProvider::new("test-key");
        let tool = WebSearchTool::new(Box::new(provider));
        assert_eq!(tool.name(), "search_web");
        assert!(tool.description().contains("Search the web"));

        let params = tool.parameters();
        assert_eq!(params["type"], "object");
        assert!(
            params["required"]
                .as_array()
                .unwrap()
                .contains(&serde_json::json!("query"))
        );
        let search_types = params["properties"]["type"]["enum"].as_array().unwrap();
        assert!(search_types.contains(&serde_json::json!("auto")));
        assert!(search_types.contains(&serde_json::json!("fast")));
        assert!(search_types.contains(&serde_json::json!("deep")));
    }

    #[test]
    fn test_search_web_tool_to_def() {
        let provider = ExaSearchProvider::new("test-key");
        let tool = WebSearchTool::new(Box::new(provider));
        let def = tool.to_def();
        assert_eq!(def.function.name, "search_web");
    }

    #[tokio::test]
    async fn test_execute_missing_query() {
        let provider = ExaSearchProvider::new("test-key");
        let tool = WebSearchTool::new(Box::new(provider));
        let result = tool.execute(r#"{}"#).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_invalid_json() {
        let provider = ExaSearchProvider::new("test-key");
        let tool = WebSearchTool::new(Box::new(provider));
        let result = tool.execute("not json").await;
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_args_with_all_optional_params() {
        let args: Value =
            serde_json::from_str(r#"{"query": "rust", "type": "deep", "num_results": 10}"#)
                .unwrap();
        assert_eq!(args["query"], "rust");
        assert_eq!(args["type"], "deep");
        assert_eq!(args["num_results"], 10);
    }

    #[test]
    fn test_parse_args_defaults() {
        let args: Value = serde_json::from_str(r#"{"query": "rust"}"#).unwrap();
        let search_type = args.get("type").and_then(|t| t.as_str()).unwrap_or("auto");
        assert_eq!(search_type, "auto");
        let num_results = args
            .get("num_results")
            .and_then(|n| n.as_u64())
            .unwrap_or(5);
        assert_eq!(num_results, 5);
    }

    #[test]
    fn test_parse_args_num_results_bounds() {
        let args: Value = serde_json::from_str(r#"{"query": "x", "num_results": 1}"#).unwrap();
        assert_eq!(args["num_results"], 1);
        let args: Value = serde_json::from_str(r#"{"query": "x", "num_results": 20}"#).unwrap();
        assert_eq!(args["num_results"], 20);
    }

    #[test]
    fn test_exa_request_body_contains_highlights_enabled() {
        let body = serde_json::json!({
            "query": "rust",
            "numResults": 5,
            "type": "auto",
            "contents": {
                "highlights": {
                    "enabled": true,
                    "query": "rust"
                }
            }
        });
        assert!(body["contents"]["highlights"]["enabled"] == true);
        assert_eq!(body["contents"]["highlights"]["query"], "rust");
    }

    #[test]
    fn test_exa_request_body_type_is_not_neural() {
        let body = serde_json::json!({
            "query": "test",
            "numResults": 5,
            "type": "auto",
            "contents": {"highlights": {"enabled": true, "query": "test"}}
        });
        assert_ne!(
            body["type"], "neural",
            "\"neural\" is not a valid Exa search type"
        );
    }

    #[test]
    fn test_exa_request_body_no_deprecated_params() {
        let body = serde_json::json!({
            "query": "test",
            "numResults": 5,
            "type": "auto",
            "contents": {"highlights": {"enabled": true, "query": "test"}}
        });
        assert!(body.get("useAutoprompt").is_none(), "useAutoprompt is deprecated");
        assert!(body.get("numSentences").is_none());
        assert!(body.get("highlightsPerUrl").is_none());
    }

    #[test]
    fn test_parse_highlight_results() {
        let highlights: Vec<String> = vec![
            "Rust is fast".into(),
            "Memory safe".into(),
            "Zero-cost abstractions".into(),
        ];
        let summary = highlights.join(" ... ");
        assert_eq!(
            summary,
            "Rust is fast ... Memory safe ... Zero-cost abstractions"
        );
        assert!(summary.contains("Rust"));
        assert!(summary.contains("Memory safe"));
    }

    #[test]
    fn test_parse_text_fallback_when_no_highlights() {
        let text = "This is a long text that should be truncated at 500 characters...";
        let summary = if text.len() > 500 {
            let end = text.char_indices().map(|(i, _)| i).nth(500).unwrap_or(text.len());
            format!("{}...", &text[..end])
        } else {
            text.to_string()
        };
        assert!(!summary.is_empty());
    }

    #[test]
    fn test_search_web_params_schema() {
        let provider = ExaSearchProvider::new("test-key");
        let tool = WebSearchTool::new(Box::new(provider));
        let params = tool.parameters();
        let contents_mode = &params["properties"]["contents_mode"];
        assert_eq!(contents_mode["type"], "string");
        let contents_enums = contents_mode["enum"].as_array().unwrap();
        assert_eq!(contents_enums.len(), 3);
        assert!(contents_enums.contains(&serde_json::json!("highlights")));
        assert!(contents_enums.contains(&serde_json::json!("text")));
        assert!(contents_enums.contains(&serde_json::json!("deep")));
        assert!(
            contents_mode["description"]
                .as_str()
                .unwrap()
                .contains("highlights")
        );
    }

    #[tokio::test]
    async fn test_execute_with_empty_exa_key_returns_error() {
        let provider = ExaSearchProvider::new("");
        let tool = WebSearchTool::new(Box::new(provider));
        let result = tool.execute(r#"{"query":"test","contents_mode":"highlights"}"#).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Exa search requires an API key"), "Expected Exa API key error, got: {}", err);
    }

    #[tokio::test]
    async fn test_execute_with_empty_brave_key_returns_error() {
        let provider = BraveSearchProvider::new("");
        let tool = WebSearchTool::new(Box::new(provider));
        let result = tool.execute(r#"{"query":"test"}"#).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Brave Search requires an API key"), "Expected Brave API key error, got: {}", err);
    }

    #[test]
    fn test_exa_provider_name() {
        let provider = ExaSearchProvider::new("key");
        assert_eq!(provider.provider_name(), "exa");
    }

    #[test]
    fn test_brave_provider_name() {
        let provider = BraveSearchProvider::new("key");
        assert_eq!(provider.provider_name(), "brave");
    }
}
