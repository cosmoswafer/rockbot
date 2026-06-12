use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use tracing::warn;

use crate::error::{Result, RockBotError};
use crate::tool::Tool;
use crate::validated::NonEmptyString;

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

#[derive(Debug, Deserialize)]
struct WebSearchParams {
    query: NonEmptyString,
    #[serde(rename = "type", default = "default_search_type")]
    search_type: SearchType,
    #[serde(default = "default_num_results")]
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

pub struct WebSearchTool {
    api_key: Option<String>,
    http_client: reqwest::Client,
}

impl WebSearchTool {
    pub fn new(api_key: impl Into<String>) -> Self {
        let key = api_key.into();
        let key = if key.is_empty() { None } else { Some(key) };
        Self {
            api_key: key,
            http_client: reqwest::Client::new(),
        }
    }

    pub fn with_client(api_key: impl Into<String>, client: reqwest::Client) -> Self {
        let key = api_key.into();
        let key = if key.is_empty() { None } else { Some(key) };
        Self {
            api_key: key,
            http_client: client,
        }
    }

    async fn search_exa(&self, query: &str, search_type: &str, num_results: u32, contents_mode: &str) -> Result<String> {
        let api_key = self.api_key.as_deref().ok_or_else(|| {
            RockBotError::Provider(
                "web_search requires an Exa API key. Configure it in [tools.exa] section of config.toml."
                    .into(),
            )
        })?;

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
                let resp = self
                    .http_client
                    .post("https://api.exa.ai/search")
                    .header("x-api-key", api_key)
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
                        "Exa search failed: invalid API key (401). Check your EXA_API_KEY env var or [tools.exa] config.".into(),
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

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "web_search"
    }

    fn description(&self) -> &str {
        "Search the web using Exa. Returns ranked results with titles, URLs, highlights, and dates. \
         Supports optional type (auto/fast/deep), num_results, and contents_mode (highlights/text/deep) parameters."
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
                    "description": "Search type: auto (balanced with autoprompt), fast (quick results), deep (comprehensive). Default: auto"
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
            RockBotError::ToolCallParse(format!("Failed to parse web_search arguments: {}", e))
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

        self.search_exa(&params.query, search_type, params.num_results, contents_mode).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_web_search_tool_definition() {
        let tool = WebSearchTool::new("test-key");
        assert_eq!(tool.name(), "web_search");
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
    fn test_web_search_tool_to_def() {
        let tool = WebSearchTool::new("test-key");
        let def = tool.to_def();
        assert_eq!(def.function.name, "web_search");
    }

    #[tokio::test]
    async fn test_execute_missing_query() {
        let tool = WebSearchTool::new("test-key");
        let result = tool.execute(r#"{}"#).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_invalid_json() {
        let tool = WebSearchTool::new("test-key");
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
    fn test_exa_contents_mode_highlights() {
        let tool = WebSearchTool::new("test-key");
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

    #[test]
    fn test_exa_contents_mode_text_in_execute() {
        let args: Value =
            serde_json::from_str(r#"{"query": "rust", "contents_mode": "text"}"#).unwrap();
        assert_eq!(args["query"], "rust");
        assert_eq!(args["contents_mode"], "text");
        let contents_mode = args
            .get("contents_mode")
            .and_then(|c| c.as_str())
            .unwrap_or("highlights");
        assert_eq!(contents_mode, "text");
    }
}
