# AI Provider

## 1. Purpose

Configurable `AiProvider` trait abstracting over OpenAI-compatible chat
completion APIs and `ImageProvider` trait for image generation. Concrete
implementations include OpenRouter, DeepSeek, llama.cpp, OpenRouterImageProvider,
and FalAiProvider (image). Each handles provider-specific headers, model naming,
and payload formatting. Supports both base64 data URIs and remote URLs via
`ContentPart::ImageUrl`. The `stream` field is sent in request bodies but SSE
response parsing is not implemented — all responses are consumed as full JSON.

- Upstream: [Configuration Management](config.md) provides `AiConfig`
- Downstream: [Agent Harness](../agent/agent-harness.md) calls `complete()` with `ChatRequest`
  (message history + tool definitions) and returns `CompletionResult`
- Downstream: [Image Gen Tool](../tools/image-gen.md) calls `generate_image()` via
  `ImageProvider` trait, implemented by `FalAiProvider` and `OpenRouterImageProvider`

### llama.cpp provider

`LlamaCppProvider` targets a local llama.cpp HTTP server (typically
`http://localhost:8080`). The llama.cpp server exposes an OpenAI-compatible
`/v1/chat/completions` endpoint, so the request/response format is shared
with `OpenRouterProvider`. Key differences:

- **Optional API key**: `api_key` is sent as an `Authorization: Bearer <key>`
  header **only when the key is non-empty**. The header is omitted when the key
  is empty, supporting local llama.cpp servers started without `--api-key`.
  When the server is started with `--api-key`, the configured key must be
  present or the server returns `401 Invalid API Key`.
- **Reasoning content extracted**: the `reasoning_content` field from the
  response message is extracted into `CompletionResult.reasoning_content`.
  Thinking models (e.g. lfm25) may put their entire output in
  `reasoning_content` and leave `content` empty. The harness uses this as
  a fallback when `content` is absent.
- **Native tool calling required**: tools are sent in the standard `tools`
  JSON field (same as OpenRouter and DeepSeek). The llama.cpp server must be
  started with `--jinja` so its Jinja2 chat template renders tool definitions
  in the model's native format. The model itself must support tool calling
  (e.g. Qwen2.5, Qwen3, Llama 3.x with tool-use chat template). The server
  returns tool calls in the standard OpenAI `tool_calls` response field with
  `finish_reason: "tool_calls"`. A text-based fallback parser
  (`✿FUNCTION✿` / `✿ARGS✿` / `✿END✿` delimiter scan) is retained for
  safety but should not trigger with properly configured servers.
- **Vision**: `ContentPart::ImageUrl` parts (data URIs) flow through to the
  server in the standard OpenAI multipart format. Vision works when the
  loaded GGUF is a multimodal model (e.g. llava, llava-llama3). For
  text-only models the server ignores or rejects the image part.
- **Single model**: `models` map typically has one entry. The model alias
  is required by config but the server ignores the `model` field in the
  request body (the loaded GGUF determines the model).
- **No retry on 429**: local servers do not rate-limit. Network errors
  (connection refused, timeout) are returned immediately as `ServerError`.
- **Leading system messages coalesced**: before serializing the request body,
  all leading `Role::System` messages are merged into a single system message
  (joined by `\n\n`). Defense-in-depth for strict Jinja chat templates (e.g.
  Qwen3.5/3.6-derived templates used by Bonsai-27B, run with `--jinja`) that
  hard-fail with HTTP 400 *"System message must be at the beginning"* when any
  system message appears at an index ≥ 1 — see Gitea issue #77. `BuildContext`
  already emits a single merged system message; this coalesce protects every
  current and future code path regardless of how the context was assembled.

## 2. Diagram

### 2a. Happy Flow (Main Success Path)

```mermaid
flowchart TD
    AGENT[Agent]
    BUILD(BuildContext)
    FORMAT(FormatProviderRequest)
    OPENROUTER(OpenRouterProvider)
    DEEPSEEK(DeepSeekProvider)
    LLAMA(LlamaCppProvider)
    HTTP(SendHttpRequest)
    PARSE(ParseResponse)
    PROVIDER_API[Provider HTTP API]

    AGENT -->|"chat request"| BUILD
    BUILD -->|"provider request"| FORMAT
    FORMAT -->|"openrouter request"| OPENROUTER
    FORMAT -->|"deepseek request"| DEEPSEEK
    FORMAT -->|"llama.cpp request<br/>(native tools field)"| LLAMA
    OPENROUTER -->|"http request"| HTTP
    DEEPSEEK -->|"http request"| HTTP
    LLAMA -->|"http request<br/>(Bearer auth header when key set)"| HTTP
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
    CTX_ERR{ContextLength<br/>Exceeded?}
    CTX_RET(ContextLengthExceeded<br/>returned to harness)
    ERR_TIMEOUT[Error: Request Timeout]
    ERR_API[Error: API Unreachable]
    ERR_PARSE[Error: Malformed Response]
    ERR_AUTH[Error: Invalid API Key]
    AGENT[Agent Loop]
    SANITIZE["Sanitize messages<br/>(strip reasoning_content<br/>+ fix malformed tool args)"]
    WARN[Warn: Malformed Tool Args]

    HTTP -.->|"429 rate limited"| RATE
    RATE -.->|"backoff signal"| RETRY
    HTTP -.->|"5xx server error"| RETRY
    HTTP -.->|"connect / read timeout"| ERR_TIMEOUT
    RETRY -.->|"retries exhausted"| ERR_API
    HTTP -->|"400 context length"| CTX_ERR
    CTX_ERR -->|"yes"| CTX_RET
    CTX_ERR -->|"no (other 400)"| AGENT
    HTTP -->|"401 unauthorized"| ERR_AUTH
    PARSE -->|"invalid json error"| ERR_PARSE
    ERR_TIMEOUT -->|"timeout error"| AGENT
    ERR_API -->|"api error"| AGENT
    ERR_AUTH -->|"auth error"| AGENT
    ERR_PARSE -->|"parse error"| AGENT
    CTX_RET -->|"context length exceeded"| AGENT
    SANITIZE -.->|"malformed tool args fixed"| WARN
```

**Context-length detection**: HTTP 400 responses whose error message contains
"context length" or "maximum context" (case-insensitive) are mapped to
`RockBotError::ContextLengthExceeded` instead of `InvalidRequest`. The harness
uses this to trigger a hard memory reset and a one-time retry. This
applies to OpenRouter, DeepSeek, and llama.cpp providers.

**HTTP timeouts**: Every `reqwest::Client` used by AI providers is built with
`connect_timeout` and request-level `timeout` to prevent indefinite hangs from
silent TCP drops or unresponsive provider endpoints.

| Parameter | Default | Description |
|-----------|---------|-------------|
| `connect_timeout` | 10s | TCP/TLS handshake timeout |
| `request_timeout` | 300s | Total request duration from first byte sent to response completion |

These timeouts apply to all providers: `DeepSeekProvider`, `OpenRouterProvider`,
`LlamaCppProvider`, and `FalAiProvider` (both submit/poll and image download).
A timeout produces `RockBotError::HttpTimeout` which the harness treats as a
transient failure — it sends an error reply and moves on, releasing the harness
lock for the next message.

The Fal image generation poll loop additionally uses a separate per-HTTP-request
timeout (30s for status polls, 600s for image downloads) to prevent individual
polling requests from blocking the task.

A `tokio::time::timeout()` wrapper at the harness call site provides an additional
defense-in-depth layer: if the provider's own timeout fails to fire (e.g., due to
a bug in `reqwest`'s timeout implementation), the wrapper cancels the future after
a hard deadline (default 360s, one minute longer than the client request timeout).

**llama.cpp error handling**: The local server does not rate-limit (no 429).
Connection errors (server not running, port unreachable) map to `ServerError`
immediately with no retry. HTTP 400 with context-length keywords still triggers
`ContextLengthExceeded`. Tool call arguments arrive as standard JSON from the
server's native tool calling, same as OpenRouter and DeepSeek.

Before sending each request, messages are sanitized:
- `reasoning_content` is stripped from all messages (response-only field that
  some providers reject in request input)
- All `function.arguments` fields in tool calls are validated as parseable
  JSON; malformed arguments (e.g. truncated from length-limited responses) are
  auto-repaired (balance braces/quotes) or reset to `{}`
- After parsing a response, tool call arguments are also validated at the
  parse stage to prevent malformed data from entering conversation history

### 2c. Vision Payload Deep Dive

```mermaid
flowchart TD
    MSG[ChatMessage]
    CHECK(CheckContentType)
    TEXT_ONLY(FormatTextContent)
    MULTI(FormatMultipartContent)
    IMG_URL(FormatImageUrl)
    IMG_B64(FormatImageBase64)
    STRIP{Provider<br/>supports vision?}
    CONVERT["Convert ImageUrl<br/>to &#91;image&#93; text"]
    REQ[ProviderRequest]

    MSG -->|"chat message"| CHECK
    CHECK -->|"text content"| TEXT_ONLY
    CHECK -->|"multipart content"| MULTI
    CHECK -->|"image url"| IMG_URL
    CHECK -->|"image base64"| IMG_B64
    TEXT_ONLY -->|"content string"| REQ
    IMG_URL -->|"image url part"| MULTI
    IMG_B64 -->|"image base64 part"| MULTI
    MULTI -->|"content array"| STRIP
    STRIP -->|"yes (OpenRouter / llama.cpp)"| REQ
    STRIP -->|"no (DeepSeek)"| CONVERT
    CONVERT -->|"text-only content"| REQ
```

**Provider-specific handling**: DeepSeek models do not support vision
(multimodal) input. `DeepSeekProvider::build_request_body()` strips all
`ContentPart::ImageUrl` parts from every `ChatMessage`, converting multipart
content to plain text with `[image]` placeholders. This keeps the shared
`ChatMessage`/`ContentPart` data structures intact across all providers while
preventing 400 errors from `unknown variant 'image_url', expected 'text'`.
`LlamaCppProvider::build_request_body()` passes multipart content through as-is
— llama.cpp servers with a multimodal GGUF (llava, llava-llama3, etc.) handle
the OpenAI-compatible image format natively. OpenRouter passes vision payloads
through as-is — any model-specific vision support is handled by OpenRouter's API.

**Fal seedream5 safety checker**: When the resolved model ID contains `"seedream/v5"`,
`FalAiProvider::submit_request()` conditionally sends `enable_safety_checker` if
present in `ImageGenParams`. The default value comes from
  `ImageModelConfig::default_enable_safety_checker` (default `false`). This is gated
on the model ID to avoid sending the parameter to non-seedream5 Fal models that
may reject it.

## 3. Data Structures

#### `ChatRequest`

| Field              | Type                    | Notes                              |
| ------------------ | ----------------------- | ---------------------------------- |
| `messages`         | `Vec<ChatMessage>`      | Conversation history               |
| `tools`            | `Option<Vec<ToolDef>>`  | Available tool/function definitions (`None` = none; conditionally omitted from serialization) |
| `stream`           | `bool`                  | Enable streaming response          |
| `model`            | `String`                | Model identifier                   |
| `temperature`      | `Option<f32>`           | Sampling temperature               |
| `max_tokens`       | `Option<u32>`           | Maximum output tokens              |
| `thinking`         | `Option<ThinkingConfig>`| Thinking mode config               |
| `reasoning_effort` | `Option<String>`        | Reasoning effort level             |
| `tool_choice`      | `Option<Value>`         | Tool choice override               |

#### `ThinkingConfig`

| Field            | Type     | Notes                              |
| -----------------| -------- | ---------------------------------- |
| `thinking_type`  | `String` | Always `"enabled"` (serialized as `"type"`) |

#### `ChatMessage`

| Field               | Type                       | Notes                             |
| ------------------- | -------------------------- | --------------------------------- |
| `role`              | `Role`                     | `System`, `User`, `Assistant`, `Tool` |
| `content`           | `MessageContent`           | Text or multipart (text + images) |
| `name`              | `Option<String>`           | Tool result name                  |
| `tool_calls`        | `Option<Vec<ToolCall>>`    | Assistant tool call requests      |
| `tool_call_id`      | `Option<String>`           | Required for tool result messages |
| `reasoning_content` | `Option<String>`           | DeepSeek reasoning/chain-of-thought|

#### `MessageContent`

| Variant     | Fields                        | Notes                          |
| ----------- | ----------------------------- | ------------------------------ |
| `Text`      | `String`                      | Plain text content             |
| `Multipart` | `Vec<ContentPart>`            | Mixed text and images          |

#### `ContentPart`

| Variant    | Fields                          | Notes                         |
| ---------- | ------------------------------- | ----------------------------- |
| `Text`     | `String`                        | Text segment                  |
| `ImageUrl` | `ImageUrlPayload { url: String, detail: Option<String> }` | Remote or `data:` base64 URL. Nested `image_url` wrapper matches OpenAI API format `{"type": "image_url", "image_url": {"url": "...", "detail": "..."}}` |

#### `CompletionResult`

| Field               | Type                  | Notes                                |
| ------------------- | --------------------- | ------------------------------------ |
| `text`              | `Option<String>`      | Assistant text response              |
| `tool_calls`        | `Vec<ToolCall>`       | Tool/function calls requested by LLM |
| `finish`            | `FinishReason`        | `Stop`, `ToolUse`, `Length`, `ContentFilter`, `InsufficientSystemResource`, `Error` |
| `reasoning_content` | `Option<String>`      | DeepSeek-style chain-of-thought text |
| `usage`             | `Option<UsageInfo>`   | Token usage statistics               |

#### `ToolCall`

| Field       | Type           | Notes                             |
| ----------- | -------------- | --------------------------------- |
| `id`        | `String`       | Provider-assigned call ID         |
| `call_type` | `String`       | Always `"function"`               |
| `function`  | `FunctionCall` | Nested function details           |

#### `FunctionCall`

| Field       | Type     | Notes                 |
| ----------- | -------- | --------------------- |
| `name`      | `String` | Tool/function name    |
| `arguments` | `String` | JSON-encoded arguments|

#### `ToolDef`

| Field       | Type         | Notes                             |
| ----------- | ------------ | --------------------------------- |
| `tool_type` | `String`     | Always `"function"`               |
| `function`  | `FunctionDef`| Wrapped function definition       |

#### `FunctionDef`

| Field         | Type              | Notes                           |
| ------------- | ----------------- | ------------------------------- |
| `name`        | `String`          | Function name                   |
| `description` | `Option<String>`  | Human-readable description      |
| `parameters`  | `Option<Value>`   | JSON Schema for arguments       |
| `strict`      | `Option<bool>`    | Strict schema enforcement       |
