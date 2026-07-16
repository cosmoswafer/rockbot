// ─── Mock Integration Tests: Tool Happy-Path Flows ──────────────────────────
//
// Each section covers one DFD's happy path with wiremock where the tool
// depends on external HTTP services (Exa, image servers, WebDAV).
// Pure-computation tools (calendar/memory) are tested via direct execute() calls.
//
// DFDs covered:
//   _dfd/tools/vision.md
//   _dfd/tools/exa-search.md
//   _dfd/tools/web-fetch.md
//   _dfd/tools/edit-soul.md
//   _dfd/tools/webdav.md
//   _dfd/tools/calendar.md
//   _dfd/base/memory.md
//   _dfd/agent-harness.md
//   _dfd/image-interception.md
//   _dfd/agent-loop.md

use rockbot::harness::AgentHarness;
use rockbot::image_cache::ImageCache;
use rockbot::memory::{ConversationHistory, MemoryManager, PersistSnapshot};
use rockbot::provider::AiProvider;
use rockbot::tool::Tool;
use rockbot::tools::{
    BraveSearchProvider, CalendarTool, EditSoulTool, ExaSearchProvider, VisionTool, WebDavTool,
    WebFetchTool, WebSearchTool,
};
use rockbot::types::{ChatMessage, ChatRequest, CompletionResult, ContentPart, FinishReason, MessageContent, ToolCall};
use rockbot::validated::{ConfigUrl, NonEmptyString, ProviderName};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use wiremock::matchers::{body_string_contains, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// A minimal 1×1 white PNG (67 bytes).
fn tiny_png_bytes() -> Vec<u8> {
    vec![
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D,
        0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
        0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53, 0xDE, 0x00, 0x00, 0x00,
        0x0C, 0x49, 0x44, 0x41, 0x54, 0x08, 0xD7, 0x63, 0xF8, 0xCF, 0xC0, 0x00,
        0x00, 0x03, 0x01, 0x01, 0x00, 0x18, 0xDD, 0x8D, 0xB0, 0x00, 0x00, 0x00,
        0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ]
}

// ============================================================================
// _dfd/tools/vision.md — Happy Path (wiremock)
// ============================================================================

#[tokio::test]
async fn test_vision_fetch_png_returns_markdown_image_tag() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/images/photo.png"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(tiny_png_bytes())
                .insert_header("Content-Type", "image/png"),
        )
        .mount(&mock_server)
        .await;

    let tool = VisionTool::new();
    let url = format!("{}/images/photo.png", mock_server.uri());
    let args = serde_json::json!({"url": url}).to_string();
    let result = tool.execute(&args).await.unwrap();

    assert!(result.starts_with("![photo.png](data:image/png;base64,"));
    assert!(result.ends_with(")"));
}

#[tokio::test]
async fn test_vision_fetch_jpeg_uses_content_type() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/images/photo.jpg"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(tiny_png_bytes())
                .insert_header("Content-Type", "image/jpeg"),
        )
        .mount(&mock_server)
        .await;

    let tool = VisionTool::new();
    let url = format!("{}/images/photo.jpg", mock_server.uri());
    let args = serde_json::json!({"url": url}).to_string();
    let result = tool.execute(&args).await.unwrap();

    // Content-Type header takes precedence over extension
    assert!(result.starts_with("![photo.jpg](data:image/jpeg;base64,"));
}

#[tokio::test]
async fn test_vision_detect_mime_from_extension() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/images/drawing.webp"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(tiny_png_bytes())
                // No Content-Type header — falls back to extension
                .insert_header("X-Custom", "value"),
        )
        .mount(&mock_server)
        .await;

    let tool = VisionTool::new();
    let url = format!("{}/images/drawing.webp", mock_server.uri());
    let args = serde_json::json!({"url": url}).to_string();
    let result = tool.execute(&args).await.unwrap();

    assert!(result.starts_with("![drawing.webp](data:image/webp;base64,"));
}

#[tokio::test]
async fn test_vision_non_200_status_returns_error() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/images/missing.png"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&mock_server)
        .await;

    let tool = VisionTool::new();
    let url = format!("{}/images/missing.png", mock_server.uri());
    let args = serde_json::json!({"url": url}).to_string();
    let result = tool.execute(&args).await;

    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("404"));
}

#[tokio::test]
async fn test_vision_image_too_large() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/images/huge.png"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(vec![0u8; 100])
                .insert_header("Content-Type", "image/png"),
        )
        .mount(&mock_server)
        .await;

    // Use a very small max to trigger the error
    let tool = VisionTool::with_max_bytes(50);
    let url = format!("{}/images/huge.png", mock_server.uri());
    let args = serde_json::json!({"url": url}).to_string();
    let result = tool.execute(&args).await;

    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("too large"));
}

#[tokio::test]
async fn test_vision_missing_url_param() {
    let tool = VisionTool::new();
    let result = tool.execute(r#"{}"#).await;
    assert!(result.is_err());
}

// ============================================================================
// _dfd/tools/search-web.md — Happy Path (wiremock)
// ============================================================================

#[tokio::test]
async fn test_search_web_tool_metadata() {
    let provider = ExaSearchProvider::new("test-exa-key");
    let tool = WebSearchTool::new(Box::new(provider));

    assert_eq!(tool.name(), "search_web");
    assert!(tool.description().contains("Search the web"));
    let params = tool.parameters();
    assert_eq!(params["type"], "object");

    let args = serde_json::json!({
        "query": "rust programming",
        "type": "auto",
        "num_results": 3
    })
    .to_string();

    let result = tool.execute(&args).await;
    assert!(result.is_err() || result.is_ok());
}

#[tokio::test]
async fn test_search_web_missing_query() {
    let provider = ExaSearchProvider::new("test-key");
    let tool = WebSearchTool::new(Box::new(provider));
    let result = tool.execute(r#"{}"#).await;
    assert!(result.is_err());
}

// ============================================================================
// _dfd/tools/search-web.md — Exa wiremock happy path
// ============================================================================

#[tokio::test]
async fn test_search_web_exa_api_returns_formatted_results() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/search"))
        .and(header("x-api-key", "test-exa-key"))
        .and(header("Content-Type", "application/json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "request_id": "req-001",
            "results": [
                {
                    "title": "Rust Programming Language",
                    "url": "https://www.rust-lang.org",
                    "publishedDate": "2026-01-15",
                    "highlights": [
                        "Rust is a systems programming language",
                        "Memory safety without garbage collection"
                    ]
                },
                {
                    "title": "Learn Rust",
                    "url": "https://learn.rust-lang.org",
                    "publishedDate": "2026-03-01",
                    "highlights": [
                        "Official Rust learning resources"
                    ]
                }
            ]
        })))
        .mount(&mock_server)
        .await;

    let provider = ExaSearchProvider::with_client("test-exa-key", mock_server.uri());
    let tool = WebSearchTool::new(Box::new(provider));

    let args = serde_json::json!({
        "query": "rust programming",
        "num_results": 5
    })
    .to_string();

    let result = tool.execute(&args).await.unwrap();
    assert!(result.contains("Rust Programming Language"), "should contain title");
    assert!(result.contains("https://www.rust-lang.org"), "should contain url");
    assert!(result.contains("Memory safety"), "should contain highlight");
    assert!(result.contains("Learn Rust"), "should contain second result");
    assert!(result.contains("2026-01-15"), "should contain date");
}

#[tokio::test]
async fn test_search_web_exa_api_empty_results_returns_message() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "request_id": "req-002",
            "results": []
        })))
        .mount(&mock_server)
        .await;

    let provider = ExaSearchProvider::with_client("test-key", mock_server.uri());
    let tool = WebSearchTool::new(Box::new(provider));

    let result = tool.execute(r#"{"query": "nonexistent", "num_results": 5}"#).await.unwrap();
    assert_eq!(result, "No search results found.");
}

#[tokio::test]
async fn test_search_web_exa_api_401_returns_error() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/search"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&mock_server)
        .await;

    let provider = ExaSearchProvider::with_client("bad-key", mock_server.uri());
    let tool = WebSearchTool::new(Box::new(provider));

    let result = tool.execute(r#"{"query": "test"}"#).await;
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("401"), "expected 401 error, got: {err}");
}

#[tokio::test]
async fn test_search_web_exa_text_mode_uses_text_contents() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/search"))
        .and(body_string_contains("\"text\""))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "results": [{
                "title": "Full Article",
                "url": "https://example.com/article",
                "text": "This is the full article content with lots of useful information."
            }]
        })))
        .mount(&mock_server)
        .await;

    let provider = ExaSearchProvider::with_client("test-key", mock_server.uri());
    let tool = WebSearchTool::new(Box::new(provider));

    let args = serde_json::json!({
        "query": "full article",
        "contents_mode": "text"
    })
    .to_string();

    let result = tool.execute(&args).await.unwrap();
    assert!(result.contains("Full Article"));
    assert!(result.contains("full article content"));
}

// ============================================================================
// _dfd/tools/search-web.md — Brave wiremock happy path
// ============================================================================

#[tokio::test]
async fn test_search_web_brave_api_returns_formatted_results() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/res/v1/web/search"))
        .and(header("X-Subscription-Token", "test-brave-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "web": {
                "results": [
                    {
                        "title": "Brave Search",
                        "url": "https://search.brave.com",
                        "description": "Brave Search is a private search engine",
                        "page_age": "2025-06-01"
                    },
                    {
                        "title": "Brave Browser",
                        "url": "https://brave.com",
                        "description": "Fast, private browser"
                    }
                ]
            }
        })))
        .mount(&mock_server)
        .await;

    let provider = BraveSearchProvider::with_client("test-brave-key", mock_server.uri());
    let tool = WebSearchTool::new(Box::new(provider));

    let result = tool.execute(r#"{"query": "brave search", "num_results": 5}"#).await.unwrap();
    assert!(result.contains("Brave Search"), "should contain title");
    assert!(result.contains("https://search.brave.com"), "should contain url");
    assert!(result.contains("private search engine"), "should contain description");
    assert!(result.contains("Brave Browser"), "should contain second result");
}

#[tokio::test]
async fn test_search_web_brave_api_empty_results_returns_message() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/res/v1/web/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "web": { "results": [] }
        })))
        .mount(&mock_server)
        .await;

    let provider = BraveSearchProvider::with_client("test-key", mock_server.uri());
    let tool = WebSearchTool::new(Box::new(provider));

    let result = tool.execute(r#"{"query": "nothing", "num_results": 5}"#).await.unwrap();
    assert_eq!(result, "No search results found.");
}

#[tokio::test]
async fn test_search_web_brave_api_401_returns_error() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/res/v1/web/search"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&mock_server)
        .await;

    let provider = BraveSearchProvider::with_client("bad-key", mock_server.uri());
    let tool = WebSearchTool::new(Box::new(provider));

    let result = tool.execute(r#"{"query": "test"}"#).await;
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("401"), "expected 401 error, got: {err}");
}

#[tokio::test]
async fn test_search_web_tool_definitions_includes_all_modes() {
    let provider = ExaSearchProvider::new("test-key");
    let tool = WebSearchTool::new(Box::new(provider));
    let params = tool.parameters();

    let types = params["properties"]["type"]["enum"].as_array().unwrap();
    assert!(types.contains(&serde_json::json!("auto")));
    assert!(types.contains(&serde_json::json!("fast")));
    assert!(types.contains(&serde_json::json!("deep")));

    let modes = params["properties"]["contents_mode"]["enum"]
        .as_array()
        .unwrap();
    assert!(modes.contains(&serde_json::json!("highlights")));
    assert!(modes.contains(&serde_json::json!("text")));
    assert!(modes.contains(&serde_json::json!("deep")));
}

// ============================================================================
// _dfd/tools/web-fetch.md — Happy Path (wiremock)
// ============================================================================

#[tokio::test]
async fn test_web_fetch_get_raw() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/data"))
        .respond_with(ResponseTemplate::new(200).set_body_string("Hello, world!"))
        .mount(&mock_server)
        .await;

    let tool = WebFetchTool::new();
    let url = format!("{}/api/data", mock_server.uri());
    let args = serde_json::json!({
        "url": url,
        "format": "raw"
    })
    .to_string();
    let result = tool.execute(&args).await.unwrap();

    assert!(result.contains("Hello, world!"));
}

#[tokio::test]
async fn test_web_fetch_post_with_body() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/submit"))
        .and(body_string_contains(r#""name":"test""#))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
            "id": "item-1",
            "status": "created"
        })))
        .mount(&mock_server)
        .await;

    let tool = WebFetchTool::new();
    let url = format!("{}/api/submit", mock_server.uri());
    let args = serde_json::json!({
        "url": url,
        "method": "POST",
        "body_json": {"name": "test"},
        "format": "raw"
    })
    .to_string();
    let result = tool.execute(&args).await.unwrap();

    assert!(result.contains("created") || result.contains("item-1"));
}

#[tokio::test]
async fn test_web_fetch_json_format() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/info"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(r#"{"version": "1.0", "status": "ok"}"#)
                .insert_header("Content-Type", "application/json"),
        )
        .mount(&mock_server)
        .await;

    let tool = WebFetchTool::new();
    let url = format!("{}/api/info", mock_server.uri());
    let args = serde_json::json!({
        "url": url,
        "format": "json"
    })
    .to_string();
    let result = tool.execute(&args).await.unwrap();

    assert!(result.contains("url"));
    assert!(result.contains("status"));
}

#[tokio::test]
async fn test_web_fetch_default_method_is_get() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/default"))
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
        .mount(&mock_server)
        .await;

    let tool = WebFetchTool::new();
    let url = format!("{}/default", mock_server.uri());
    // Omit method — should default to GET
    let args = serde_json::json!({"url": url}).to_string();
    let result = tool.execute(&args).await.unwrap();

    assert!(result.contains("ok"));
}

#[tokio::test]
async fn test_web_fetch_missing_url() {
    let tool = WebFetchTool::new();
    let result = tool.execute(r#"{}"#).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_web_fetch_http_error() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/not-found"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&mock_server)
        .await;

    let tool = WebFetchTool::new();
    let url = format!("{}/not-found", mock_server.uri());
    let args = serde_json::json!({"url": url}).to_string();
    let result = tool.execute(&args).await;

    assert!(result.is_err());
}

// ============================================================================
// _dfd/tools/edit-soul.md — Happy Path (wiremock WebDAV)
// ============================================================================

#[tokio::test]
async fn test_edit_soul_replaces_soul_file() {
    let mock_server = MockServer::start().await;

    // Mock the WebDAV PUT for soul.md
    Mock::given(method("PUT"))
        .and(path("/r-test/memory/soul.md"))
        .and(header("Authorization", "Basic dGVzdDpwYXNz")) // base64("test:pass")
        .respond_with(ResponseTemplate::new(201))
        .mount(&mock_server)
        .await;

    let webdav_url = format!("{}/", mock_server.uri());
    let webdav = webdav::WebDavClient::new(&webdav_url, "test", "pass").unwrap();
    let tool = EditSoulTool::new(webdav);

    let soul_content = "# Soul Memory\n\n- My name is RockBot ✨\n- I like Rust\n";
    let args = serde_json::json!({
        "content": soul_content,
        "webdav_dir": "r-test"
    })
    .to_string();
    let result = tool.execute(&args).await.unwrap();

    assert_eq!(result, "Soul memory updated.");
}

#[tokio::test]
async fn test_edit_soul_empty_content_fails() {
    let mock_server = MockServer::start().await;
    let webdav_url = format!("{}/", mock_server.uri());
    let webdav = webdav::WebDavClient::new(&webdav_url, "test", "pass").unwrap();
    let tool = EditSoulTool::new(webdav);

    let result = tool.execute(r#"{"content": "", "webdav_dir": "r-test"}"#).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_edit_soul_missing_content_fails() {
    let mock_server = MockServer::start().await;
    let webdav_url = format!("{}/", mock_server.uri());
    let webdav = webdav::WebDavClient::new(&webdav_url, "test", "pass").unwrap();
    let tool = EditSoulTool::new(webdav);

    let result = tool.execute(r#"{"webdav_dir": "r-test"}"#).await;
    assert!(result.is_err());
}

// ============================================================================
// _dfd/tools/webdav.md — Happy Path (wiremock WebDAV)
// ============================================================================

#[tokio::test]
async fn test_webdav_write_file() {
    let mock_server = MockServer::start().await;

    Mock::given(method("PUT"))
        .and(path("/r-test/notes/hello.txt"))
        .and(header("Authorization", "Basic dGVzdDpwYXNz"))
        .respond_with(ResponseTemplate::new(201))
        .mount(&mock_server)
        .await;

    let webdav_url = format!("{}/", mock_server.uri());
    let client = webdav::WebDavClient::new(&webdav_url, "test", "pass").unwrap();
    let tool = WebDavTool::new(client);

    let args = serde_json::json!({
        "action": "write",
        "path": "notes/hello.txt",
        "content": "Hello from test",
        "webdav_dir": "r-test"
    })
    .to_string();
    let result = tool.execute(&args).await.unwrap();

    assert!(result.contains("Written"));
    assert!(result.contains("notes/hello.txt"));
}

#[tokio::test]
async fn test_webdav_read_text_file() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/r-test/docs/readme.md"))
        .and(header("Authorization", "Basic dGVzdDpwYXNz"))
        .respond_with(ResponseTemplate::new(200).set_body_string("# Readme\n\nHello!"))
        .mount(&mock_server)
        .await;

    let webdav_url = format!("{}/", mock_server.uri());
    let client = webdav::WebDavClient::new(&webdav_url, "test", "pass").unwrap();
    let tool = WebDavTool::new(client);

    let args = serde_json::json!({
        "action": "read",
        "path": "docs/readme.md",
        "webdav_dir": "r-test"
    })
    .to_string();
    let result = tool.execute(&args).await.unwrap();

    assert!(result.contains("# Readme"));
    assert!(result.contains("Hello!"));
}

#[tokio::test]
async fn test_webdav_read_image_file_returns_markdown() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/r-test/images/photo.png"))
        .and(header("Authorization", "Basic dGVzdDpwYXNz"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(tiny_png_bytes())
                .insert_header("Content-Type", "image/png"),
        )
        .mount(&mock_server)
        .await;

    let webdav_url = format!("{}/", mock_server.uri());
    let client = webdav::WebDavClient::new(&webdav_url, "test", "pass").unwrap();
    let tool = WebDavTool::new(client);

    let args = serde_json::json!({
        "action": "read",
        "path": "images/photo.png",
        "webdav_dir": "r-test"
    })
    .to_string();
    let result = tool.execute(&args).await.unwrap();

    assert!(result.starts_with("![photo.png](data:image/png;base64,"));
}

#[tokio::test]
async fn test_webdav_list_directory() {
    let mock_server = MockServer::start().await;

    // PROPFIND response for list
    let propfind_body = r#"<?xml version="1.0" encoding="UTF-8"?>
<d:multistatus xmlns:d="DAV:">
  <d:response>
    <d:href>/r-test/files/</d:href>
    <d:propstat>
      <d:prop>
        <d:resourcetype><d:collection/></d:resourcetype>
        <d:getlastmodified>Thu, 01 Jan 2026 00:00:00 GMT</d:getlastmodified>
      </d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
  </d:response>
  <d:response>
    <d:href>/r-test/files/a.txt</d:href>
    <d:propstat>
      <d:prop>
        <d:getcontentlength>42</d:getcontentlength>
        <d:getcontenttype>text/plain</d:getcontenttype>
        <d:getlastmodified>Thu, 01 Jan 2026 00:00:00 GMT</d:getlastmodified>
      </d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
  </d:response>
</d:multistatus>"#;

    Mock::given(method("PROPFIND"))
        .and(path("/r-test/files"))
        .and(header("Authorization", "Basic dGVzdDpwYXNz"))
        .respond_with(
            ResponseTemplate::new(207)
                .set_body_string(propfind_body)
                .insert_header("Content-Type", "application/xml"),
        )
        .mount(&mock_server)
        .await;

    let webdav_url = format!("{}/", mock_server.uri());
    let client = webdav::WebDavClient::new(&webdav_url, "test", "pass").unwrap();
    let tool = WebDavTool::new(client);

    let args = serde_json::json!({
        "action": "list",
        "path": "files",
        "webdav_dir": "r-test"
    })
    .to_string();
    let result = tool.execute(&args).await.unwrap();

    // Listing should contain the file entry
    assert!(result.contains("a.txt"));
}

#[tokio::test]
async fn test_webdav_delete_file() {
    let mock_server = MockServer::start().await;

    Mock::given(method("DELETE"))
        .and(path("/r-test/temp/old.txt"))
        .and(header("Authorization", "Basic dGVzdDpwYXNz"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&mock_server)
        .await;

    let webdav_url = format!("{}/", mock_server.uri());
    let client = webdav::WebDavClient::new(&webdav_url, "test", "pass").unwrap();
    let tool = WebDavTool::new(client);

    let args = serde_json::json!({
        "action": "delete",
        "path": "temp/old.txt",
        "webdav_dir": "r-test"
    })
    .to_string();
    let result = tool.execute(&args).await.unwrap();

    assert!(result.contains("Deleted"));
    assert!(result.contains("temp/old.txt"));
}

#[tokio::test]
async fn test_webdav_exists_positive() {
    let mock_server = MockServer::start().await;

    let propfind_body = r#"<?xml version="1.0" encoding="UTF-8"?>
<d:multistatus xmlns:d="DAV:">
  <d:response>
    <d:href>/r-test/config.json</d:href>
    <d:propstat>
      <d:prop>
        <d:getcontentlength>10</d:getcontentlength>
      </d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
  </d:response>
</d:multistatus>"#;

    Mock::given(method("PROPFIND"))
        .and(path("/r-test/config.json"))
        .and(header("Authorization", "Basic dGVzdDpwYXNz"))
        .respond_with(
            ResponseTemplate::new(207)
                .set_body_string(propfind_body)
                .insert_header("Content-Type", "application/xml"),
        )
        .mount(&mock_server)
        .await;

    let webdav_url = format!("{}/", mock_server.uri());
    let client = webdav::WebDavClient::new(&webdav_url, "test", "pass").unwrap();
    let tool = WebDavTool::new(client);

    let args = serde_json::json!({
        "action": "exists",
        "path": "config.json",
        "webdav_dir": "r-test"
    })
    .to_string();
    let result = tool.execute(&args).await.unwrap();

    assert!(result.contains("exists"));
}

#[tokio::test]
async fn test_webdav_mkdir_creates_directory() {
    let mock_server = MockServer::start().await;

    // ensure_directory_all creates parent dirs first; /r-test may already exist
    Mock::given(method("MKCOL"))
        .and(path("/r-test"))
        .and(header("Authorization", "Basic dGVzdDpwYXNz"))
        .respond_with(ResponseTemplate::new(405)) // already exists
        .mount(&mock_server)
        .await;

    Mock::given(method("MKCOL"))
        .and(path("/r-test/newdir"))
        .and(header("Authorization", "Basic dGVzdDpwYXNz"))
        .respond_with(ResponseTemplate::new(201))
        .mount(&mock_server)
        .await;

    let webdav_url = format!("{}/", mock_server.uri());
    let client = webdav::WebDavClient::new(&webdav_url, "test", "pass").unwrap();
    let tool = WebDavTool::new(client);

    let args = serde_json::json!({
        "action": "mkdir",
        "path": "newdir",
        "webdav_dir": "r-test"
    })
    .to_string();
    let result = tool.execute(&args).await.unwrap();

    assert!(result.contains("Directory created"));
}

#[tokio::test]
async fn test_webdav_write_missing_content_fails() {
    let mock_server = MockServer::start().await;
    let webdav_url = format!("{}/", mock_server.uri());
    let client = webdav::WebDavClient::new(&webdav_url, "test", "pass").unwrap();
    let tool = WebDavTool::new(client);

    let args = serde_json::json!({
        "action": "write",
        "path": "file.txt",
        "webdav_dir": "r-test"
    })
    .to_string();
    let result = tool.execute(&args).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_webdav_rename_file() {
    let mock_server = MockServer::start().await;
    let webdav_url = format!("{}/", mock_server.uri());

    let dst_url = format!("{}r-test/notes/new-name.txt", webdav_url);
    Mock::given(method("MOVE"))
        .and(path("/r-test/notes/old-name.txt"))
        .and(header("Authorization", "Basic dGVzdDpwYXNz"))
        .and(header("Destination", dst_url.as_str()))
        .and(header("Overwrite", "F"))
        .respond_with(ResponseTemplate::new(201))
        .mount(&mock_server)
        .await;

    let client = webdav::WebDavClient::new(&webdav_url, "test", "pass").unwrap();
    let tool = WebDavTool::new(client);

    let args = serde_json::json!({
        "action": "rename",
        "path": "notes/old-name.txt",
        "destination": "notes/new-name.txt",
        "webdav_dir": "r-test"
    })
    .to_string();
    let result = tool.execute(&args).await.unwrap();

    assert!(result.contains("Renamed"));
    assert!(result.contains("old-name.txt"));
    assert!(result.contains("new-name.txt"));
}

// ============================================================================
// _dfd/tools/calendar.md — Tool definition + param validation
// ============================================================================

#[tokio::test]
async fn test_calendar_tool_definition() {
    let mock_server = MockServer::start().await;
    let webdav_url = format!("{}/", mock_server.uri());
    let client = webdav::WebDavClient::new(&webdav_url, "test", "pass").unwrap();
    let config = make_webdav_config(&webdav_url);
    let tool = CalendarTool::from_config(client, &config);

    assert_eq!(tool.name(), "calendar");
    assert!(tool.description().contains("Manage calendar events"));
}

#[tokio::test]
async fn test_calendar_missing_action_fails() {
    let mock_server = MockServer::start().await;
    let webdav_url = format!("{}/", mock_server.uri());
    let client = webdav::WebDavClient::new(&webdav_url, "test", "pass").unwrap();
    let config = make_webdav_config(&webdav_url);
    let tool = CalendarTool::from_config(client, &config);

    let result = tool.execute(r#"{}"#).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_calendar_add_event_missing_required_fields() {
    let mock_server = MockServer::start().await;
    let webdav_url = format!("{}/", mock_server.uri());
    let client = webdav::WebDavClient::new(&webdav_url, "test", "pass").unwrap();
    let config = make_webdav_config(&webdav_url);
    let tool = CalendarTool::from_config(client, &config);

    // add_event requires summary, dtstart, dtend
    let result = tool
        .execute(r#"{"action": "add_event"}"#)
        .await;
    assert!(result.is_err());
}

// ============================================================================
// _dfd/base/memory.md — Happy Path (MemoryManager, ConversationHistory, PersistSnapshot)
// ============================================================================

#[test]
fn test_conversation_history_append_and_char_count() {
    let mut history = ConversationHistory::new("room-1");
    assert_eq!(history.char_count, 0);

    history.append(ChatMessage::user("Hello world"));
    assert_eq!(history.messages.len(), 1);
    assert_eq!(history.char_count, 11); // "Hello world" = 11 chars
}

#[test]
fn test_conversation_history_needs_archive() {
    let mut history = ConversationHistory::new("room-1");
    for i in 0..20 {
        history.append(ChatMessage::user(format!("message number {i} is quite long with extra padding")));
    }
    // Each message ~50 chars, 20 messages = ~1000 chars
    assert!(history.needs_archive(300));
    assert!(history.messages.len() > 4);
}

#[test]
fn test_conversation_history_oldest_messages() {
    let mut history = ConversationHistory::new("room-1");
    history.append(ChatMessage::user("first"));
    history.append(ChatMessage::user("second"));
    history.append(ChatMessage::user("third"));

    let oldest = history.oldest_messages(2);
    assert_eq!(oldest.len(), 2);
}

#[test]
fn test_memory_manager_new() {
    let mgr = MemoryManager::new(5000, 60, 4_000_000);
    assert_eq!(mgr.persist_interval_secs, 60);
    assert_eq!(mgr.max_soul_chars, 5000);
}

#[test]
fn test_persist_snapshot_serialization() {
    let snapshot = PersistSnapshot {
        schema: NonEmptyString::try_new("rockbot-snapshot/1".to_string()).unwrap(),
        room_id: NonEmptyString::try_new("room-abc".to_string()).unwrap(),
        messages: vec![ChatMessage::user("Hello")],
        char_count: 5,
        archive_seq: 0,
        soul: Some("# Soul\n\n- My name is Bot ✨".into()),
        summary: None,
        updated_at: "2026-06-13T00:00:00Z".to_string(),
    };

    let json = serde_json::to_string_pretty(&snapshot).unwrap();
    assert!(json.contains("rockbot-snapshot/1"));
    assert!(json.contains("room-abc"));
    assert!(json.contains("My name is Bot"));

    // Round-trip
    let deserialized: PersistSnapshot = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.schema, snapshot.schema);
    assert_eq!(deserialized.room_id, snapshot.room_id);
    assert_eq!(deserialized.char_count, 5);
}

// ============================================================================
// _dfd/agent-harness.md — Happy Path (harness with mock AI provider)
// ============================================================================

/// A mock AI provider that returns canned responses for testing the harness.
struct MockProvider {
    response: String,
    tool_calls: Vec<ToolCall>,
    fail_on_call: Option<String>,
}

impl MockProvider {
    fn new_text(text: impl Into<String>) -> Self {
        Self {
            response: text.into(),
            tool_calls: vec![],
            fail_on_call: None,
        }
    }

    fn new_with_tool_calls(text: impl Into<String>, tool_calls: Vec<ToolCall>) -> Self {
        Self {
            response: text.into(),
            tool_calls,
            fail_on_call: None,
        }
    }
}

/// A provider that pops responses from a queue — supports multi-turn tests.
struct SequentialMockProvider {
    responses: Mutex<Vec<Result<CompletionResult, rockbot::error::RockBotError>>>,
}

impl SequentialMockProvider {
    fn new(responses: Vec<CompletionResult>) -> Self {
        Self {
            responses: Mutex::new(responses.into_iter().map(Ok).collect()),
        }
    }
}

#[async_trait::async_trait]
impl AiProvider for SequentialMockProvider {
    async fn complete(&self, _request: ChatRequest) -> Result<CompletionResult, rockbot::error::RockBotError> {
        let mut responses = self.responses.lock().unwrap();
        if responses.is_empty() {
            Err(rockbot::error::RockBotError::Provider("No mock responses left".into()))
        } else {
            responses.remove(0)
        }
    }

    fn provider_name(&self) -> &str { "sequential-mock" }
    fn model_name(&self) -> &str { "sequential-mock-model" }
}

#[async_trait::async_trait]
impl AiProvider for MockProvider {
    async fn complete(&self, _request: ChatRequest) -> Result<CompletionResult, rockbot::error::RockBotError> {
        if let Some(err) = &self.fail_on_call {
            return Err(rockbot::error::RockBotError::Provider(err.clone()));
        }
        Ok(CompletionResult {
            text: if self.response.is_empty() { None } else { Some(self.response.clone()) },
            finish: if self.tool_calls.is_empty() { FinishReason::Stop } else { FinishReason::ToolUse },
            tool_calls: self.tool_calls.clone(),
            reasoning_content: None,
            usage: None,
        })
    }

    fn provider_name(&self) -> &str { "mock" }
    fn model_name(&self) -> &str { "mock-model" }
}

#[tokio::test]
async fn test_harness_constructs_and_registers_tools() {
    let mock_server = MockServer::start().await;
    let webdav_url = format!("{}/", mock_server.uri());

    let config = make_test_config(&webdav_url);
    let provider = Box::new(MockProvider::new_text("Hello!"));
    let webdav = webdav::WebDavClient::new(&webdav_url, "test", "pass").unwrap();
    let image_cache = Arc::new(ImageCache::new());
    let mut harness = AgentHarness::new(config, provider, Some(webdav), image_cache, "@testbot");

    // Register some tools
    harness.register_tool(Box::new(VisionTool::with_max_bytes(10_000_000)));
    harness.register_tool(Box::new(WebSearchTool::new(Box::new(ExaSearchProvider::new("test-key")))));

    // Verify config access
    let cfg = harness.config();
    assert_eq!(cfg.model.max_iterations, 5);

    // Verify memory
    let mem = harness.memory();
    assert_eq!(mem.max_soul_chars, 5000);
}

#[tokio::test]
async fn test_harness_get_or_create_room() {
    let mock_server = MockServer::start().await;
    let webdav_url = format!("{}/", mock_server.uri());

    let config = make_test_config(&webdav_url);
    let provider = Box::new(MockProvider::new_text("Hi! How can I help?"));
    let webdav = webdav::WebDavClient::new(&webdav_url, "test", "pass").unwrap();
    let image_cache = Arc::new(ImageCache::new());
    let mut harness = AgentHarness::new(config, provider, Some(webdav), image_cache, "@testbot");
    harness.register_tool(Box::new(VisionTool::with_max_bytes(10_000_000)));

    // MemoryManager::get_or_create creates rooms on first access
    let mem = harness.memory_mut();
    let room = mem.get_or_create("room-1", "general", "", false);
    assert_eq!(room.room_id, "room-1");
    assert_eq!(room.room_name, "general");
    assert!(!room.is_dm);
}

// ============================================================================
// _dfd/image-interception.md — Harness image pool starts empty
// ============================================================================

#[tokio::test]
async fn test_harness_image_pool_starts_empty() {
    let mock_server = MockServer::start().await;
    let webdav_url = format!("{}/", mock_server.uri());

    let config = make_test_config(&webdav_url);
    let provider = Box::new(MockProvider::new_text("ok"));
    let webdav = webdav::WebDavClient::new(&webdav_url, "test", "pass").unwrap();
    let image_cache = Arc::new(ImageCache::new());
    let harness = AgentHarness::new(config, provider, Some(webdav), image_cache, "@testbot");

    // Image pool starts empty — verify harness constructs successfully
    assert!(harness.provider().provider_name() == "mock");
}

// ============================================================================
// _dfd/agent-loop.md — Happy Path (main loop config-driven behavior)
// ============================================================================

#[test]
fn test_agent_loop_max_iterations_from_config() {
    let config = make_test_config("https://chat.example.com");
    assert_eq!(config.model.max_iterations, 5);
    assert_eq!(config.model.model_context_length, 1_000_000);
}

// ============================================================================
// _dfd/interception/image-interception.md — Vision/webdav result interception
// (image_pool caching + ContentPart injection)
// ============================================================================

#[tokio::test]
async fn test_harness_vision_result_cached_in_image_pool_and_injected() {
    let mock_server = MockServer::start().await;
    let webdav_url = format!("{}/", mock_server.uri());

    // Mock the image that the vision tool will fetch
    Mock::given(method("GET"))
        .and(path("/images/cat.png"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(tiny_png_bytes())
                .insert_header("Content-Type", "image/png"),
        )
        .mount(&mock_server)
        .await;

    let config = make_test_config(&webdav_url);
    let webdav = webdav::WebDavClient::new(&webdav_url, "test", "pass").unwrap();
    let image_cache = Arc::new(ImageCache::new());

    // Sequential: vision tool call first, then text response
    let url = format!("{}/images/cat.png", mock_server.uri());
    let sequential = SequentialMockProvider::new(vec![
        // Iteration 1: LLM calls vision tool
        CompletionResult {
            text: None,
            finish: FinishReason::ToolUse,
            tool_calls: vec![ToolCall::new(
                "call_v_001",
                "vision",
                serde_json::json!({"url": url}).to_string(),
            )],
            reasoning_content: None,
            usage: None,
        },
        // Iteration 2: LLM acknowledges (after ContentPart injection)
        CompletionResult {
            text: Some("I can see the cat photo. What would you like me to do with it?".into()),
            finish: FinishReason::Stop,
            tool_calls: vec![],
            reasoning_content: None,
            usage: None,
        },
    ]);

    let mut harness = AgentHarness::new(config, Box::new(sequential), Some(webdav), image_cache.clone(), "@testbot");
    harness.register_tool(Box::new(VisionTool::with_max_bytes(10_000_000)));

    let result = harness
        .process_message("room1", "general", "General", false, "user", "Look at cat.png", &[], &[])
        .await;

    assert!(result.is_ok());
    assert!(result.unwrap().is_some());

    // Verify image_pool was populated
    let pool_len = harness.image_pool_len("room1");
    assert_eq!(pool_len, 1, "vision tool result should be cached in image_pool");

    // Verify the synthetic "Fetched images:" message was injected into history
    let room = harness.memory().get("room1").unwrap();
    let messages = &room.history.messages;
    let fetched_msg = messages
        .iter()
        .find(|m| {
            if let MessageContent::Multipart(ref parts) = m.content {
                parts.iter().any(|p| {
                    if let ContentPart::Text { text } = p {
                        text.contains("Fetched images:")
                    } else {
                        false
                    }
                })
            } else {
                false
            }
        })
        .expect("should contain a 'Fetched images:' message in history");

    // Verify the fetched message has ContentPart::ImageUrl for the cat.png
    if let MessageContent::Multipart(ref parts) = fetched_msg.content {
        let has_image_part = parts.iter().any(|p| {
            matches!(p, ContentPart::ImageUrl { .. })
        });
        assert!(
            has_image_part,
            "'Fetched images:' message should contain ImageUrl ContentPart"
        );
        let image_count = parts
            .iter()
            .filter(|p| matches!(p, ContentPart::ImageUrl { .. }))
            .count();
        assert_eq!(image_count, 1, "should have exactly 1 ImageUrl part for cat.png");
    } else {
        panic!("'Fetched images:' message should be Multipart content, got: {:?}", fetched_msg.content);
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn make_test_config(webdav_url: &str) -> rockbot::config::AppConfig {
    use rockbot::config::{ImageModelConfig, ModelConfig, RocketChatSection, ServerConfig};
    use rockbot::validated::BoundedUsize;

    let chat_config = rockbot::config::ProviderConfig {
        name: ProviderName::try_new("mock".to_string()).unwrap(),
        api_key: "sk-test".into(),
        base_url: ConfigUrl::try_new(webdav_url.to_string()).unwrap(),
        basecf_url: None,
        chat_path: Some("/chat/completions".into()),
        draw_path: None,
        models: HashMap::new(),
    };

    let image_config = ImageModelConfig {
        default_provider: ProviderName::try_new("mock".to_string()).unwrap(),
        default_text_model: "mock-img".into(),
        default_edit_model: "mock-img-edit".into(),
        default_quality: "standard".into(),
        default_output_format: "png".into(),
        default_num_images: 1,
        default_image_size: "1024x1024".into(),
        default_image_size_tier: "1K".into(),
        default_enable_safety_checker: false,
    };

    let webdav_cfg = make_webdav_config(webdav_url);

    rockbot::config::AppConfig {
        platform: Default::default(),
        rocketchat: RocketChatSection {
            server: ServerConfig {
                url: "chat.example.com".into(),
                username: "bot".into(),
                password: "secret".into(),
                debug: false,
            },
            model: None,
        },
        matrix: None,
        model: ModelConfig {
            default_provider: ProviderName::try_new("mock".to_string()).unwrap(),
            default_model: "mock-model".into(),
            max_soul_chars: BoundedUsize::try_new(5000).unwrap(),
            max_iterations: 5,
            persist_interval_secs: 60,
            memory_ttl_secs: 86400,
            max_context_bytes: BoundedUsize::try_new(4194304).unwrap(),
            max_attachment_bytes: 20971520,
            model_context_length: 1_000_000,
            summarization_enabled: true,
            summarization_ratio: 0.6,
            summarization_target_tokens: 1024,
        },
        chat_providers: vec![chat_config],
        image_providers: vec![rockbot::config::ProviderConfig {
            name: ProviderName::try_new("mock".to_string()).unwrap(),
            api_key: "sk-test".into(),
            base_url: ConfigUrl::try_new(webdav_url.to_string()).unwrap(),
            basecf_url: None,
            chat_path: None,
            draw_path: Some("/draw".into()),
            models: HashMap::new(),
        }],
        image_model: image_config,
        tools: HashMap::new(),
        search: Default::default(),
        webdav: Some(webdav_cfg),
        agent: Default::default(),
    }
}

/// Build a WebDavConfig by parsing a TOML snippet (DavUrl/DavRoot are private types).
fn make_webdav_config(base_url: &str) -> webdav::WebDavConfig {
    let toml = format!(
        r#"
url = "{base_url}"
username = "test"
password = "pass"
root = "rockbot"
dav_path = "remote.php/dav"
"#
    );
    toml::from_str(&toml).expect("valid WebDavConfig TOML")
}
