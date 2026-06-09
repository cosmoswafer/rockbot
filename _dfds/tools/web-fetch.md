# Web Fetch

## 1. Purpose

Fetches content from arbitrary URLs with three output formats — `raw` (unmodified
response body), `markdown` (HTML-to-markdown conversion for AI consumption), and
`json` (structured metadata with content). Optionally cross-verifies fetched
content via a parallel Exa web search.

- Upstream: [Exa Search](exa-search.md) provides the verification search when
  `verify` is enabled and an Exa API key is configured
- Upstream: [Configuration Management](../base/config.md) supplies the
  `exa_api_key` for the optional verify flow
- Upstream: [Agent Harness](../agent-harness.md) invokes web_fetch as a tool
  during the agent loop, passing a URL and format selector
- Downstream: [AI Provider](../base/ai-provider.md) consumes the returned content
  (plain text, markdown, or structured JSON) as context for chat completions

## 2. Diagram

### 2a. Happy Flow (Main Success Path)

```mermaid
flowchart TD
    AGENT[Agent Harness]
    CFG[(ToolsConfig)]
    FETCH(FetchUrl)
    HTTP[HTTP GET]
    SERVER[(Web Server)]
    MODE(SelectOutputMode)
    MD_CONV(ConvertHtmlToMarkdown)
    JSON_FMT(FormatJsonOutput)
    RAW_OUT(PassThroughRaw)
    AI[AiProvider]

    AGENT -->|"url + format param"| FETCH
    CFG -->|"exa_api_key (optional)"| FETCH
    FETCH -->|"GET request"| HTTP
    HTTP -->|"response body + headers"| SERVER
    SERVER -->|"html / json / text"| MODE
    MODE -->|"format=markdown + text/html"| MD_CONV
    MODE -->|"format=json"| JSON_FMT
    MODE -->|"format=raw"| RAW_OUT
    MD_CONV -->|"markdown text"| AI
    JSON_FMT -->|"structured json"| AI
    RAW_OUT -->|"raw response"| AI
```

### 2b. Error Handling & Fallbacks

```mermaid
flowchart TD
    FETCH(FetchUrl)
    HTTP[HTTP GET]
    SERVER[(Web Server)]
    TIMEOUT[Error: Request Timeout]
    NON200[Error: HTTP 4xx/5xx]
    PARSE_ERR[Error: Non-UTF8 Body]
    AGENT[Agent Harness]

    FETCH -.->|"30s elapsed"| TIMEOUT
    HTTP -.->|"!200 status"| NON200
    SERVER -.->|"binary / non-text content"| PARSE_ERR
    TIMEOUT -->|"error string"| AGENT
    NON200 -->|"error with status code"| AGENT
    PARSE_ERR -->|"content-type warning"| AGENT
```

### 2c. Verify Deep Dive (Double-Check)

```mermaid
flowchart TD
    FETCH(FetchUrl)
    MODE(SelectOutputMode)
    EXA[Exa Search API]
    PARSE_TITLE(ExtractPageTitle)
    SEARCH(SearchRelated)
    MERGE(MergeResults)
    OUTPUT(OutputWithSources)

    FETCH -->|"response html"| PARSE_TITLE
    PARSE_TITLE -->|"page title / domain"| SEARCH
    SEARCH -->|"POST /search (query=title)"| EXA
    EXA -->|"related results"| MERGE
    FETCH -->|"primary content"| MERGE
    MERGE -->|"content + related sources"| OUTPUT
    OUTPUT -->|"verified output"| MODE
```

When `verify` is `true` and the tool holds a valid Exa API key, the fetched
page title is extracted and used as a query to the Exa search API. The resulting
related sources are bundled alongside the primary content, giving the AI provider
cross-referenced information for fact-checking.

## 3. Data Structures

### `FetchParams`

| Field    | Type     | Notes                                          |
| -------- | -------- | ---------------------------------------------- |
| `url`    | `String` | The URL to fetch (required)                    |
| `format` | `String` | Output format: `"json"`, `"markdown"`, or `"raw"` (default: `"raw"`) |
| `verify` | `bool`   | Trigger a parallel Exa search for cross-referencing (default: `false`) |

### `FetchJsonOutput` (format=`"json"`)

| Field             | Type              | Notes                                         |
| ----------------- | ----------------- | --------------------------------------------- |
| `url`             | `String`          | The requested URL                             |
| `status`          | `u16`             | HTTP status code                              |
| `content_type`    | `String`          | Content-Type header value                     |
| `content`         | `String`          | Response body (truncated to 10,000 chars)     |
| `verified`        | `bool`            | Whether cross-verification was performed      |
| `related_sources` | `Vec<SearchRef>`  | Results from the Exa verification search      |

### `SearchRef`

| Field    | Type     | Notes              |
| -------- | -------- | ------------------ |
| `title`  | `String` | Page title         |
| `url`    | `String` | Page URL           |
| `snippet`| `String` | Search snippet     |
