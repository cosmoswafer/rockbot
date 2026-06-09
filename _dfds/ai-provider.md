# AI Provider

## 1. Purpose

Configurable `AiProvider` trait abstracting over OpenAI-compatible chat
completion APIs. Concrete implementations for OpenRouter and DeepSeek handle
provider-specific headers, model naming, and vision payload formatting. Supports
streaming responses and tool/function calling.

- Upstream: [Configuration Management](config.md) provides `AiConfig`
- Upstream: [Agent Harness](agent-harness.md) selects the provider via
  `AppConfig` on startup
- Downstream: [Agent Orchestration](agent.md) calls `complete()` with message
  history and tool definitions

## 2. Diagram

### 2a. Happy Flow (Main Success Path)

```mermaid
flowchart TD
    AGENT[Agent]
    BUILD(BuildRequest)
    ROUTE(RouteProvider)
    PROVIDER{AiProvider trait}
    OPENROUTER(OpenRouter)
    DEEPSEEK(DeepSeek)
    HTTP(SendHttpRequest)
    PARSE(ParseResponse)
    AGENT_OUT[CompletionResult]
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
    PARSE -->|"CompletionResult"| AGENT_OUT
    AGENT_OUT -->|"text + tool_calls"| AGENT
```

### 2b. Error Handling & Fallbacks

```mermaid
flowchart TD
    HTTP(SendHttpRequest)
    PARSE(ParseResponse)
    RATE(RateLimitBackoff)
    RETRY(RetryWithBackoff)
    ERR_API[Error: API Unreachable]
    ERR_PARSE[Error: Malformed Response]
    ERR_AUTH[Error: Invalid API Key]
    AGENT[Agent]

    HTTP -->|"429 Too Many Requests"| RATE
    RATE -->|"wait + retry"| RETRY
    HTTP -->|"5xx Server Error"| RETRY
    RETRY -->|"max retries exceeded"| ERR_API
    HTTP -->|"401 Unauthorized"| ERR_AUTH
    PARSE -->|"invalid JSON"| ERR_PARSE
    ERR_API -->|"error"| AGENT
    ERR_AUTH -->|"error"| AGENT
    ERR_PARSE -->|"error"| AGENT
```

### 2c. Vision Payload Deep Dive

```mermaid
flowchart TD
    MSG[ChatMessage]
    CHECK(CheckContentType)
    TEXT_ONLY(FormatTextContent)
    MULTI(FormatMultipartContent)
    IMG_URL(ImageUrlPart)
    IMG_B64(ImageBase64Part)
    REQ[ProviderRequest]

    MSG -->|"message"| CHECK
    CHECK -->|"text only"| TEXT_ONLY
    CHECK -->|"text + image"| MULTI
    CHECK -->|"image from WebDAV"| IMG_B64
    TEXT_ONLY -->|"content string"| REQ
    MULTI -->|"content array"| REQ
    IMG_URL -->|"{type: image_url, url}"| MULTI
    IMG_B64 -->|"{type: image_url, base64}"| MULTI
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
