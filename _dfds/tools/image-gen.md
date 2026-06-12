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

    AGENT -->|"prompt + image_urls (LLM), room_id + webdav_dir + image_cache_key + image_size (harness injects)"| PARSE
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

### 2d. Image URL Injection for Editing

When the LLM calls `image_gen` for editing (with `image_urls` in the
arguments), the harness intercepts the call at `inject_image_urls_from_refs()`
(`harness.rs:1475`) and enriches the arguments with image URLs from four
converging sources. The full merge logic is in
[Image Interception](../image-interception.md#2d-image-editing--inject_image_urls_from_refs).

```mermaid
flowchart TD
    LLM_CALL["LLM Calls image_gen<br/>prompt + optional image_urls"]
    ATTACH["1. User Attachments<br/>(matched by title in prompt)"]
    POOL["2. Vision/WebDAV Pool<br/>(matched by name in prompt)"]
    AGENT_URL["3. Agent-Provided URLs<br/>(explicit image_urls from LLM)"]
    MSG_URL["4. Message Image URLs<br/>(auto-injected unconditionally)"]
    INJECT["Harness Intercepts<br/>inject_image_urls_from_refs<br/>merge + deduplicate"]
    IMG_GEN["ImageGenTool.execute<br/>prompt + enriched image_urls"]

    LLM_CALL -->|"raw args"| INJECT
    ATTACH -->|"data URIs"| INJECT
    POOL -->|"data URIs"| INJECT
    AGENT_URL -->|"https or data URLs"| INJECT
    MSG_URL -->|"share URLs"| INJECT
    INJECT -->|"enriched args"| IMG_GEN
```

After injection, `data:` URIs are uploaded to the provider's CDN (Fal) via
`upload_data_uri`, which returns an `https://` URL. Existing `https://` URLs
(e.g. NextCloud share links from a previous `image_gen` result) pass through
directly. See
[Provider Selection](#2c-provider-selection--data-uri-handling) for the
subsequent provider dispatch.

## 3. Data Structures

#### `ImageGenParams`

LLM provides `prompt` and optional `image_size`; all other fields come from config.

| Field           | Source            | Type                                           | Description                                      |
| --------------- | ----------------- | ---------------------------------------------- | ------------------------------------------------ |
| `prompt`        | LLM               | `string`                                       | **Required.** Text description of the image      |
| `image_size`    | Config            | preset name                                   | Aspect ratio preset. Set from `[image_model] default_image_size`. Hidden from LLM. |
| `size_tier`     | Config            | `"4K"`, `"2K"`, `"1K"`                        | Resolution tier for OpenRouter. Set from `default_image_size_tier`. Ignored by fal. |
| `room_id`       | Harness           | `string`                                       | Room UUID for image storage (injected if omitted). **Note:** injected at execute time, not stored in the Rust struct. |
| `webdav_dir`    | Harness           | `string`                                       | Type-prefixed room path (injected; falls back to room_id). **Note:** injected at execute time, not stored in the Rust struct. |
| `image_cache_key`| Harness          | `string`                                       | Tool call_id — used as ImageCache lookup key     |
| `image_urls`    | Harness (auto)    | `[]string`                                     | Injected from 4 converging sources (see §2d): user attachments, vision/WebDAV pool, agent-provided URLs, and message image URLs (auto-injected unconditionally) |
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
