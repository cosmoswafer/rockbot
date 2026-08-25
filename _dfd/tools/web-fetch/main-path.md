# Web Fetch

## 1. Purpose

Acts as a curl-like HTTP client: fetches content from arbitrary URLs with
customizable HTTP method, headers, and body. Supports JSON request bodies,
reading request bodies from WebDAV files, and saving response bodies to WebDAV.
Three output formats — `raw` (unmodified response body), `markdown` (HTML-to-markdown
conversion for AI consumption), and `json` (structured metadata with content).
Optionally cross-verifies fetched content via a parallel Exa web search.

This enables managing external APIs like Gitea, GitHub, or any REST API directly
from chat — create issues, query resources, or interact with webhooks.

- Upstream: [Search Web](../search-web/main-path.md) provides the verification search when
  `verify` is enabled and an Exa API key is configured
- Upstream: [Configuration Management](../../infra/config/main-path.md) supplies the
  Exa API key (from `[search.exa]` or legacy `[tools.exa]`) for the optional verify flow
- Upstream: [Agent Harness](../../agent/agent-harness/tool-dispatch.md) invokes web_fetch as a tool
  during the agent loop, passing a URL and format selector
- Upstream: [WebDAV Tool](../webdav/main-path.md) provides file read/write for `file_from_webdav`
  and `save_to_webdav` body source/sink
- Upstream: [Secret Interception](../../interception/secret-interception/uuidv5-scoped-injection.md) transparently
  replaces `secret:<key>` references in header values with actual secrets
  from `secrets.toml` on WebDAV before the tool sees the arguments
- Downstream: [AI Provider](../../ai/ai-provider/main-path.md) consumes the returned content
  (plain text, markdown, or structured JSON) as context for chat completions

## 2. Diagram

```mermaid
flowchart TD
    AGENT[Agent Harness]
    CFG[(ToolsConfig)]
    FETCH(FetchUrl)
    BUILD_REQ(BuildRequest)
    HTTP[HTTP Client]
    SERVER[(Web Server)]
    DAV[(NextCloud WebDAV)]
    MODE(SelectOutputMode)
    MD_CONV(ConvertHtmlToMarkdown)
    JSON_FMT(FormatJsonOutput)
    RAW_OUT(PassThroughRaw)
    SAVE_DAV(SaveToWebDav)
    AI[AiProvider]

    AGENT -->|"url + method + headers + body"| FETCH
    CFG -->|"Exa API key (optional)"| FETCH
    FETCH -->|"params"| BUILD_REQ
    DAV -->|"file content"| BUILD_REQ
    BUILD_REQ -->|"GET/POST/PUT/PATCH/DELETE"| HTTP
    HTTP -->|"response body + headers"| SERVER
    SERVER -->|"html / json / text"| MODE
    MODE -->|"format=markdown + text/html"| MD_CONV
    MODE -->|"format=json"| JSON_FMT
    MODE -->|"format=raw"| RAW_OUT
    MODE -->|"save_to_webdav"| SAVE_DAV
    SAVE_DAV -->|"write file"| DAV
    MD_CONV -->|"markdown text"| AI
    JSON_FMT -->|"structured json"| AI
    RAW_OUT -->|"raw response"| AI
```

## 3. Data Structures

Shared with [structures.md](structures.md).
