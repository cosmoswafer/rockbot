# Image Interception

## 1. Purpose

The harness transparently intercepts image data at multiple points in the agent
loop, bridging the gap between text-only tool results and multimodal AI
providers. Five interception points enable the LLM to see, generate, edit, and
share images without handling raw bytes directly.

- Upstream: [Configuration Management](base/config.md) provides RocketChat
  server URL for attachment downloads
- Upstream: [Agent Harness](agent-harness.md) runs the interception logic
  inside `process_message()` — image injection happens before the first LLM
  call and after each tool-execution iteration
- Downstream: [AI Provider](base/ai-provider.md) receives `ChatMessage` with
  `ContentPart::ImageUrl` parts containing `data:` URIs
- Downstream: [Vision Tool](tools/vision.md) produces markdown `![name](data:...)`
  tags that the harness parses into the `image_pool`
- Downstream: [Image Gen Tool](tools/image-gen.md) receives `image_urls` in
  its parameters, injected by the harness from attachments + image_pool + agent
  URLs
- Downstream: [WebDAV Tool](tools/webdav.md) — reading image files triggers
  the same `cache_vision_images` pipeline as vision results
- Downstream: [WebDAV Directory](tools/webdav.md#1a-transparent-path-isolation)
  stores generated images and provides share URLs

## 2. Diagram

### 2a. Complete Interception Pipeline

```mermaid
flowchart TD
    subgraph "Input Sources"
        ATTACH["User Attachments<br/>download_attachment_refs"]
        VISION["Vision Tool Result<br/>![name](data:...)"]
        WEBDAV_READ["WebDAV Read<br/>(image files)"]
        PREV_GEN["Previous image_gen Result<br/>(share_url in ImageCache)"]
        MSG_URLS["Message Image URLs<br/>IncomingMessage.urls<br/>filtered: content_type image/*"]
    end

    subgraph "Interception Layer"
        REFS["AttachmentRef list<br/>{title, data_uri}"]
        POOL[(ImagePool<br/>room_id → Vec<CachedImage>)]
        CACHE_VISION[cache_vision_images<br/>parse markdown data URIs]
        INJECT_VISION[inject_vision_images<br/>drain pool → user msg]
        INJECT_URLS[inject_image_urls_from_refs<br/>match by name → image_urls]
    end

    subgraph "Consumers"
        CTX[LLM Context<br/>ChatMessage::user_with_images]
        IMG_GEN[image_gen Tool<br/>params.image_urls]
        REPLY[Bot Reply<br/>main.rs → share_url or data_uri]
    end

    ATTACH -->|"download → data: URIs"| REFS
    VISION -->|"markdown tags"| CACHE_VISION
    WEBDAV_READ -->|"markdown tags"| CACHE_VISION
    CACHE_VISION -->|"CachedImage {data_uri, name}"| POOL
    POOL -->|"drain"| INJECT_VISION
    INJECT_VISION -->|"user msg + ImageUrl parts"| CTX
    REFS -->|"match title in prompt"| INJECT_URLS
    POOL -->|"match name in prompt"| INJECT_URLS
    PREV_GEN -->|"LLM passes share_url"| INJECT_URLS
    MSG_URLS -->|"auto-inject (no matching)"| INJECT_URLS
    INJECT_URLS -->|"args['image_urls']"| IMG_GEN
    IMG_GEN -->|"GeneratedImage"| REPLY
```

### 2b. Attachment → Context Flow

When a user sends an image in RocketChat, the harness downloads it, encodes it
as a `data:` URI, and embeds it directly in the user's `ChatMessage`:

```mermaid
flowchart LR
    RC[IncomingMessage.attachments]
    DOWNLOAD[download_attachment_refs]
    ENCODE["Base64 encode<br/>→ data:image/png;base64,..."]
    BUILD["ChatMessage::user_with_images<br/>text + ImageUrl parts"]
    HIST[(ConversationHistory)]
    CTX[LLM Context]

    RC -->|"title_link"| DOWNLOAD
    DOWNLOAD -->|"image bytes"| ENCODE
    ENCODE -->|"AttachmentRef {title, data_uri}"| BUILD
    BUILD -->|"user message"| HIST
    HIST -->|"preserved on last user msg"| CTX
```

The message text contains a reference label like `Attached: ![apple.png](apple.png)`.
The actual pixels are embedded as `ContentPart::ImageUrl { url: "data:..." }` in
the same message.

**Provider-level handling** (see [ai-provider.md §2c](../base/ai-provider.md#2c-vision-payload-deep-dive)):
- **Vision-capable providers** (OpenRouter): multipart messages with `ImageUrl`
  parts pass through unchanged — the LLM sees the actual image pixels.
- **Text-only providers** (DeepSeek): `ImageUrl` parts are stripped from every
  message and replaced with `[image]` text placeholders via
  `strip_message_images()`. The LLM cannot see image content but can still call
  `image_gen` to edit images via `current_image_urls` auto-injection.

### 2c. Vision/WebDAV → ImagePool → Context Flow

When the LLM fetches an image from a public URL or WebDAV, the harness parses
the markdown result and caches the data URI for injection into the next LLM call:

```mermaid
flowchart LR
    TOOL[Vision / WebDAV Read]
    RESULT["![name](data:image/png;base64,...)"]
    PARSE[cache_vision_images<br/>extract name + data URI]
    POOL["(ImagePool<br/>CachedImage {data_uri, name})"]
    INJECT[inject_vision_images<br/>rename to photoN.ext]
    MSG["user msg:<br/>'The requested image is visible below:<br/>Attached: ![photo1.png](photo1.png)'"]

    TOOL --> RESULT
    RESULT --> PARSE
    PARSE --> POOL
    POOL --> INJECT
    INJECT --> MSG
```

The pool is drained on each injection — images are ephemeral, used for a single
LLM cycle. Injection happens before the first LLM call and after each
tool-execution iteration.

### 2d. Image Editing — inject_image_urls_from_refs

When the LLM calls `image_gen` with an edit prompt, the harness intercepts the
arguments and injects real image data from four converging sources:

```mermaid
flowchart TD
    PROMPT[LLM Prompt<br/>e.g. 'edit apple.png to add a hat']
    ATTACH_REF["AttachmentRef<br/>{title: 'apple.png', data_uri: 'data:...'}"]
    IMG_POOL["ImagePool<br/>CachedImage name: photo1.png, data_uri: ..."]
    AGENT_URL["LLM-provided URLs<br/>(share_url, https://...)"]
    MSG_URLS["Message Image URLs<br/>current_image_urls<br/>(from DDP urls,<br/>content_type image/*)"]

    INJECT[inject_image_urls_from_refs]
    DEDUP[Deduplicate by URL string]
    OUT["args[image_urls]"]

    PROMPT -->|"contains 'apple.png'?"| ATTACH_REF
    PROMPT -->|"contains 'photo1.png'?"| IMG_POOL
    AGENT_URL -->|"explicit image_urls"| INJECT
    ATTACH_REF -->|"match → inject data URI"| INJECT
    IMG_POOL -->|"match → inject data URI"| INJECT
    MSG_URLS -->|"auto-inject (unconditional)"| INJECT
    INJECT --> DEDUP
    DEDUP --> OUT
```

**How matching works**: the prompt is lowercased and checked for substring
matches against:
1. `AttachmentRef.title` — original user attachment filenames
2. `CachedImage.name` and `![name]` label — vision/webdav-fetched images
3. Explicit `image_urls` from the LLM (deduplicated against injected URIs)
4. `current_image_urls` — image URLs from the DDP message `urls` field (filtered
   by `content_type: image/*`). These are **always injected unconditionally**
   — no prompt matching required — because the harness knows the user shared them
   for editing.

**After injection**: `image_gen` receives the `image_urls` array. `data:` URIs
are uploaded to the provider's CDN (Fal) via `upload_data_uri` → returns an
`https://` URL. Existing `https://` URLs (e.g. from a previous `image_gen`
`share_url`) pass through directly.

### 2e. Generated Image Loopback

Generated images can be reused for editing — the `image_gen` tool exposes the
NextCloud `share_url` in its result JSON, which the LLM can pass back in
`image_urls` on a subsequent call:

```mermaid
flowchart LR
    GEN["image_gen Result<br/>{share_url, image_key}"]
    LLM[LLM sees share_url]
    NEXT["Next image_gen Call<br/>image_urls: share_url"]
    PROVIDER[Provider Receives<br/>https:// URL for img2img]

    GEN -->|"share_url in result JSON"| LLM
    LLM -->|"passes in image_urls"| NEXT
    NEXT -->|"inject_image_urls_from_refs<br/>merges with agent URLs"| PROVIDER
```

The loopback path: `image_gen` → `ImageCache` + tool result → LLM includes
`share_url` in next call → `inject_image_urls_from_refs` merges it →
provider receives `https://` URL (no re-upload needed).

## 3. Data Structures

### `AttachmentRef`
| Field     | Type   | Notes                                          |
| --------- | ------ | ---------------------------------------------- |
| `title`   | String | Original filename (e.g. `"apple.png"`)          |
| `data_uri`| String | `"data:image/png;base64,..."`                   |

### `CachedImage` (image_pool entry)
| Field     | Type   | Notes                                          |
| --------- | ------ | ---------------------------------------------- |
| `name`    | String | Filename from markdown alt-text                 |
| `data_uri`| String | `"data:image/png;base64,..."`                   |

### `ImagePool`
`HashMap<String, Vec<CachedImage>>` keyed by `room_id`. Drained on each
`inject_vision_images` call. Populated by `cache_vision_images` from vision
and webdav tool results.

### `ImageCache`
`Arc<Mutex<HashMap<String, GeneratedImage>>>` keyed by tool `call_id`. Stores
generated images for the reply pipeline. Entries are consumed by `take_image()`.

### `GeneratedImage`
| Field         | Type           | Notes                                   |
| ------------- | -------------- | --------------------------------------- |
| `webdav_path` | String         | WebDAV path where image was persisted   |
| `image_bytes` | `Vec<u8>`      | Raw bytes for fallback data URI         |
| `mime_type`   | String         | `"image/png"`, `"image/jpeg"`, etc.     |
| `share_url`   | Option\<String\>| NextCloud public share link (7-day expiry) |

## 4. Key Functions

| Function | Location | Role |
|----------|----------|------|
| `download_attachment_refs` | `harness.rs` | Downloads RocketChat attachments → `AttachmentRef` list |
| `download_and_encode_single` | `harness.rs` | Single attachment → `data:` URI |
| `inject_image_urls_from_refs` | `harness.rs` | Injects image URLs from attachments + image_pool + agent URLs |
| `current_image_urls injection` | `harness.rs` (inline in `process_message`) | Auto-injects message image URLs into image_gen args (no prompt matching) |
| `cache_vision_images` | `harness.rs` | Parses `![name](data:...)` from tool results → `image_pool` |
| `inject_vision_images` | `harness.rs` | Drains `image_pool` → `ChatMessage::user_with_images` |
| `create_nextcloud_share_link` | `crate-webdav/src/client.rs` | Creates 7-day public share for generated images |
| `upload_data_uri` | `tools/image_gen.rs` | Uploads `data:` URI to Fal CDN → returns `https://` URL |
| `strip_markdown_image_id` | `utils.rs` | Removes `![desc](image_key)` from reply text |
| `take_last_image_ids` | `harness.rs` | Returns and drains `last_image_ids` |
| `take_image` | `harness.rs` | Removes `GeneratedImage` from `ImageCache` by call_id |
