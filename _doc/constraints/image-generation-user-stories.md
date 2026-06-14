# Image Generation & Image Memory User Stories

Five user stories spanning text-only LLMs, vision LLMs, image attachment interception, image memory, and the pipeline that makes them all work together.

---

## 1. Text-Only LLM Uses Image Gen Tool

**User sends an image generation prompt. The text-only LLM cannot see images but calls the `image_gen` tool to produce one.**

1. User sends a message with an image generation prompt (e.g. "generate a cyberpunk city at night").
2. The text-only LLM calls the `image_gen` tool.
3. The harness intercepts the tool call at `harness.rs:346-400` and:
   - Calls `inject_image_urls_from_refs` (`harness.rs:1468-1517`) to inject `room_id`, `webdav_dir`, and merge up to 3 image URL sources (see summary below).
   - Injects `image_cache_key` set to `tool_call.id` (`harness.rs:359-363`).
   - Conditionally auto-injects `current_image_urls` into `image_urls` if non-empty (`harness.rs:365-396`).
4. `ImageGenTool` sends the prompt to the configured image provider (fal.ai or OpenRouter), polls for completion, fetches the result (`tools/image_gen.rs`).
5. The generated image is uploaded to WebDAV and a NextCloud share link is created. The image bytes are stored in `ImageCache` keyed by `tool_call.id` (`image_cache.rs`).
6. The harness collects the `call_id` in `image_ids_this_turn` (`harness.rs:485`), then assigns it to `last_image_ids` (`harness.rs:574`).
7. The main loop retrieves the generated image from `ImageCache` via `take_last_image_ids` + `take_image`, calls `strip_markdown_image_id` to remove the key from the reply text, and appends `![Generated image](share_url)` markdown (or falls back to a data URI attachment) (`main.rs:448-473`).

**Edge case — DeepSeek:** DeepSeek does not support vision natively. Images are stripped from messages before sending to DeepSeek (`provider/deepseek.rs:60-77`), replacing image parts with `[image]`. The LLM can still generate images via the tool — it just cannot describe or analyze visual content.

---

## 2. User Pastes Image as Attachment — Stored Directly in Conversation Memory

**User pastes or uploads an image into the chat. The bot downloads it, encodes it as a `data:` URI, and stores it in per-room conversation history — immediately available to vision LLMs and the `image_gen` tool.**

1. User pastes/drags/drops an image into RocketChat and sends the message (with or without accompanying text).
2. RocketChat delivers a DDP `changed` event with an `attachments` array. Each attachment has `title_link` (relative URL), `title` (filename), and optional `image_url` / `image_dimensions` / `image_type`.
3. `RocketChatClient` parses the event into `IncomingMessage`, extracting `AttachmentInfo` structs (`crate-rocketchat/src/types.rs:AttachmentInfo`).
4. The harness calls `download_attachment_refs()` (`harness.rs:687-714`), which for each attachment delegates to `download_and_encode_single()` (`harness.rs:716-761`):
   - Builds the full download URL (`https://{server}{title_link}`)
   - Downloads each image over HTTP with auth headers from `rest.headers()` (`X-Auth-Token`, `X-User-Id`) (`harness.rs:718-719`)
   - Enforces `max_attachment_bytes` size limit (default 25 MB, `config.rs`) — checked against `content-length` response header (`harness.rs:743-749`)
   - 30-second download timeout (`harness.rs:722`)
   - Detects MIME type from `content-type` response header, falls back to `image/png` (`harness.rs:735-741`)
   - Base64-encodes the bytes into `data:{mime};base64,...` format (`harness.rs:756-760`)
   - Returns `Vec<AttachmentRef>` (each with `title` and `data_uri`) (`harness.rs:1408-1411`)
5. The harness builds a `ChatMessage::user_with_images(prompt, data_uris)` — multipart message with `ContentPart::Text` and `ContentPart::ImageUrl` parts (`harness.rs:197-215`, `types.rs:136-160`).
6. This message is appended to the room's `ConversationHistory` and becomes part of the LLM context for all subsequent turns.

**After this step, the attached image is:**
- Visible to vision LLMs (OpenRouter) — `data:` URIs passed through in chat requests.
- Stripped to `[image]` for text-only LLMs (DeepSeek) — via `strip_message_images()`.
- Available to `image_gen` for editing — `inject_image_urls_from_refs()` matches attachment `title` against the LLM's prompt text and injects the `data:` URI.
- Auto-injected via `current_image_urls` — the most recent message's RocketChat image URLs are conditionally appended to `image_gen` calls when the list is non-empty.

---

## 3. Text-Only LLM Edits Images in Conversation Memory

**User asks a text-only LLM to edit an image already in the conversation (previously generated or attached). The LLM cannot see the image but references it by name or URL.**

1. An image exists in the conversation — as a user-attached image in `conversation_history`, a previously generated image with a share URL in the markdown reply, or a `current_image_urls` entry from the most recent message.
2. User sends an editing request (e.g. "add neon signs to the cyberpunk city image").
3. The LLM calls `image_gen` with the editing prompt and (optionally) references the image in `image_urls`.
4. The harness intercepts the call and calls `inject_image_urls_from_refs()` (`harness.rs:1468-1517`), merging three sources:
   - User-attached images matching prompt text by `title` (`harness.rs:1482-1486`)
   - Vision/webdav pool images matching prompt text by `name` (`harness.rs:1488-1497`)
   - Agent-provided URLs the LLM already included in `image_urls` (`harness.rs:1499-1507`)
   
   Separately, `current_image_urls` from the current message is auto-injected in `process_message` (`harness.rs:365-396`) — this is done unconditionally when the list is non-empty.
5. `ImageGenTool` sends the edit prompt + `image_urls` to the image provider, which uses the existing image as input for editing (`tools/image_gen.rs`).
6. Result is uploaded to WebDAV, stored in `ImageCache`, and returned as a reply.

**Why this works for text-only LLMs:** `current_image_urls` auto-injection means the most recent attached image URL is always passed to the edit call — the LLM doesn't need to "know" the URL; the harness provides it automatically.

---

## 4. Vision LLM Uses Image Gen Tool (Sees Images in Memory)

**User with a vision-capable LLM (e.g. OpenRouter) asks the bot to generate or edit an image. The LLM can see images in the conversation and use them as context.**

1. Images are present in context — user-attached images as `ChatMessage::user_with_images` (multipart content) and vision-fetched images from `image_pool`.
2. When building context, OpenRouter passes multipart messages through unchanged — images sent as `data:` URIs in the request (`provider/openrouter.rs`).
3. The vision LLM can see images and: describe their content, answer questions about them, call `image_gen` with a prompt informed by what it sees, and reference images by name in `image_urls` for editing.
4. The `image_gen` tool call is intercepted and enhanced with `image_urls` from all sources (same as Story 3).

**Differences from text-only mode:**

| Aspect | Text-Only (DeepSeek) | Vision (OpenRouter) |
|--------|---------------------|---------------------|
| Images in messages | Stripped → `[image]` | Passed as `data:` URIs |
| LLM sees images | No | Yes |
| Can describe images | No | Yes |
| Can reference images for editing | Only via `current_image_urls` auto-injection + title/name matching | By name in prompt + auto-injection + title/name matching |
| Image gen works | Yes (tool call only) | Yes (tool call + visual context) |

---

## 5. Vision Tool Fetches Public URL Images into Memory

**User shares a public image URL or asks the bot to fetch an image. The `vision` tool downloads it, caches it in `image_pool`, and makes it available to vision LLMs and the `image_gen` tool.**

1. User asks the bot to fetch an image from a public URL (e.g. "get the image at https://example.com/photo.jpg").
2. The LLM calls the `vision` tool with the URL (`tools/vision.rs`).
3. `VisionTool` downloads the image, detects MIME type, encodes as base64, and returns `![name](data:mime/type;base64,...)` markdown.
4. The harness detects the successful `vision` tool result and calls `cache_vision_images()` (`harness.rs:476`, fn at `763-793`), which parses the markdown and pushes `CachedImage { data_uri, name }` into `image_pool[room_id]`.
5. On the next context build, `inject_vision_images()` (`harness.rs:795-826`) drains `image_pool` for the room and injects the images as `ChatMessage::user_with_images` — the vision LLM can now see them.
6. If the user subsequently asks to edit the image, `inject_image_urls_from_refs` resolves the image name from `image_pool` (`harness.rs:1488-1497`).

**Same flows for `webdav` tool image reads:** The `webdav` tool can also read image files from WebDAV storage and return them as base64 markdown (`harness.rs:480`). The harness treats `webdav` tool results identically to `vision` tool results — both trigger `cache_vision_images()`.

**Generated images also enter image_pool:** When the `image_gen` tool produces an image, the harness adds it to `image_pool` with the prompt text as its name (`harness.rs:503-509`). This allows subsequent tool calls (e.g. "make the fluffy cat darker") to reference the just-generated image by matching the previous prompt text.

---

## Image Memory Flow Diagram

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
│ inject_image_urls_from_refs() (harness)   │
│ Merges 3 sources → image_gen image_urls   │
│  1. AttachmentRefs matched by title        │
│  2. image_pool matched by name            │
│  3. Agent-provided URLs (deduped)         │
└───────────────────────────────────────────┘

Vision tool / webdav tool returns base64 markdown
        │
        ▼
┌───────────────────────┐
│ image_pool            │────▶ inject_vision_images() → user_with_images in context
│ (CachedImage per room)│          (visible to vision LLMs on next turn)
└───────────────────────┘
        │                              ▲
        │  (matched by name in prompt) │
        ▼                              │
┌───────────────────────────────────────────┐
│ inject_image_urls_from_refs()             │
│ Also resolves pool names → image_urls     │
└───────────────────────────────────────────┘

Generated image (image_gen tool)
        │
        ├────▶ ImageCache (call_id → GeneratedImage)
        │           │
        │           ▼
        │      main.rs: take_last_image_ids → share URL in reply
        │
        └────▶ image_pool (added with prompt text as name)
                       │
                       ▼
                  Subsequent tool calls can reference by prompt name

current_image_urls (most recent message's RocketChat image URLs)
        │
        ▼  (injected separately in process_message, harness.rs:365-396)
┌───────────────────────────────────────────┐
│ Merged into image_gen image_urls          │
│ (conditional — only when non-empty)       │
└───────────────────────────────────────────┘
```

---

## Summary: Four Sources Converging on `image_urls`

When the LLM calls `image_gen`, image URL sources converge at two points:

**`inject_image_urls_from_refs()`** (`harness.rs:1468-1517`) merges 3 sources:

| Source | Type | When available |
|--------|------|---------------|
| User-attached images | `AttachmentRef` (data URI) | Current message has RocketChat image attachments |
| Vision/webdav pool | `CachedImage` (data URI) | `vision` or `webdav` tool was called this turn (or `image_gen` generated earlier in the turn) |
| Agent-provided URLs | fal CDN URL or share link | LLM explicitly passes `image_urls` in tool call |

**`process_message`** (`harness.rs:365-396`) adds a 4th source:

| Source | Type | When available |
|--------|------|---------------|
| `current_image_urls` | NextCloud share link | Most recent message had RocketChat image URLs (auto-injected when list is non-empty) |

All merging is deduplicated — if the same URL appears from multiple sources, it is included once.
