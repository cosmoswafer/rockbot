use async_trait::async_trait;
use serde_json::Value;

use crate::error::{Result, RockBotError};
use crate::tool::Tool;

pub struct WebFetchTool {
    http_client: reqwest::Client,
    exa_api_key: Option<String>,
}

impl WebFetchTool {
    pub fn new() -> Self {
        Self {
            http_client: reqwest::Client::new(),
            exa_api_key: None,
        }
    }

    pub fn with_exa_key(api_key: impl Into<String>) -> Self {
        Self {
            http_client: reqwest::Client::new(),
            exa_api_key: Some(api_key.into()),
        }
    }

    pub fn with_client_and_key(client: reqwest::Client, api_key: Option<String>) -> Self {
        Self {
            http_client: client,
            exa_api_key: api_key,
        }
    }
}

impl Default for WebFetchTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputFormat {
    Json,
    Markdown,
    Raw,
}

impl OutputFormat {
    fn from_str(s: &str) -> Self {
        match s {
            "json" => Self::Json,
            "markdown" => Self::Markdown,
            _ => Self::Raw,
        }
    }
}

impl WebFetchTool {
    async fn fetch_url(&self, url: &str, format: OutputFormat, verify: bool) -> Result<String> {
        let response = self
            .http_client
            .get(url)
            .header("User-Agent", "RockBot/1.0")
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await?;

        let status = response.status().as_u16();
        if !response.status().is_success() {
            return Err(RockBotError::Provider(format!(
                "Failed to fetch URL: HTTP {}",
                status
            )));
        }

        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("unknown")
            .to_string();

        let body = response.text().await?;

        let is_html = content_type.contains("text/html");

        let related_sources = if verify {
            self.verify_content(url, &body, is_html).await
        } else {
            None
        };

        match format {
            OutputFormat::Json => {
                let content = if is_html {
                    html_to_markdown(&body)
                } else {
                    truncate(&body, 10000)
                };

                let mut json = serde_json::json!({
                    "url": url,
                    "status": status,
                    "content_type": content_type,
                    "content": content,
                    "verified": verify,
                });

                if let Some(sources) = related_sources {
                    json["related_sources"] = sources;
                } else if verify {
                    json["related_sources"] = serde_json::json!([]);
                }

                Ok(json.to_string())
            }
            OutputFormat::Markdown => {
                if is_html {
                    let mut md = html_to_markdown(&body);
                    if let Some(sources) = related_sources {
                        md.push_str("\n\n## Related Sources\n\n");
                        append_related_sources(&mut md, &sources);
                    }
                    Ok(md)
                } else {
                    Ok(truncate(&body, 10000))
                }
            }
            OutputFormat::Raw => {
                let mut out = truncate(&body, 10000);
                if let Some(sources) = related_sources {
                    out.push_str("\n\n## Related Sources\n\n");
                    append_related_sources(&mut out, &sources);
                }
                Ok(out)
            }
        }
    }

    async fn verify_content(
        &self,
        url: &str,
        body: &str,
        is_html: bool,
    ) -> Option<serde_json::Value> {
        let api_key = self.exa_api_key.as_ref()?;
        if api_key.is_empty() {
            return None;
        }

        let query = if is_html {
            extract_page_title(body).unwrap_or_else(|| extract_domain(url).to_string())
        } else {
            url.to_string()
        };

        let response = self
            .http_client
            .post("https://api.exa.ai/search")
            .header("x-api-key", api_key)
            .header("Content-Type", "application/json")
            .json(&serde_json::json!({
                "query": query,
                "numResults": 3,
            }))
            .send()
            .await
            .ok()?;

        if !response.status().is_success() {
            return None;
        }

        let body: Value = response.json().await.ok()?;
        let results = body.get("results")?.as_array()?;

        let sources: Vec<Value> = results
            .iter()
            .map(|r| {
                serde_json::json!({
                    "title": r.get("title").and_then(|t| t.as_str()).unwrap_or("Untitled"),
                    "url": r.get("url").and_then(|u| u.as_str()).unwrap_or(""),
                    "snippet": r.get("text").and_then(|t| t.as_str())
                        .or_else(|| r.get("snippet").and_then(|s| s.as_str()))
                        .unwrap_or(""),
                })
            })
            .collect();

        Some(serde_json::json!(sources))
    }
}

fn extract_page_title(html: &str) -> Option<String> {
    let lower = html.to_lowercase();
    let start = lower.find("<title")?;
    let tag_end = lower[start..].find('>')?;
    let content_start = start + tag_end + 1;
    let close = lower[content_start..].find("</title>")?;
    let title = &html[content_start..content_start + close];
    let title = title.trim().to_string();
    if title.is_empty() { None } else { Some(title) }
}

fn extract_domain(url: &str) -> &str {
    url.strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or(url)
}

fn append_related_sources(out: &mut String, sources: &serde_json::Value) {
    if let Some(arr) = sources.as_array() {
        for (i, src) in arr.iter().enumerate() {
            let title = src
                .get("title")
                .and_then(|t| t.as_str())
                .unwrap_or("Untitled");
            let src_url = src.get("url").and_then(|u| u.as_str()).unwrap_or("");
            let snippet = src.get("snippet").and_then(|s| s.as_str()).unwrap_or("");
            out.push_str(&format!("{}. **{}**\n", i + 1, title));
            out.push_str(&format!("   URL: {}\n", src_url));
            if !snippet.is_empty() {
                out.push_str(&format!("   {}\n", snippet));
            }
            out.push('\n');
        }
    }
}

fn html_to_markdown(html: &str) -> String {
    let mut result = html.to_string();

    result = result
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'");

    result = remove_tag_content(&result, "script");
    result = remove_tag_content(&result, "style");
    result = result
        .replace("<br>", "\n")
        .replace("<br/>", "\n")
        .replace("<br />", "\n");
    result = result.replace("<p>", "\n\n").replace("</p>", "\n\n");
    result = result.replace("<strong>", "**").replace("</strong>", "**");
    result = result.replace("<b>", "**").replace("</b>", "**");
    result = result.replace("<em>", "*").replace("</em>", "*");
    result = result.replace("<i>", "*").replace("</i>", "*");
    result = result.replace("<code>", "`").replace("</code>", "`");

    for level in 1..=6 {
        let open = format!("<h{}>", level);
        let close = format!("</h{}>", level);
        let heading = format!("\n\n{} ", "#".repeat(level));
        result = result.replace(&open, &heading);
        result = result.replace(&close, "\n\n");
    }

    result = convert_links(&result);
    result = strip_remaining_tags(&result);
    result = compress_newlines(&result);
    result = result.trim().to_string();

    truncate(&result, 10000)
}

fn truncate(text: &str, max_chars: usize) -> String {
    if text.len() <= max_chars {
        text.to_string()
    } else {
        let mut result = text[..max_chars].to_string();
        result.push_str("\n\n... (truncated)");
        result
    }
}

fn remove_tag_content(html: &str, tag: &str) -> String {
    let mut result = Vec::with_capacity(html.len());
    let bytes = html.as_bytes();
    let open = format!("<{}", tag);
    let close = format!("</{}>", tag);
    let open_bytes = open.as_bytes();
    let close_bytes = close.as_bytes();
    let mut skip = false;
    let mut depth: i32 = 0;
    let mut i = 0;

    while i < bytes.len() {
        if !skip
            && bytes[i] == b'<'
            && i + open_bytes.len() <= bytes.len()
            && bytes[i..i + open_bytes.len()].eq_ignore_ascii_case(open_bytes)
        {
            skip = true;
            depth = 1;
            i += open_bytes.len();
            continue;
        }
        if skip
            && bytes[i] == b'<'
            && i + close_bytes.len() <= bytes.len()
            && bytes[i..i + close_bytes.len()].eq_ignore_ascii_case(close_bytes)
        {
            depth -= 1;
            i += close_bytes.len();
            if depth <= 0 {
                skip = false;
            }
            continue;
        }
        if skip && depth > 0 && bytes[i] == b'<' {
            let end = find_byte(html, i, b'>');
            let tag_text = if end > i { &html[i + 1..end] } else { "" };
            let tag_name = tag_text
                .split_whitespace()
                .next()
                .unwrap_or("")
                .trim_start_matches('/');
            if tag_name.eq_ignore_ascii_case(tag) {
                depth += 1;
            }
            i = end + 1;
            continue;
        }
        if !skip {
            result.push(bytes[i]);
        }
        i += 1;
    }

    String::from_utf8_lossy(&result).into_owned()
}

fn find_byte(html: &str, start: usize, byte: u8) -> usize {
    html.as_bytes()[start..]
        .iter()
        .position(|&b| b == byte)
        .map(|p| start + p)
        .unwrap_or(html.len())
}

fn convert_links(html: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let mut i = 0;
    let bytes = html.as_bytes();

    while i < bytes.len() {
        if bytes[i] == b'<' && i + 2 < bytes.len() && bytes[i + 1] == b'a' {
            let end = find_byte(html, i, b'>');
            let tag = &html[i..end];
            let href = extract_attr(tag, "href");
            let close_tag = "</a>";
            if let Some(close_pos) = html[end..].find(close_tag) {
                let text = &html[end..end + close_pos];
                if let Some(href_val) = href {
                    result.push_str(&format!("[{}]({})", text, href_val));
                } else {
                    result.push_str(text);
                }
                i = end + close_pos + close_tag.len();
                continue;
            }
        }
        let ch = html[i..].chars().next().unwrap();
        result.push(ch);
        i += ch.len_utf8();
    }
    result
}

fn extract_attr(tag: &str, attr: &str) -> Option<String> {
    let lower = tag.to_lowercase();
    let pattern = format!("{}=", attr);
    let pos = lower.find(&pattern)?;
    let after = &tag[pos + pattern.len()..];
    if let Some(stripped) = after.strip_prefix('"') {
        let end = stripped.find('"')?;
        Some(stripped[..end].to_string())
    } else if let Some(stripped) = after.strip_prefix('\'') {
        let end = stripped.find('\'')?;
        Some(stripped[..end].to_string())
    } else {
        let end = after.find(|c: char| c.is_whitespace() || c == '>')?;
        Some(after[..end].to_string())
    }
}

fn strip_remaining_tags(html: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => result.push(ch),
            _ => {}
        }
    }
    result
}

fn compress_newlines(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut newline_count: u32 = 0;
    for ch in text.chars() {
        if ch == '\n' {
            newline_count += 1;
            if newline_count <= 2 {
                result.push(ch);
            }
        } else {
            newline_count = 0;
            result.push(ch);
        }
    }
    result
}

#[async_trait]
impl Tool for WebFetchTool {
    fn name(&self) -> &str {
        "web_fetch"
    }

    fn description(&self) -> &str {
        "Fetch content from a URL. Supports three output formats: json (structured with metadata), \
         markdown (HTML converted to markdown for AI consumption), raw (unmodified response text). \
         Optionally cross-verifies content via web search when verify=true."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "The URL to fetch"
                },
                "format": {
                    "type": "string",
                    "enum": ["json", "markdown", "raw"],
                    "description": "Output format: json returns structured metadata, \
                                    markdown converts HTML to markdown for AI, \
                                    raw returns unmodified text (default: raw)"
                },
                "verify": {
                    "type": "boolean",
                    "description": "Perform a web search to cross-verify content (default: false)"
                }
            },
            "required": ["url"]
        })
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments).map_err(|e| {
            RockBotError::ToolCallParse(format!("Failed to parse web_fetch arguments: {}", e))
        })?;

        let url = args
            .get("url")
            .and_then(|u| u.as_str())
            .ok_or_else(|| RockBotError::ToolCallParse("web_fetch requires 'url' field".into()))?;

        let format = args
            .get("format")
            .and_then(|f| f.as_str())
            .map(OutputFormat::from_str)
            .unwrap_or(OutputFormat::Raw);

        let verify = args
            .get("verify")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        self.fetch_url(url, format, verify).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_web_fetch_tool_definition() {
        let tool = WebFetchTool::new();
        assert_eq!(tool.name(), "web_fetch");
        assert!(tool.description().contains("Fetch content"));

        let params = tool.parameters();
        assert_eq!(params["type"], "object");
        assert!(
            params["properties"]["format"]["enum"]
                .as_array()
                .unwrap()
                .len()
                == 3
        );
    }

    #[test]
    fn test_html_to_markdown_basic() {
        let html = "<h1>Title</h1><p>Hello <b>world</b></p>";
        let md = html_to_markdown(html);
        assert!(md.contains("# Title"));
        assert!(md.contains("**world**"));
    }

    #[test]
    fn test_html_to_markdown_link() {
        let html = r#"<a href="https://example.com">Click here</a>"#;
        let md = html_to_markdown(html);
        assert!(md.contains("[Click here](https://example.com)"));
    }

    #[test]
    fn test_html_to_markdown_strips_script() {
        let html = "<p>Hello</p><script>evil()</script><p>World</p>";
        let md = html_to_markdown(html);
        assert!(!md.contains("evil"));
        assert!(md.contains("Hello"));
        assert!(md.contains("World"));
    }

    #[test]
    fn test_html_to_markdown_chinese() {
        let html =
            "<html><head><title>2026贵州旅游</title></head><body><p>本地人私藏</p></body></html>";
        let md = html_to_markdown(html);
        assert!(md.contains("2026贵州旅游"));
        assert!(md.contains("本地人私藏"));
    }

    #[test]
    fn test_remove_tag_content_with_chinese() {
        let html = "<p>你好</p><script>var x = 1;</script><p>世界</p>";
        let result = remove_tag_content(html, "script");
        assert!(result.contains("你好"));
        assert!(result.contains("世界"));
        assert!(!result.contains("var x"));
    }

    #[test]
    fn test_output_format_from_str() {
        assert_eq!(OutputFormat::from_str("json"), OutputFormat::Json);
        assert_eq!(OutputFormat::from_str("markdown"), OutputFormat::Markdown);
        assert_eq!(OutputFormat::from_str("raw"), OutputFormat::Raw);
        assert_eq!(OutputFormat::from_str("unknown"), OutputFormat::Raw);
    }

    #[test]
    fn test_extract_page_title() {
        let html = "<html><head><title>My Page</title></head><body></body></html>";
        assert_eq!(extract_page_title(html), Some("My Page".into()));
    }

    #[test]
    fn test_extract_page_title_empty() {
        let html = "<html><head><title></title></head></html>";
        assert_eq!(extract_page_title(html), None);
    }

    #[test]
    fn test_extract_page_title_none() {
        let html = "<html><body>No title</body></html>";
        assert_eq!(extract_page_title(html), None);
    }

    #[test]
    fn test_extract_domain() {
        assert_eq!(extract_domain("https://example.com/path"), "example.com");
        assert_eq!(extract_domain("http://foo.bar/baz?q=1"), "foo.bar");
        assert_eq!(extract_domain("plain.domain"), "plain.domain");
    }

    #[test]
    fn test_truncate() {
        assert_eq!(truncate("hello", 100), "hello");
        let long = "a".repeat(10005);
        let truncated = truncate(&long, 10000);
        assert_eq!(truncated.len(), 10000 + "\n\n... (truncated)".len());
        assert!(truncated.ends_with("... (truncated)"));
    }

    #[tokio::test]
    async fn test_execute_missing_url() {
        let tool = WebFetchTool::new();
        let result = tool.execute(r#"{}"#).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_format_parameter() {
        let args: Value =
            serde_json::from_str(r#"{"url": "https://x.example", "format": "json"}"#).unwrap();
        assert_eq!(args["format"].as_str().unwrap(), "json");
        let format = args
            .get("format")
            .and_then(|f| f.as_str())
            .map(OutputFormat::from_str)
            .unwrap_or(OutputFormat::Raw);
        assert_eq!(format, OutputFormat::Json);
    }

    #[test]
    fn test_default() {
        let tool = WebFetchTool::default();
        assert_eq!(tool.name(), "web_fetch");
    }

    #[test]
    fn test_with_exa_key() {
        let tool = WebFetchTool::with_exa_key("test-key");
        assert_eq!(tool.exa_api_key, Some("test-key".into()));
    }

    #[test]
    fn test_with_client_and_key() {
        let client = reqwest::Client::new();
        let tool = WebFetchTool::with_client_and_key(client, Some("key".into()));
        assert_eq!(tool.exa_api_key, Some("key".into()));
        let tool2 = WebFetchTool::with_client_and_key(reqwest::Client::new(), None);
        assert_eq!(tool2.exa_api_key, None);
    }
}
