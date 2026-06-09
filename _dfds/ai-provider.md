# AI Provider

## 1. Purpose

Configurable `AiProvider` trait abstracting over OpenAI-compatible chat
completion APIs. Concrete implementations for OpenRouter and DeepSeek handle
provider-specific headers, model naming, and vision payload formatting. Supports
streaming responses and tool/function calling.

- Upstream: [Configuration Management](config.md) provides `AiConfig`
- Downstream: [Agent Loop](agent-harness.md) calls `complete()` with `ChatRequest`
  (message history + tool definitions) and receives `CompletionResult`

## 2. Diagram

### 2a. Happy Flow (Main Success Path)

```mermaid
flowchart TD
    AGENT[Agent]
    BUILD(BuildRequest)
    ROUTE(RouteProvider)
    SELECT{SelectProvider}
    OPENROUTER(OpenRouter)
    DEEPSEEK(DeepSeek)
    HTTP(SendHttpRequest)
    PARSE(ParseResponse)
    PROVIDER_API[Provider HTTP API]

    AGENT -->|"ChatRequest"| BUILD
    BUILD -->|"ProviderRequest"| ROUTE
    ROUTE -->|"provider == openrouter"| OPENROUTER
    ROUTE -->|"provider == deepseek"| DEEPSEEK
    OPENROUTER -->|"reqwest::Request"| HTTP
    DEEPSEEK -->|"reqwest::Request"| HTTP
    HTTP -->|"HTTP POST"| PROVIDER_API
    PROVIDER_API -->|"JSON response body"| HTTP
    HTTP -->|"raw bytes"| PARSE
    PARSE -->|"text + tool_calls"| AGENT
```

### 2b. Error Handling & Fallbacks

```mermaid
flowchart TD
    HTTP(SendHttpRequest)
    PARSE(ParseResponse)
    RATE(RateLimitBackoff)
    RETRY(RetryWithBackoff)
    REPORT_API(ReportApiError)
    REPORT_PARSE(ReportParseError)
    REPORT_AUTH(ReportAuthError)
    AGENT[Agent Loop]

    HTTP -->|"429 Too Many Requests"| RATE
    RATE -->|"wait + retry"| RETRY
    HTTP -->|"5xx Server Error"| RETRY
    RETRY -->|"max retries exceeded"| REPORT_API
    HTTP -->|"401 Unauthorized"| REPORT_AUTH
    PARSE -->|"invalid JSON"| REPORT_PARSE
    REPORT_API -->|"API unreachable"| AGENT
    REPORT_AUTH -->|"invalid API key"| AGENT
    REPORT_PARSE -->|"malformed response"| AGENT
```

### 2c. Vision Payload Deep Dive

```mermaid
flowchart TD
    AGENT[Agent]
    CHECK(CheckContentType)
    TEXT_ONLY(FormatTextContent)
    MULTI(FormatMultipartContent)
    BUILD_URL(BuildImageUrl)
    BUILD_B64(BuildImageBase64)
    REQUEST[(ChatRequest)]

    AGENT -->|"ChatMessage"| CHECK
    CHECK -->|"text only"| TEXT_ONLY
    CHECK -->|"text + image"| MULTI
    CHECK -->|"image from WebDAV"| BUILD_B64
    TEXT_ONLY -->|"content string"| REQUEST
    MULTI -->|"content array"| REQUEST
    BUILD_URL -->|"{type: image_url, url}"| MULTI
    BUILD_B64 -->|"{type: image_url, base64}"| MULTI
```

## 3. Data Structures

#### `ChatRequest`

| Field      | Type              | Notes                                    |
| ---------- | ----------------- | ---------------------------------------- |
| `messages` | `Vec<ChatMessage>`| Conversation history                     |
| `tools`    | `Vec<ToolDef>`    | Available tool/function definitions      |
| `stream`   | `bool`            | Enable streaming response                |

#### `ChatMessage`

| Field     | Type               | Notes                                    |
| --------- | ------------------ | ---------------------------------------- |
| `role`    | `Role`             | `System`, `User`, `Assistant`, `Tool`    |
| `content` | `MessageContent`   | Text or multipart (text + images)        |
| `name`    | `Option<String>`   | Tool result name                         |
| `tool_calls` | `Option<Vec<ToolCall>>` | Assistant tool call requests      |

#### `MessageContent`

| Variant     | Fields                        | Notes                          |
| ----------- | ----------------------------- | ------------------------------ |
| `Text`      | `String`                      | Plain text content             |
| `Multipart` | `Vec<ContentPart>`            | Mixed text and images          |

#### `ContentPart`

| Variant    | Fields                          | Notes                         |
| ---------- | ------------------------------- | ----------------------------- |
| `Text`     | `String`                        | Text segment                  |
| `ImageUrl` | `{ url: String, detail: String }` | Remote or `data:` base64 URL |

#### `CompletionResult`

| Field        | Type                  | Notes                                |
| ------------ | --------------------- | ------------------------------------ |
| `text`       | `Option<String>`      | Assistant text response              |
| `tool_calls` | `Vec<ToolCall>`       | Tool/function calls requested by LLM |
| `finish`     | `FinishReason`        | `Stop`, `ToolUse`, `Length`, `Error` |

#### `ToolCall`

| Field      | Type     | Notes                                      |
| ---------- | -------- | ------------------------------------------ |
| `id`       | `String` | Provider-assigned call ID                  |
| `name`     | `String` | Tool/function name                         |
| `arguments`| `String` | JSON-encoded arguments                     |

#### `ToolDef`

| Field        | Type     | Notes                                   |
| ------------ | -------- | --------------------------------------- |
| `name`       | `String` | Function name                           |
| `description`| `String` | Human-readable description for the LLM  |
| `parameters` | `Value`  | JSON Schema for arguments               |
