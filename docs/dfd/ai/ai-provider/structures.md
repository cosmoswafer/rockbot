# AI Provider — Shared Structures

## 1. Overview

Configurable `AiProvider` trait abstracting over OpenAI-compatible chat
completion APIs and `ImageProvider` trait for image generation. Concrete
implementations include OpenRouter, DeepSeek, llama.cpp, OpenRouterImageProvider,
and FalAiProvider (image). Each handles provider-specific headers, model naming,
and payload formatting. Supports both base64 data URIs and remote URLs via
`ContentPart::ImageUrl`. The `stream` field is sent in request bodies but SSE
response parsing is not implemented — all responses are consumed as full JSON.

`OpenRouterImageProvider` targets two OpenRouter API surfaces (see
[OpenRouter Image API Routing](level-2/openrouter-image-routing.md)): the
dedicated **Image API** (`POST {base}/images`) for pure image models listed in
the image catalog (`GET {base}/images/models`), and the legacy chat
completions endpoint with `modalities: ["image"]` as fallback for models
absent from the catalog (Gitea issue #84).

- Upstream: [Configuration Management](../../infra/config/main-path.md) provides `AiConfig`
- Downstream: [Agent Harness](../../agent/agent-harness/agent-loop.md) calls `complete()` with `ChatRequest`
  (message history + tool definitions) and returns `CompletionResult`
- Downstream: [Image Gen Tool](../../tools/image-gen/main-path.md) calls `generate_image()` via
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

#### `ToolArgsRepair` (shared — `provider/tool_args.rs`)

| Function | Signature | Notes |
| -------- | --------- | ----- |
| `repair_tool_args` | `(name: &str, args: &str) -> String` | String-aware scan: close unterminated strings, escape raw control chars, balance braces/brackets outside strings, validate; fallback `{}` |
| `sanitize_messages_tool_calls` | `(&mut [ChatMessage]) -> usize` | Repair every invalid `function.arguments` in a message list in place; returns repair count |
| `is_tool_call_parse_error` | `(&RockBotError) -> bool` | True when the provider error body indicates a tool-call arguments JSON parse failure (nlohmann `parse_error.10x` / "missing closing quote" / "invalid string" + tool-call keywords) |

#### `ImageCatalogModel` (OpenRouter image catalog entry, parsed at boundary)

Response shape of `GET {base}/images/models` → `data[]` (see
[OpenRouter Image API Routing](level-2/openrouter-image-routing.md)).

| Field                  | Type                                        | Notes                                          |
| ---------------------- | ------------------------------------------- | ---------------------------------------------- |
| `id`                   | `String`                                    | Model slug used in generation requests         |
| `supported_parameters` | `Option<HashMap<String, ParamDescriptor>>`  | Capability descriptors keyed by parameter name |

#### `ParamDescriptor` (capability descriptor)

| Variant   | Fields              | Notes                                   |
| --------- | ------------------- | --------------------------------------- |
| `Enum`    | `values: Vec<String>` | Discrete allowlist (e.g. resolutions) |
| `Range`   | `min: i64, max: i64` | Any integer in range (e.g. `n`)        |
| `Boolean` | —                   | Present = supported                     |

#### `ImageApiCaps` (domain caps derived from `ImageCatalogModel`)

| Field                     | Type                 | Notes                                                        |
| ------------------------- | -------------------- | ------------------------------------------------------------ |
| `resolutions`             | `Option<Vec<String>>`| Allowed tiers sorted ascending (`512` < `1K` < `2K` < `4K`)  |
| `aspect_ratios`           | `Option<Vec<String>>`| Allowed `W:H` strings                                        |
| `n_max`                   | `Option<u32>`        | Max images per call (from `n` range descriptor)              |
| `supports_quality`        | `bool`               | `quality` present in `supported_parameters`                  |
| `supports_output_format`  | `bool`               | `output_format` present in `supported_parameters`            |
