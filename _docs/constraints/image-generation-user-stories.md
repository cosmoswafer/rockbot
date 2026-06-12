# Image Generation & Image Memory User Stories

Five user stories spanning text-only LLMs, vision LLMs, image attachment interception, image memory, and the pipeline that makes them all work together.

---

## Story 1: Text-Only LLM Uses Image Gen Tool

**A user with a text-only LLM provider (e.g. DeepSeek) asks the bot to generate an image. The LLM cannot see images, but it can call the `image_gen` tool to produce one.**

### Flow

1. User sends a message with an image generation prompt (e.g. "generate a cyberpunk city at night").
2. The text-only LLM decides to call the `image_gen` tool with the prompt.
3. The harness intercepts the tool call and injects `room_id`, `webdav_dir`, `image_cache_key`, and any `current_image_urls` (NextCloud share links from attached images) into the arguments.
4. `ImageGenTool` sends the prompt to the configured image provider (fal.ai or OpenRouter), polls for completion, fetches the result.
5. The generated image is uploaded to WebDAV and a NextCloud share link is created. The image bytes are stored in `ImageCache` keyed by `tool_call.id`.
6. The harness records the `call_id` in `last_image_ids`.
7. The main loop retrieves the generated image from `ImageCache`, builds a markdown `![Generated image](share_url)` reply, and sends it to RocketChat.

### Key implementation details

| Step | Location |
|------|----------|
| Tool call interception (inject room_id, webdav_dir, image_urls) | `harness.rs:343-395` |
| Auto-inject `current_image_urls` for text-only model editing | `harness.rs:365-390` |
| Image generation → WebDAV upload → share link | `tools/image_gen.rs` |
| Image cached in `ImageCache` by `call_id` | `image_cache.rs` |
| Generated image retrieved and sent as reply | `main.rs:442` (via `take_last_image_ids`) |

### Edge case: DeepSeek

DeepSeek does not support vision natively. Images are stripped from messages before sending to DeepSeek (`provider/deepseek.rs:strip_message_images`), replacing image parts with `[image]`. The LLM can still generate images via the tool — it just cannot describe or analyze visual content.

---

## Story 2: User Pastes Image as Attachment — Stored Directly in Conversation Memory

**A user pastes or uploads an image directly into the RocketChat chat. The bot intercepts the attachment, downloads it, encodes it as a `data:` URI, and stores it in the per-room conversation history as a multipart message — making it immediately available to vision LLMs and the `image_gen` tool.**

### Flow

1. User pastes/drags/drops an image into the RocketChat chat input and sends the message (with or without accompanying text).
2. RocketChat delivers the DDP `changed` event with an `attachments` array. Each attachment has a `title_link` (relative URL on the RocketChat server), `title` (filename), and optional `image_url` / `image_dimensions` / `image_type`.
3. The `RocketChatClient` parses the event into `IncomingMessage`, extracting `AttachmentInfo` structs with all metadata.
4. The harness calls `download_attachment_refs()`:
   - Builds the full download URL (`https://{server}{title_link}`)
   - Downloads each image over HTTP with auth headers (`X-Auth-Token`, `X-User-Id`)
   - Enforces `max_attachment_bytes` size limit (default 25 MB)
   - Detects MIME type from `content-type` response header, falls back to `image/png`
   - Base64-encodes the bytes into `data:{mime};base64,...` format
   - Returns `Vec<AttachmentRef>` (each with `title` and `data_uri`)
5. Each `AttachmentRef` becomes a markdown label in the user prompt: `![{title}]({title})`.
6. The harness builds a `ChatMessage::user_with_images(prompt, data_uris)` — a multipart message where text is `ContentPart::Text` and each image is `ContentPart::ImageUrl { url: "data:..." }`.
7. This message is appended to the room's `ConversationHistory` and becomes part of the LLM context for all subsequent turns.

### Key implementation details

| Step | Location |
|------|----------|
| RocketChat attachment parsing → `AttachmentInfo` | `crate-rocketchat/src/types.rs:AttachmentInfo` |
| Collecting `current_image_urls` from message URLs | `harness.rs:185-191` |
| Building `attachment_refs` + multipart user message | `harness.rs:209-233` |
| `download_attachment_refs` — iterates attachments, builds download URLs | `harness.rs:622-648` |
| `download_and_encode_single` — HTTP GET + auth + size check + base64 | `harness.rs:651-696` |
| `ChatMessage::user_with_images` constructs multipart message | `types.rs` |
| Size limit: `max_attachment_bytes` (default 25 MB) | `config.rs` |

### What happens next (enables Stories 3-5)

Once stored in conversation history, the attached image is:

- **Visible to vision LLMs** (OpenRouter) — multipart `ContentPart::ImageUrl` with `data:` URI is passed through in the chat request. The LLM can describe, analyze, and answer questions about the image.
- **Stripped to `[image]` for text-only LLMs** (DeepSeek) — `strip_message_images()` replaces image parts with the text placeholder `[image]`. The LLM knows an image was attached but cannot see it.
- **Available to `image_gen` for editing** — `inject_image_urls_from_refs()` matches the attachment's `title` against the LLM's prompt text. If the LLM references the image by filename, the `data:` URI is injected into the tool call's `image_urls`.
- **Auto-injected via `current_image_urls`** — the most recent message's RocketChat image URLs (share links) are unconditionally appended to `image_gen` calls, enabling text-only LLMs to edit images without naming them.

### Data structure: `AttachmentRef`

```rust
struct AttachmentRef {
    pub title: String,     // filename from RocketChat attachment (e.g. "photo.png")
    pub data_uri: String,  // base64-encoded image (e.g. "data:image/png;base64,iVBOR...")
}
```

### Security and limits

| Constraint | Value | Location |
|-----------|-------|----------|
| Download timeout | 30 seconds | `harness.rs:658` |
| Max image size | `max_attachment_bytes` (default 25 MB) | `config.rs` |
| Auth on download | `X-Auth-Token` + `X-User-Id` headers | `harness.rs:654-656` |
| MIME detection | content-type header → extension fallback → `image/png` | `harness.rs:666-674` |

---

## Story 3: Text-Only LLM Edits Images in Conversation Memory

**A user with a text-only LLM provider asks the bot to edit an image that was previously generated or attached in the conversation. The LLM cannot see the image, but it can reference it by name or URL.**

### Flow

1. An image exists in the conversation — either:
   - A user-attached image downloaded from RocketChat (in `conversation_history` as `ChatMessage::user_with_images`)
   - A previously generated image whose share URL was sent as markdown in the conversation
   - A `current_image_urls` entry from the most recent message
2. User sends a message asking to edit the image (e.g. "add neon signs to the cyberpunk city image").
3. The LLM calls `image_gen` with the editing prompt and (optionally) references the image in `image_urls`.
4. The harness intercepts the call and calls `inject_image_urls_from_refs()`, which merges four sources:
   - **User-attached images** matching prompt text by `title`
   - **Vision-fetched images** in `image_pool` matching prompt text by `name` or `![name]`
   - **Agent-provided URLs** the LLM already included in `image_urls`
   - **`current_image_urls`** from the current message (share links auto-injected unconditionally)
5. `ImageGenTool` sends the edit prompt + `image_urls` to the image provider. The provider uses the existing image as input for editing.
6. The result is uploaded to WebDAV, stored in `ImageCache`, and returned as a reply.

### Key implementation details

| Step | Location |
|------|----------|
| `inject_image_urls_from_refs` merging 4 sources | `harness.rs:1475-1519` |
| Attachment title matching in prompt | `harness.rs:1489-1492` |
| Vision pool name matching in prompt | `harness.rs:1495-1503` |
| Agent URL deduplication merge | `harness.rs:1506-1513` |
| Auto-injected `current_image_urls` | `harness.rs:365-390` |
| Image edit via `generate_image_url(image_urls: Some(urls))` | `tools/image_gen.rs` |

### Why this works for text-only LLMs

The `current_image_urls` auto-injection means the most recent attached image is always passed to the edit call — the text-only LLM doesn't need to "know" the URL; the harness provides it automatically. This is the key enabler for Story 2.

---

## Story 4: Vision LLM Uses Image Gen Tool (Sees Images in Memory)

**A user with a vision-capable LLM provider (e.g. OpenRouter) asks the bot to generate or edit an image. The LLM can see images in the conversation and use them as context.**

### Flow

1. Images are present in the conversation context:
   - User-attached images as `ChatMessage::user_with_images` (multipart content with `ContentPart::ImageUrl`)
   - Vision-fetched images injected from `image_pool` as additional `ChatMessage::user_with_images`
2. When building context for the LLM, OpenRouter (unlike DeepSeek) passes multipart messages through unchanged — the images are sent as `data:` URIs in the request.
3. The vision LLM can "see" the images and:
   - Describe their content
   - Answer questions about them
   - Call `image_gen` with a prompt informed by what it sees
   - Reference images by name in `image_urls` for editing
4. The `image_gen` tool call is intercepted and enhanced with `image_urls` from all four sources (same as Story 2), so the LLM can name-drop image references in its prompt and have them resolved.

### Key implementation details

| Step | Location |
|------|----------|
| Multipart messages passed through to OpenRouter | `provider/openrouter.rs` (no image stripping) |
| Vision image injection into context | `harness.rs:730-761` (`inject_vision_images`) |
| Same `inject_image_urls_from_refs` for image_gen | `harness.rs:1475-1519` |

### Differences from text-only mode

| Aspect | Text-Only (DeepSeek) | Vision (OpenRouter) |
|--------|---------------------|---------------------|
| Images in messages | Stripped → `[image]` | Passed as `data:` URIs |
| LLM sees images | No | Yes |
| Can describe images | No | Yes |
| Can reference images for editing | Only via `current_image_urls` auto-injection | By name in prompt + auto-injection |
| Image gen works | Yes (tool call only) | Yes (tool call + visual context) |

---

## Story 5: Vision Tool Fetches Public URL Images into Memory

**A user shares a public image URL or asks the bot to fetch an image from the web. The `vision` tool downloads the image, caches it in `image_pool`, and makes it available to both vision LLMs and the `image_gen` tool.**

### Flow

1. User asks the bot to fetch an image from a public URL (e.g. "get the image at https://example.com/photo.jpg").
2. The LLM calls the `vision` tool with the URL.
3. `VisionTool` downloads the image, detects MIME type (from content-type header or file extension), encodes as base64, and returns `![name](data:mime/type;base64,...)` markdown.
4. The harness detects the successful `vision` tool result and calls `cache_vision_images()`, which parses the markdown and pushes `CachedImage { data_uri, name }` into `image_pool[room_id]`.
5. On the next context build, `inject_vision_images()` drains `image_pool` and injects the images as `ChatMessage::user_with_images` — the vision LLM can now see them.
6. If the user subsequently asks to edit the image, the `image_gen` tool call interception resolves the image name from `image_pool` via `inject_image_urls_from_refs`.

### Same flows for `webdav` tool image reads

The `webdav` tool can also read image files from WebDAV storage and return them as base64 markdown. The harness treats `webdav` tool results identically to `vision` tool results — both trigger `cache_vision_images()`.

### Key implementation details

| Step | Location |
|------|----------|
| `vision` tool downloads and encodes image | `tools/vision.rs` |
| `webdav` tool returns images as base64 markdown | `tools/webdav.rs` |
| Harness caches both tool results in `image_pool` | `harness.rs:429-435` |
| `cache_vision_images` parses `![name](data:...)` markdown | `harness.rs:698-728` |
| `inject_vision_images` drains pool into LLM context | `harness.rs:730-761` |
| `inject_image_urls_from_refs` resolves pool names for image_gen | `harness.rs:1495-1503` |
| Size limit: `max_attachment_bytes` (default 25MB) | `config.rs` |

### Image memory flow diagram

```
User attaches image in RocketChat
        │
        ▼
┌───────────────────────────┐
│ AttachmentRef (data URI)  │────▶ ConversationHistory as user_with_images
└───────────────────────────┘          (always visible to vision LLMs)
        │
        │  (matched by title in prompt)
        ▼
┌───────────────────────────────────────────┐
│ inject_image_urls_from_refs()             │
│ Merges 4 sources → image_gen image_urls   │
└───────────────────────────────────────────┘

Vision tool / webdav tool returns base64 markdown
        │
        ▼
┌───────────────────────┐
│ image_pool            │────▶ inject_vision_images() → user_with_images in context
│ (CachedImage per room)│          (visible to vision LLMs on next turn)
└───────────────────────┘
        │
        │  (matched by name in prompt)
        ▼
┌───────────────────────────────────────────┐
│ inject_image_urls_from_refs()             │
│ Also resolves pool names → image_urls     │
└───────────────────────────────────────────┘

Generated image (image_gen tool)
        │
        ▼
┌───────────────────────┐
│ ImageCache            │────▶ share URL in markdown reply → sent to room
│ (GeneratedImage)      │          (becomes part of conversation history)
└───────────────────────┘
```

---

## Summary: Four Sources Converging on `image_urls`

The `inject_image_urls_from_refs()` function at `harness.rs:1475` is the single convergence point that makes all four stories possible. When the LLM calls `image_gen`, these sources are merged:

| Source | Type | When available |
|--------|------|---------------|
| User-attached images | `AttachmentRef` (data URI) | Current message has RocketChat image attachments |
| Vision/webdav pool | `CachedImage` (data URI) | `vision` or `webdav` tool was called earlier this turn |
| Agent-provided URLs | Share URL or fal CDN URL | LLM explicitly passes `image_urls` in tool call |
| `current_image_urls` | NextCloud share link | Most recent message had RocketChat image URLs (auto-injected) |

The merging is deduplicated — if the same URL appears from multiple sources, it is included once.
