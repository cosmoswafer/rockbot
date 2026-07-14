# Image Generation Tool

## 1. Purpose

Generates images via an `ImageProvider` (fal.ai queue API or OpenRouter
synchronous endpoint), stores them on WebDAV for persistence, and caches the
raw image bytes in the shared `ImageCache`. The agent loop calls `image_gen`
with a prompt and optional parameters; the tool delegates to the provider,
writes to WebDAV, stores to the cache, and returns a minimal result
(`{ok, webdav_path, image_key}`) so the LLM context stays lightweight.

- Upstream: [Agent Harness](../agent/agent-harness.md) injects `room_id`, `webdav_dir`,
  and `image_cache_key` (call_id) into tool args before invoking `execute_by_name()`
- Upstream: [Image Injection Pipeline](../agent/agent-harness.md#2i-generated-image-upload--injection-pipeline)
  retrieves the image from ImageCache by key and uploads it as a RocketChat attachment
- Downstream: [Image Provider](../ai/ai-provider.md) — `FalAiProvider` (CDN-hosted URLs)
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

    AGENT -->|"prompt + aspect_ratio + image_urls + reference_image_key (LLM), room_id + webdav_dir + image_cache_key (harness injects)"| PARSE
    PARSE -->|"merged with config defaults (quality, output_format, num_images, size_tier) + uploaded image_urls + resolved image_size"| RESOLVE
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

### 2d. Aspect Ratio Resolution

The LLM is **required** to supply `aspect_ratio` as a `W:H` string (e.g. `"16:9"`, `"2:3"`,
`"1:1"`). The tool stores the value as `ImageSizeValue::Preset(ratio_string)` and
each provider resolves it to its required format:

```mermaid
flowchart TD
    LLM["LLM provides<br/>aspect_ratio: '16:9'<br/>(required)"]
    TOOL["ImageGenTool.execute<br/>stores as Preset('16:9')"]
    FAL["FalAiProvider<br/>resolve_image_size()"]
    OR["OpenRouterImageProvider<br/>preset_to_aspect_ratio()"]
    FAL_OUT["{'width': 3840, 'height': 2160}<br/>pixel dimensions"]
    OR_OUT["'16:9'<br/>ratio string passthrough"]

    LLM -->|"aspect_ratio arg"| TOOL
    TOOL -->|"ImageGenParams.image_size"| FAL
    TOOL -->|"ImageGenParams.image_size"| OR
    FAL --> FAL_OUT
    OR --> OR_OUT
```

| Ratio string | Fal `resolve_image_size()` output | OpenRouter `preset_to_aspect_ratio()` output |
|---|---|---|
| `"16:9"` | `{"width": 3840, "height": 2160}` | `"16:9"` |
| `"9:16"` | `{"width": 2160, "height": 3840}` | `"9:16"` |
| `"2:3"` | `{"width": 2336, "height": 3504}` | `"2:3"` |
| `"1:1"` | `{"width": 2880, "height": 2880}` | `"1:1"` |
| `"4:3"` | `{"width": 3312, "height": 2480}` | `"4:3"` |
| `"3:4"` | `{"width": 2480, "height": 3312}` | `"3:4"` |
| `"3:2"` | `{"width": 3504, "height": 2336}` | `"3:2"` |
| `"auto"` | `"auto"` (passthrough) | `"auto"` (passthrough) |
| `"auto_2K"` | `"auto_2K"` (passthrough) | `"auto_2K"` (passthrough) |
| `"auto_1K"` | `"auto_1K"` (passthrough) | `"auto_1K"` (passthrough) |

Fal requires pixel dimensions in the `image_size` body field; OpenRouter accepts
the ratio string directly in the `image_config.aspect_ratio` field. Unknown
strings (e.g. `"auto"`, `"auto_2K"`, `"auto_1K"`) pass through unchanged to both providers.
Seedream5 (Fal) supports `auto_2K` and `auto_1K` as auto-dimensional mode strings.

### 2e. Image URL Injection for Editing

When the LLM calls `image_gen` for editing (with `image_urls` or
`reference_image_key` in the arguments), the harness intercepts the call at
`inject_image_urls_from_refs()` (`harness.rs:1475`) and enriches the
arguments with image URLs from five converging sources. The full merge logic
is in [Image Interception](../interception/image-interception.md#2d-image-editing--inject_image_urls_from_refs).

`reference_image_key` provides a simpler alternative: the LLM passes the
`image_key` from a prior `image_gen` result, and the tool looks up the
cached image bytes in `ImageCache`, uploads the data URI to the provider's
CDN, and appends the resulting `https://` URL to `image_urls` — no prompt
matching needed.

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

LLM provides `prompt` and `aspect_ratio` (both required); all other fields come from config.

| Field           | Source            | Type                                           | Description                                      |
| --------------- | ----------------- | ---------------------------------------------- | ------------------------------------------------ |
| `prompt`        | LLM               | `NonEmptyString`                               | **Required.** Validated at JSON deserialization — empty prompt fails at parse boundary. |
| `aspect_ratio`  | LLM               | `NonEmptyString`                              | **Required.** Aspect ratio as `W:H` (e.g. `"16:9"`, `"2:3"`, `"1:1"`). Validated non-empty at deserialization. Stored directly as `image_size: Preset(ratio_string)`. |
| `image_size`    | Tool (resolved)  | preset name → pixels                         | Resolved from LLM's `aspect_ratio` per-provider. Hidden from LLM. |
| `size_tier`     | Config            | `"4K"`, `"2K"`, `"1K"`                        | Resolution tier for OpenRouter. Set from `default_image_size_tier`. Ignored by fal. |
| `room_id`       | Harness           | `string`                                       | Room UUID for image storage (injected if omitted). **Note:** injected at execute time, not stored in the Rust struct. |
| `webdav_dir`    | Harness           | `string`                                       | Type-prefixed room path (injected; falls back to room_id). **Note:** injected at execute time, not stored in the Rust struct; also absent from the LLM-facing tool schema. |
| `image_cache_key`| Harness          | `string`                                       | Tool call_id — used as ImageCache lookup key. **Note:** injected at execute time, not in LLM-facing schema. |
| `image_urls`    | Harness (auto)    | `[]string`                                     | Injected from 5 converging sources (see §2e): user attachments, vision/WebDAV pool, agent-provided URLs, message image URLs (auto-injected unconditionally), and `reference_image_key` (ImageCache lookup) |
| `reference_image_key` | LLM | `string` | Alternative to `image_urls` — the `image_key` from a prior `image_gen` result. Looked up in ImageCache; data URI uploaded to provider CDN. Tool-level arg — not a field on the Rust struct; resolved before `ImageGenParams` construction. |
| `model_id`      | Provider (runtime)| `string`                                       | Selected at provider call time via `provider.model_id()`. The struct field exists but is not populated from config by the tool. |
| `quality`       | Config            | `string`                                       | From `default_quality`                           |
| `output_format` | Config            | `string`                                       | From `default_output_format`                     |
| `num_images`    | Config            | `integer`                                      | From `default_num_images`                        |
| `enable_safety_checker` | Config     | `boolean`                                      | From `default_enable_safety_checker` (default `false`). Sent by `FalAiProvider` only when model contains `"seedream/v5"`. |

#### `ImageGenResult`

The tool returns minimal JSON — no base64. The actual image bytes are in `ImageCache` keyed by `image_key`.

```json
{"ok": true, "webdav_path": "...", "image_key": "call_abc123def4567890", "share_url": "https://..."}
```

The `share_url` field is conditionally present — included only when a NextCloud share link was successfully created for the generated image. It is absent when share generation failed (fallback to DDP attachment path).

#### `ImageCache` Entry (GeneratedImage)

Stored in `Arc<Mutex<HashMap<String, GeneratedImage>>>` keyed by call_id.

| Field          | Type           | Description                                   |
| -------------- | -------------- | --------------------------------------------- |
| `webdav_path`  | `NonEmptyString` | WebDAV path where the image was persisted     |
| `image_bytes`  | `Vec<u8>`        | Raw image bytes (fallback for data URI)       |
| `mime_type`    | `NonEmptyString` | MIME type, e.g. `image/png`                  |
| `share_url`    | `Option<string>`| NextCloud public share link (7-day expiry)    |

After WebDAV upload, the tool calls `create_nextcloud_share_link()` on the
`WebDavClient` which POSTs to `/ocs/v2.php/apps/files_sharing/api/v1/shares`
with `shareType=3`, `permissions=1`, and `expireDate={today+7d}`. The resulting
short URL is stored in `share_url`. The agent loop (main.rs) prefers this URL
for the reply text — appending `![Generated image](share_url)` — which
RocketChat renders as an inline image preview. If share generation fails,
the agent falls back to a `data:` URI as a DDP attachment.
