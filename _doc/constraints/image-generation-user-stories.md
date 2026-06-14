# Image Generation & Image Memory User Stories

Three major user stories covering text-only LLM image generation, multi-source image editing, and vision LLM cooperation with all images in context.

---

## 1. Generate Image from Text Prompt

**User sends a text prompt. The LLM (text-only or vision) calls the `image_gen` tool to produce a new image.**

1. User sends a message with an image generation prompt (e.g. "generate a cyberpunk city at night").
2. The LLM calls the `image_gen` tool with the prompt and optional parameters (aspect ratio, style, etc.).
3. The harness intercepts the tool call at `harness.rs:346-400` and:
   - Calls `inject_image_urls_from_refs` (`harness.rs:1468-1517`) to inject `room_id`, `webdav_dir`, and merge available image URL sources (see summary below) — relevant when the LLM provides reference images.
   - Injects `image_cache_key` set to `tool_call.id` (`harness.rs:359-363`).
   - Conditionally auto-injects `current_image_urls` into `image_urls` if non-empty (`harness.rs:365-396`).
4. `ImageGenTool` sends the prompt to the configured image provider (fal.ai or OpenRouter), polls for completion, fetches the result (`tools/image_gen.rs`).
5. The generated image is uploaded to WebDAV and a NextCloud share link is created. The image bytes are stored in `ImageCache` keyed by `tool_call.id` (`image_cache.rs`).
6. The harness collects the `call_id` in `image_ids_this_turn` (`harness.rs:485`), then assigns it to `last_image_ids` (`harness.rs:574`).
7. The generated image is also added to `image_pool` with the prompt text as its name (`harness.rs:503-509`). This enables subsequent tool calls in the same turn to reference it (e.g. "make it darker").
8. The main loop retrieves the generated image from `ImageCache` via `take_last_image_ids` + `take_image`, calls `strip_markdown_image_id` to remove the key from the reply text, and appends `![Generated image](share_url)` markdown (or falls back to a data URI attachment) (`main.rs:448-473`).
9. The share URL reply becomes part of `conversation_history`, visible to vision LLMs in subsequent turns.

**Edge case — text-only LLM (DeepSeek):** DeepSeek does not support vision natively. Images are stripped from messages before sending (`provider/deepseek.rs:60-77`), replacing image parts with `[image]`. The LLM can still generate images via the tool — it just cannot describe or analyze visual content. The `current_image_urls` auto-injection ensures reference images are still passed to the generation provider even though the LLM cannot see them.

---

## 2. Edit Image from User Paste to Chat or WebDAV File or Public URL

**User provides an image via one of three paths (RocketChat attachment, WebDAV file, or public URL) and asks the LLM to edit it. The harness resolves the image to a `data:` URI and passes it to the `image_gen` tool.**

### 2a. User Pastes/Uploads Image as RocketChat Attachment

1. User pastes/drags/drops an image into RocketChat and sends the message (with or without accompanying text).
2. RocketChat delivers a DDP `changed` event with an `attachments` array containing `title_link`, `title`, and optional `image_url` / `image_dimensions` / `image_type`.
3. `RocketChatClient` parses the event into `IncomingMessage`, extracting `AttachmentInfo` structs (`crate-rocketchat/src/types.rs:AttachmentInfo`).
4. The harness calls `download_attachment_refs()` (`harness.rs:687-714`), which delegates to `download_and_encode_single()` (`harness.rs:716-761`):
   - Builds the full download URL (`https://{server}{title_link}`)
   - Downloads over HTTP with auth headers from `rest.headers()` (`X-Auth-Token`, `X-User-Id`) (`harness.rs:718-719`)
   - Enforces `max_attachment_bytes` size limit (default 25 MB) — checked against `content-length` (`harness.rs:743-749`)
   - 30-second download timeout (`harness.rs:722`)
   - Detects MIME type from `content-type`, falls back to `image/png` (`harness.rs:735-741`)
   - Base64-encodes into `data:{mime};base64,...` format (`harness.rs:756-760`)
   - Returns `Vec<AttachmentRef>` with `title` and `data_uri` (`harness.rs:1408-1411`)
5. The harness builds `ChatMessage::user_with_images(prompt, data_uris)` — multipart content with `ContentPart::Text` + `ContentPart::ImageUrl` parts (`harness.rs:197-215`, `types.rs:136-160`). This is stored in `conversation_history` — immediately available to vision LLMs.

### 2b. User Reads Image from WebDAV File

1. User asks the LLM to read an image file from WebDAV storage.
2. The LLM calls the `webdav` tool to read the file (`tools/webdav.rs`).
3. The tool returns the image as `![filename](data:mime/type;base64,...)` markdown.
4. The harness detects the successful `webdav` tool result and calls `cache_vision_images()` (`harness.rs:480`, fn at `763-793`), which parses the markdown and pushes `CachedImage { data_uri, name }` into `image_pool[room_id]`.
5. On the next context build, `inject_vision_images()` (`harness.rs:795-826`) drains `image_pool` and injects images as `ChatMessage::user_with_images` — the vision LLM sees them in context.

### 2c. User Fetches Image from Public URL

1. User asks the LLM to fetch an image from a public URL.
2. The LLM calls the `vision` tool with the URL (`tools/vision.rs`).
3. `VisionTool` downloads the image, detects MIME type, encodes as base64, and returns `![name](data:mime/type;base64,...)` markdown.
4. The harness detects the successful `vision` tool result and calls `cache_vision_images()` (`harness.rs:476`, fn at `763-793`), pushing into `image_pool`.
5. On the next context build, `inject_vision_images()` drains `image_pool` and the vision LLM sees the image.

### 2d. Editing (Common to All Three Input Paths)

1. User sends an editing request referencing an image (e.g. "add neon signs to the cyberpunk city image").
2. The LLM calls `image_gen` with the editing prompt and optionally includes `image_urls`.
3. The harness intercepts and calls `inject_image_urls_from_refs()` (`harness.rs:1468-1517`), merging three sources:
   - **AttachmentRefs** matched by `title` appearing in the prompt text (`harness.rs:1482-1486`)
   - **image_pool** (vision/webdav/generated) matched by `name` or `![name]` in the prompt (`harness.rs:1488-1497`)
   - **Agent-provided URLs** from the LLM's own `image_urls` argument, deduplicated (`harness.rs:1499-1507`)
4. Separately, `current_image_urls` (NextCloud share links from the most recent message) are conditionally injected in `process_message` (`harness.rs:365-396`).
5. `ImageGenTool` sends the edit prompt + merged `image_urls` to the image provider, which uses the existing image as input (`tools/image_gen.rs`).
6. Result is uploaded to WebDAV, stored in `ImageCache`, and returned as a reply — same pipeline as Story 1 steps 6-8.

**Text-only LLMs** rely on `current_image_urls` auto-injection and title/name matching — the harness provides URLs automatically even though the LLM cannot see the images. **Vision LLMs** can also reference images by name in their prompt text for more precise matching.

---

## 3. Vision Model Sees All Input and Output Images and Cooperates During Image Gen

**User with a vision-capable LLM (e.g. OpenRouter) sees all images in the conversation — pasted attachments, fetched images, and previously generated images — and uses that visual context when calling `image_gen`.**

1. Images enter the vision LLM's context through two mechanisms:
   - **Directly in conversation_history:** User-pasted images are stored as `ChatMessage::user_with_images` with multipart content (`data:` URIs). These are present in every context build.
   - **Via image_pool injection:** Vision-fetched images (from `vision` or `webdav` tools) are cached in `image_pool`. On the next context build, `inject_vision_images()` (`harness.rs:795-826`) drains the pool and appends a `ChatMessage::user_with_images` to the context — the vision LLM can see them as if the user had just shared them.
   - **Generated images** from `image_gen` also enter `image_pool` with their prompt text as the name (`harness.rs:503-509`), bridging the generation output back into visual context.
2. When building context, OpenRouter passes multipart messages through unchanged — `data:` URIs are sent directly in the API request (`provider/openrouter.rs`). DeepSeek strips them to `[image]`.
3. With images visible in context, the vision LLM can:
   - Describe image content in detail.
   - Answer questions about what it sees.
   - Call `image_gen` with a prompt informed by visual analysis.
   - Reference images by name in `image_urls` for editing — the harness injects all matched sources at `inject_image_urls_from_refs`.
4. When `image_gen` produces a new image, it enters `image_pool` and becomes part of the context for the next turn. The vision LLM can then:
   - Describe the generated image.
   - Suggest further edits referencing it by prompt text name.
   - Chain multiple generations informed by prior visual output.

**Key difference from text-only:**
| Aspect | Text-Only (DeepSeek) | Vision (OpenRouter) |
|--------|---------------------|---------------------|
| Images in messages | Stripped → `[image]` | Passed as `data:` URIs |
| LLM sees images | No | Yes |
| Can describe images | No | Yes |
| Editing references | Auto-injected (title/name match + `current_image_urls`) | Visual analysis + auto-injection |
| Image gen cooperation | Tool call only, blind | Visual context informs prompt |

---

## Image Memory Flow Diagram

```
INPUT PATHS                           PROCESSING                          OUTPUT
══════════════                        ══════════════                      ══════════

User paste/upload
       │
       ▼
AttachmentRef (data URI) ─────▶ ConversationHistory ─────▶ vision LLM sees
       │                         (user_with_images)          (multipart context)
       │
       │  (matched by title)
       ▼
┌───────────────────────────────────────────┐
│ inject_image_urls_from_refs()             │
│ Merges 3 sources → image_gen image_urls:  │
│   1. AttachmentRefs (by title match)      │
│   2. image_pool (by name match)           │
│   3. Agent-provided URLs (deduped)        │
└───────────────────────────────────────────┘
                           │
WebDAV file / Public URL   │
       │                   │
       ▼                   │
 Vision/webdav tool ─────▶ image_pool ──────┘
 returns base64            │
 markdown          inject_vision_images()
                          │
                          ▼
                 ConversationHistory
                 (user_with_images on next turn)
                          │
                          │  (matched by name)
                          │
                          ▼
            inject_image_urls_from_refs() (source 2)

current_image_urls (NextCloud share links from most recent message)
       │
       ▼  (injected separately in process_message, harness.rs:365-396)
┌───────────────────────────────────────────┐
│ Merged into image_gen image_urls          │
│ (4th source, conditional — non-empty)     │
└───────────────────────────────────────────┘

Generated image (image_gen tool result)
       │
       ├────▶ ImageCache (call_id → GeneratedImage)
       │           │
       │           ▼
       │      main.rs: take_last_image_ids → strip_markdown_image_id → share URL reply
       │
       └────▶ image_pool (added with prompt text as name)
                          │
                          ▼ (next turn)
                  inject_vision_images() → vision LLM sees it
```

---

## Summary: Source Convergence on `image_urls`

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
