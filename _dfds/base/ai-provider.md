# AI Provider

## 1. Purpose

Configurable `AiProvider` trait abstracting over OpenAI-compatible chat
completion APIs. Concrete implementations for OpenRouter and DeepSeek handle
provider-specific headers, model naming, and vision payload formatting. Supports
streaming responses and tool/function calling.

- Upstream: [Configuration Management](config.md) provides `AiConfig`
- Downstream: [Agent Loop](agent-harness.md) calls `complete()` with `ChatRequest`
  (message history + tool definitions) and returns `CompletionResult`

## 2. Diagram

### 2a. Happy Flow (Main Success Path)

```mermaid
flowchart TD
    AGENT[Agent]
    BUILD(BuildContext)
    FORMAT(FormatProviderRequest)
    OPENROUTER(OpenRouterProvider)
    DEEPSEEK(DeepSeekProvider)
    HTTP(SendHttpRequest)
    PARSE(ParseResponse)
    PROVIDER_API[Provider HTTP API]

    AGENT -->|"chat request"| BUILD
    BUILD -->|"provider request"| FORMAT
    FORMAT -->|"openrouter request"| OPENROUTER
    FORMAT -->|"deepseek request"| DEEPSEEK
    OPENROUTER -->|"http request"| HTTP
    DEEPSEEK -->|"http request"| HTTP
    HTTP -->|"http post"| PROVIDER_API
    PROVIDER_API -->|"json response body"| HTTP
    HTTP -->|"raw bytes"| PARSE
    PARSE -->|"completion result"| AGENT
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
    AGENT[Agent Loop]

    HTTP -.->|"429 rate limited"| RATE
    RATE -.->|"backoff signal"| RETRY
    HTTP -.->|"5xx server error"| RETRY
    RETRY -.->|"retries exhausted"| ERR_API
    HTTP -->|"401 unauthorized"| ERR_AUTH
    PARSE -->|"invalid json error"| ERR_PARSE
    ERR_API -->|"api error"| AGENT
    ERR_AUTH -->|"auth error"| AGENT
    ERR_PARSE -->|"parse error"| AGENT
```

### 2c. Vision Payload Deep Dive

```mermaid
flowchart TD
    MSG[ChatMessage]
    CHECK(CheckContentType)
    TEXT_ONLY(FormatTextContent)
    MULTI(FormatMultipartContent)
    IMG_URL(FormatImageUrl)
    IMG_B64(FormatImageBase64)
    REQ[ProviderRequest]

    MSG -->|"chat message"| CHECK
    CHECK -->|"text content"| TEXT_ONLY
    CHECK -->|"multipart content"| MULTI
    CHECK -->|"image url"| IMG_URL
    CHECK -->|"image base64"| IMG_B64
    TEXT_ONLY -->|"content string"| REQ
    MULTI -->|"content array"| REQ
    IMG_URL -->|"image url part"| MULTI
    IMG_B64 -->|"image base64 part"| MULTI
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
