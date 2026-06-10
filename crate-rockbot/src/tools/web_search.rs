use async_trait::async_trait;
use serde_json::Value;

use crate::error::{Result, RockBotError};
use crate::tool::Tool;

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

    async fn search_exa(&self, query: &str, search_type: &str, num_results: u32) -> Result<String> {
        let api_key = self.api_key.as_deref().ok_or_else(|| {
            RockBotError::Provider(
                "web_search requires EXA_API_KEY to be set. Configure it in your environment."
                    .into(),
            )
        })?;

        let mut body = serde_json::json!({
            "query": query,
            "numResults": num_results,
            "type": search_type,
            "contents": {
                "highlights": {
                    "numSentences": 3,
                    "highlightsPerUrl": 3,
                    "query": query
                }
            }
        });

        if search_type == "auto" {
            body["useAutoprompt"] = serde_json::Value::Bool(true);
        }

        let response = self
            .http_client
            .post("https://api.exa.ai/search")
            .header("x-api-key", api_key)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let error_body = response
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".into());
            return Err(RockBotError::Provider(format!(
                "Exa search failed with status {}: {}",
                status, error_body
            )));
        }

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
                            format!("{}...", &t[..500])
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
         Supports optional type (auto/fast/deep) and num_results parameters."
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
        let args: Value = serde_json::from_str(arguments).map_err(|e| {
            RockBotError::ToolCallParse(format!("Failed to parse web_search arguments: {}", e))
        })?;

        let query = args.get("query").and_then(|q| q.as_str()).ok_or_else(|| {
            RockBotError::ToolCallParse("web_search requires 'query' field".into())
        })?;

        let search_type = args.get("type").and_then(|t| t.as_str()).unwrap_or("auto");

        let num_results = args
            .get("num_results")
            .and_then(|n| n.as_u64())
            .unwrap_or(5) as u32;

        self.search_exa(query, search_type, num_results).await
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
}
