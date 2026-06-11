# Vision

## 1. Purpose

The agent harness natively "sees" images: when a user uploads an attachment to
RocketChat, the harness downloads it, encodes it as a base64 data URI, and embeds
it directly in the user's `ChatMessage` as `ContentPart::ImageUrl` parts. The AI
provider receives the image data inline — no tool call needed.

The **vision tool** exists for the two cases where the image is NOT already
attached to the incoming message:

- **Public URL**: fetch and analyze any image on the public web (HTTP/HTTPS URL)
- **WebDAV file**: fetch an image stored in the room's WebDAV directory by
  constructing the file's full URL

When the vision tool downloads and encodes an image, its JSON result carries a
`data_uri` field. The harness extracts this and injects an image `ChatMessage`
back into chat history so the LLM "sees" the remote image on the next loop
iteration.

- **Chat history preservation**: `build_context()` keeps `ContentPart::ImageUrl`
  parts only on the most recent user message; earlier ones are collapsed to
  `[image]` text placeholders to save tokens.

- Upstream: [Agent Harness](../agent-harness.md) embeds attachment images directly
  and injects vision tool results into chat context
- Downstream: [AI Provider](../base/ai-provider.md) receives `ChatRequest`
  messages with `ContentPart::ImageUrl` parts and returns multimodal completions

## 2. Diagram

### 2a. Happy Flow (Main Success Path)

The agent harness natively sees user attachments (left path). The vision tool is
only invoked by the LLM for remote images — public URLs or WebDAV files (right
path).

```mermaid
flowchart TD
    RC[RocketChat]
    HARNESS(Harness Encode Attachments)
    HIST[(ConversationHistory)]
    BUILD(BuildContext)
    AI[AiProvider]
    VISION(VisionTool)
    HTTP_DL(DownloadImage)
    WEB[(Public / WebDAV Server)]
    MIME(DetectMimeType)
    ENCODE(Base64Encode)

    RC -->|"message + attachments"| HARNESS
    HARNESS -->|"user msg + data uris"| HIST
    HIST -->|"messages with images"| BUILD
    BUILD -->|"chat request with ImageUrl parts"| AI
    AI -->|"multimodal completion"| HARNESS
    VISION -->|"GET image url"| HTTP_DL
    HTTP_DL -->|"http request"| WEB
    WEB -->|"image bytes"| HTTP_DL
    HTTP_DL -->|"image bytes + url"| MIME
    MIME -->|"mime type"| ENCODE
    ENCODE -->|"data uri"| VISION
    VISION -->|"data_uri + prompt"| HARNESS
    HARNESS -->|"inject image message"| HIST
```

### 2b. Error Handling & Fallbacks

```mermaid
flowchart TD
    HTTP_DL(DownloadImage)
    WEB[(Remote Server)]
    ENCODE(Base64Encode)
    ERR_STATUS[Error: HTTP Non-200]
    ERR_TIMEOUT[Error: Request Timeout]
    ERR_NET[Error: Network Unreachable]
    ERR_SIZE[Error: Image Too Large]
    HARNESS[Agent Harness]

    HTTP_DL -.->|"!200 status"| ERR_STATUS
    HTTP_DL -.->|"30s timeout"| ERR_TIMEOUT
    HTTP_DL -.->|"connection refused / dns failure"| ERR_NET
    ENCODE -.->|"image > 20MB"| ERR_SIZE
    ERR_STATUS -->|"error string"| HARNESS
    ERR_TIMEOUT -->|"error string"| HARNESS
    ERR_NET -->|"error string"| HARNESS
    ERR_SIZE -->|"error string"| HARNESS
```

Errors during auto-attachment download/encode are logged and the attachment is
skipped; the message still enters chat history with text-only content. Errors from
the vision tool are appended as tool result errors.

### 2c. Image Encoding Deep Dive

Level 2 decomposition: downloads the image bytes, verifies the MIME type and size
limit (max 20MB), encodes as base64, and constructs a data URI. Identical logic
shared by auto-attachment (in harness) and vision tool (in `vision.rs`).

```mermaid
flowchart TD
    URL[Image URL]
    DOWNLOAD(HTTP GET)
    CHECK_STATUS{Status 200?}
    CHECK_SIZE{Size < 20MB?}
    DETECT_MIME(Detect MIME from URL ext + Content-Type)
    ENCODE(Base64 encode bytes)
    BUILD_URI(Build data: URI)
    AI[AiProvider]

    URL -->|"full URL"| DOWNLOAD
    DOWNLOAD -->|"response"| CHECK_STATUS
    CHECK_STATUS -->|"yes"| CHECK_SIZE
    CHECK_STATUS -->|"no"| ERR_STATUS[Error: HTTP status]
    CHECK_SIZE -->|"yes"| DETECT_MIME
    CHECK_SIZE -->|"no"| ERR_SIZE[Error: image too large]
    DETECT_MIME -->|"mime type"| ENCODE
    ENCODE -->|"base64 string"| BUILD_URI
    BUILD_URI -->|"data:mime/type;base64,..."| AI
```

The data URI format is: `data:{mime_type};base64,{base64_encoded_bytes}`. The AI
provider wraps this in a `ContentPart::ImageUrl` with the data URI as the `url`
field. The provider's chat completion handler converts it to the provider-specific
format (OpenAI-compatible `image_url` type).

### 2d. Vision Tool Result Feedback

When the vision tool completes, its JSON result contains a `data_uri` field. The
harness extracts this and creates a `ChatMessage::user_with_image(prompt, data_uri)`,
appending it to chat history. The agent loop then continues (via `continue`) so the
LLM "sees" the remote image on the next iteration.

```mermaid
flowchart TD
    TOOL(VisionTool)
    PARSE(Parse vision result)
    CHECK_HAS_URI{Has data_uri?}
    INJECT(ChatMessage::user_with_image)
    HIST[(ConversationHistory)]
    LOOP(Continue agent loop)
    AI[AiProvider]

    TOOL -->|"JSON result"| PARSE
    PARSE -->|"data_uri + prompt"| CHECK_HAS_URI
    CHECK_HAS_URI -->|"yes"| INJECT
    CHECK_HAS_URI -->|"no"| LOOP
    INJECT -->|"image message"| HIST
    HIST -->|"updated messages"| AI
    AI -->|"analysis of injected image"| LOOP
```

If the vision result lacks a `data_uri` field, the result is treated as a standard
tool response (no image injection).

### 2e. Chat History Image Preservation

When `build_context()` assembles messages for the AI provider, it preserves
`ContentPart::ImageUrl` data URIs only on the most recent user message. Earlier
user messages with images are rewritten: image parts become `[image]` text
placeholders, reducing token consumption while keeping the LLM aware that images
were present.

```mermaid
flowchart TD
    HIST[(ConversationHistory)]
    ITER(Iterate messages)
    FIND_LAST(Find last user msg index)
    CHECK{Is last user msg?}
    PRESERVE(Preserve Multipart content)
    STRIP(Strip images to [image] text)
    BUILD(Build messages vec)
    AI[AiProvider]

    HIST -->|"room history"| ITER
    ITER -->|"all messages"| FIND_LAST
    FIND_LAST -->|"last_user_idx"| ITER
    ITER -->|"each message"| CHECK
    CHECK -->|"yes"| PRESERVE
    CHECK -->|"no"| STRIP
    PRESERVE -->|"full message with ImageUrl parts"| BUILD
    STRIP -->|"text-only message"| BUILD
    BUILD -->|"ChatRequest.messages"| AI
```

This ensures the LLM can still "see" attached images from the current user turn
while avoiding unbounded data URI accumulation in the context window. The
`strip_images_from_message()` function in `memory.rs` collapses `Multipart`
content with images into a single-text `[image]` placeholder joined with
remaining text parts.

## 3. Data Structures

#### `VisionParams`

| Field    | Type     | Notes                                                  |
| -------- | -------- | ------------------------------------------------------ |
| `url`    | `string` | URL of the image to download (public or WebDAV)        |
| `prompt` | `string` | What to look for, ask, or analyze in the image         |

#### `VisionResult` (internal tool output)

| Field       | Type     | Notes                                       |
| ----------- | -------- | ------------------------------------------- |
| `data_uri`  | `string` | Base64-encoded data URI                     |
| `mime_type` | `string` | Detected MIME type (`image/png`, etc.)      |
| `size_bytes`| `u64`    | Image file size in bytes                    |
| `prompt`    | `string` | The prompt used for this analysis           |

The harness reads `data_uri` and `prompt` from this JSON to build an image
`ChatMessage` for injection into chat history (see section 2d).

#### Image Content Part

The vision tool and auto-attachment both build `ContentPart::ImageUrl` for the
AI provider:

| Field     | Type     | Notes                                            |
| --------- | -------- | ------------------------------------------------ |
| `url`     | `string` | `data:{mime};base64,{encoded}` data URI           |
| `detail`  | `Option<String>` | `"high"` for high-res analysis            |

This is passed to the AI provider as part of the chat request messages. The AI
provider converts it to the API-specific format (e.g. OpenAI-compatible
`{ "type": "image_url", "image_url": { "url": "...", "detail": "..." } }`).

#### MIME Detection

Detection uses the HTTP `Content-Type` header + URL file extension fallback:

| Extension       | MIME Type        |
| --------------- | ---------------- |
| `.png`          | `image/png`      |
| `.jpg` / `.jpeg`| `image/jpeg`     |
| `.gif`          | `image/gif`      |
| `.webp`         | `image/webp`     |
| `.svg`          | `image/svg+xml`  |
| *(other)*       | `image/png`      |

If the HTTP response includes a `Content-Type` header with a recognized image
MIME type, that takes precedence over extension-based detection.

#### `ChatMessage::user_with_images`

Not a Vision-specific type, but relevant: when the harness auto-attaches images
or injects a vision result, it uses `ChatMessage::user_with_images(text, data_uris)`
or `ChatMessage::user_with_image(text, data_uri)`. These produce `MessageContent::Multipart`
with a `ContentPart::Text` followed by one or more `ContentPart::ImageUrl` parts.
See `types.rs` for the full `ChatMessage` definition.
