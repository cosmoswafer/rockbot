# Image Generation Tool

## 1. Purpose

Generates images via an `ImageProvider` (fal.ai queue API or OpenRouter
synchronous endpoint), stores them on WebDAV for persistence, and caches the
raw image bytes in the shared `ImageCache`. The agent loop calls `image_gen`
with a prompt and optional parameters; the tool delegates to the provider,
writes to WebDAV, stores to the cache, and returns a minimal result
(`{ok, webdav_path, image_key}`) so the LLM context stays lightweight.

- Upstream: [Agent Harness](../agent-harness.md) injects `room_id`, `webdav_dir`,
  and `image_cache_key` (call_id) into tool args before invoking `execute_by_name()`
- Upstream: [Image Injection Pipeline](../agent-harness.md#2i-generated-image-upload--injection-pipeline)
  retrieves the image from ImageCache by key and uploads it as a RocketChat attachment
- Downstream: [Image Provider](../base/ai-provider.md) — `FalAiProvider` (CDN-hosted URLs)
  and `OpenRouterImageProvider` (inline base64) implement `generate_image() -> Vec<u8>`
- Downstream: WebDAV crate persists image assets
- Shared: `ImageCache` (`image_cache.rs`) is the central store keyed by call_id

## 2. Diagram

### 2a. Happy Flow (Main Success Path)

```mermaid
flowchart TD
    AGENT[Agent Loop]
    PARSE(ParseArgs)
    RESOLVE(ResolveModelProvider)
    PROVIDER[ImageProvider]
    GEN(GenerateImage)
    DAV_UPLOAD(UploadToWebDAV)
    DAV[(NextCloud WebDAV)]
    CACHE[(ImageCache)]
    FORMAT(FormatResult)

    AGENT -->|"prompt + image_size (LLM), room_id + webdav_dir + image_cache_key (harness injects)"| PARSE
    PARSE -->|"merged with config defaults (quality, output_format, num_images) + uploaded image_urls"| RESOLVE
    RESOLVE -->|"t2i or edit provider + ImageGenParams"| PROVIDER
    PROVIDER --> GEN
    GEN -->|"raw image bytes (Vec<u8>)"| DAV_UPLOAD
    DAV_UPLOAD -->|"PUT {output_format}"| DAV
    DAV -->|"webdav_path"| DAV_UPLOAD
    GEN -->|"raw image bytes"| CACHE
    CACHE -->|"stored by image_cache_key"| CACHE
    DAV_UPLOAD -->|"webdav_path"| FORMAT
    FORMAT -->|"{ok, webdav_path, image_key}"| AGENT
```

### 2b. Error Handling & Fallbacks

```mermaid
flowchart TD
    GEN(GenerateImage)
    DAV_UPLOAD(UploadToWebDAV)
    ERR_GEN[Error: GenerateImage Failed]
    ERR_UPLOAD[Error: WebDAV Upload Failed]
    FALLBACK[Return Error to Agent]

    GEN -.->|"HTTP error / timeout / missing result"| ERR_GEN
    DAV_UPLOAD -.->|"WebDAV PUT error"| ERR_UPLOAD
    ERR_GEN --> FALLBACK
    ERR_UPLOAD --> FALLBACK
    FALLBACK -->|"error message"| AGENT[Agent Loop]
```

### 2c. Provider Selection & Data URI Handling

The tool selects the provider based on `image_urls` presence and configuration.
Fal requires CDN-hosted URLs (data URIs uploaded first), OpenRouter accepts
inline base64. The harness is unaware of this difference — both implement
`ImageProvider::generate_image() -> Vec<u8>`.

```mermaid
flowchart TD
    PARSE(ParseArgs)
    CHECK{Has image_urls?}
    UPLOAD_URI[Upload DataURIs<br/>via provider.upload_file]
    T2I[t2i provider]
    IMG2IMG[img2img/edit provider]
    GEN(GenerateImage)

    PARSE --> CHECK
    CHECK -->|"yes (user attachments or LLM-provided URLs)"| UPLOAD_URI
    CHECK -->|"no"| T2I
    UPLOAD_URI --> IMG2IMG
    T2I --> GEN
    IMG2IMG --> GEN
```

**Provider differences:**

| Aspect | fal.ai | OpenRouter |
|--------|--------|------------|
| `upload_file()` | Initiate + PUT to CDN → file_url | Base64-encode → data URI |
| `generate_image()` | Submit → Poll → Fetch CDN → Download | Single POST → parse base64 response |
| Image delivery | CDN URL → separate HTTP GET | Base64 inline in response JSON |
| Protocol | 3-phase async (submit/poll/fetch) | Single synchronous POST |

The `ImageProvider` trait abstracts both — the tool and harness never branch on provider type.

### 2d. Harness Attachment Injection

User-attached images are downloaded and labeled by the harness before the agent
loop. Images appear in the conversation as markdown tags `![title](title)`.
The LLM references images by their title in image_gen prompts. The harness
scans prompts for title matches and injects the matched data URIs.

```mermaid
flowchart TD
    RC_ATT[User Attachment]
    DLOAD(DownloadAttachment)
    ENCODE(EncodeDataURI)
    LABEL[AssignTitle from filename]
    BUILD_MSG["BuildUserMessage with image tags"]
    LLM_CTX[(LLM Context)]
    GEN_PROMPT{LLM prompt mentions title?}
    INJECT(InjectMatched DataURIs)
    TOOL_ARGS[(image_gen args with image_urls)]

    RC_ATT --> DLOAD
    DLOAD --> ENCODE
    ENCODE --> LABEL
    LABEL --> BUILD_MSG
    BUILD_MSG --> LLM_CTX
    LLM_CTX --> GEN_PROMPT
    GEN_PROMPT -->|"yes"| INJECT
    GEN_PROMPT -->|"no (new generation)"| TOOL_ARGS
    INJECT --> TOOL_ARGS
```

## 3. Data Structures

#### `ImageGenParams`

LLM provides `prompt` and optional `image_size`; all other fields come from config.

| Field           | Source            | Type                                           | Description                                      |
| --------------- | ----------------- | ---------------------------------------------- | ------------------------------------------------ |
| `prompt`        | LLM               | `string`                                       | **Required.** Text description of the image      |
| `image_size`    | LLM               | preset name or `{width: int, height: int}`     | Aspect ratio preset or custom dimensions. Both edges multiples of 16, max edge 3840px, aspect ratio ≤ 3:1. Default: `"landscape_4_3"` |
| `room_id`       | Harness           | `string`                                       | Room UUID for image storage (injected if omitted)|
| `webdav_dir`    | Harness           | `string`                                       | Type-prefixed room path (injected; falls back to room_id) |
| `image_cache_key`| Harness          | `string`                                       | Tool call_id — used as ImageCache lookup key     |
| `image_urls`    | Harness (auto)    | `[]string`                                     | Injected from user attachments or LLM-provided URLs |
| `model_id`      | Config            | `string`                                       | From `default_text_model` / `default_edit_model` |
| `quality`       | Config            | `string`                                       | From `default_quality`                           |
| `output_format` | Config            | `string`                                       | From `default_output_format`                     |
| `num_images`    | Config            | `integer`                                      | From `default_num_images`                        |

#### `ImageGenResult`

The tool returns minimal JSON — no base64. The actual image bytes are in `ImageCache` keyed by `image_key`.

```json
{"ok": true, "webdav_path": "...", "image_key": "call_abc123def4567890"}
```

#### `ImageCache` Entry (GeneratedImage)

Stored in `Arc<Mutex<HashMap<String, GeneratedImage>>>` keyed by call_id.

| Field          | Type           | Description                                   |
| -------------- | -------------- | --------------------------------------------- |
| `webdav_path`  | `string`       | WebDAV path where the image was persisted     |
| `image_bytes`  | `Vec<u8>`      | Raw image bytes (fallback for data URI)       |
| `mime_type`    | `string`       | MIME type, e.g. `image/png`                  |
| `share_url`    | `Option<string>`| NextCloud public share link (7-day expiry)    |

After WebDAV upload, the tool calls `create_nextcloud_share_link()` on the
`WebDavClient` which POSTs to `/ocs/v2.php/apps/files_sharing/api/v1/shares`
with `shareType=3`, `permissions=1`, and `expireDate={today+7d}`. The resulting
short URL is stored in `share_url`. The agent loop (main.rs) prefers this URL
for the reply text — appending `![Generated image](share_url)` — which
RocketChat renders as an inline image preview. If share generation fails,
the agent falls back to a `data:` URI as a DDP attachment.
