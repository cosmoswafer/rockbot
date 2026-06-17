# Image Generation — Top 3 Use Case Scenarios

Three major user stories covering text-to-image generation, multi-source image editing, and vision LLM cooperation.

---

## 1. Generate Image from Text Prompt

**User sends a text prompt. The LLM calls `image_gen` to produce a new image.**

1. User sends a message with an image generation prompt (e.g. "generate a cyberpunk city at night").
2. The LLM calls the `image_gen` tool with `prompt` and optional `aspect_ratio`.
3. The harness intercepts the tool call (`harness.rs:355-409`) and:
   - Calls `inject_image_urls_from_refs()` (`harness.rs:1401-1450`) to inject `room_id`, `webdav_dir`, and merge available image URL sources — relevant when the LLM provides reference images.
   - Injects `image_cache_key` set to `tool_call.id` (`harness.rs:368-373`).
   - Conditionally auto-injects `current_image_urls` (NextCloud share links from the most recent message) into `image_urls` if non-empty (`harness.rs:377-405`).
4. `ImageGenTool` sends the prompt to the configured image provider (fal.ai or OpenRouter), polls for completion, fetches the result (`tools/image_gen.rs`).
5. The generated image is uploaded to WebDAV and a NextCloud share link is created. The image bytes are stored in `ImageCache` keyed by `tool_call.id` (`image_cache.rs`).
6. The harness collects the `call_id` in `image_ids_this_turn` (`harness.rs:485`), then assigns it to `last_image_ids` (`harness.rs:573`).
7. The generated image is also added to `image_pool` with the prompt text as its name (`harness.rs:503-509`). This enables subsequent tool calls in the same turn to reference it by name (e.g. "make it darker").
8. The main loop retrieves the generated image from `ImageCache` via `take_last_image_ids` + `take_image`, calls `strip_markdown_image_id` to remove the key from the reply text, and appends `![Generated image](share_url)` markdown (or falls back to a data URI attachment) (`main.rs:448-473`).
9. The share URL reply becomes part of `conversation_history`, visible to vision LLMs in subsequent turns.

**Edge case — text-only LLM (DeepSeek):** DeepSeek does not support vision natively. Images are stripped from messages before sending (`provider/deepseek.rs:71`), replacing image parts with `[image]`. The LLM can still generate images via the tool — it just cannot describe or analyze visual content. The `current_image_urls` auto-injection ensures reference images are still passed to the generation provider even though the LLM cannot see them.

---

## 2. Edit Image from User Attachment, WebDAV File, or Public URL

**User provides an image via one of three input paths and asks the LLM to edit it.**

### 2a. User Pastes/Uploads Image as RocketChat Attachment

1. User pastes/drags/drops an image into RocketChat and sends the message.
2. RocketChat delivers a DDP event with an `attachments` array containing `title_link`, `title`, and optional `image_url` / `image_dimensions` / `image_type`.
3. `RocketChatClient` parses the event into `IncomingMessage`, extracting `AttachmentInfo` structs (`crate-rocketchat/src/types.rs`).
4. The harness calls `download_attachment_refs()` (`harness.rs:685-712`), which delegates to `download_and_encode_single()` (`harness.rs:714-759`):
   - Builds the full download URL (`https://{server}{title_link}`)
   - Downloads over HTTP with auth headers (`X-Auth-Token`, `X-User-Id`) (`harness.rs:718-719`)
   - Enforces `max_attachment_bytes` size limit (default 25 MB) — checked against `content-length` (`harness.rs:741-748`)
   - 30-second download timeout (`harness.rs:720`)
   - Detects MIME type from `content-type`, falls back to `image/png` (`harness.rs:739`)
   - Base64-encodes into `data:{mime};base64,...` format (`harness.rs:754-758`)
   - Returns `Vec<AttachmentRef>` with `title` and `data_uri`
5. The harness builds `ChatMessage::user_with_images(prompt, data_uris)` — multipart content with `ContentPart::Text` + `ContentPart::ImageUrl` parts (`harness.rs:207-222`). This is stored in `conversation_history` — immediately available to vision LLMs.

### 2b. User Reads Image from WebDAV or Fetches from Public URL

1. User asks the LLM to read an image file from WebDAV or fetch from a URL.
2. The LLM calls the `webdav` tool (for files) or `vision` tool (for URLs).
3. Both tools return the image as `![filename](data:mime/type;base64,...)` markdown in the tool result.
4. The LLM receives the base64 data directly in its tool-result context. Unlike the previous design, these images are **not** cached into `image_pool` — the LLM consumes the tool result directly.

### 2c. Editing (Common to All Input Paths)

1. User sends an editing request referencing an image (e.g. "add neon signs to the cyberpunk city image").
2. The LLM calls `image_gen` with the editing prompt. It can reference images in three ways:
   - **`image_urls`** — explicit URLs (e.g. `share_url` from a previous generation)
   - **`reference_image_key`** — the `image_key` of a previously generated image (looked up in `ImageCache`, uploaded to provider CDN, then appended to `image_urls`)
   - **By name in prompt** — the harness matches filenames/labels mentioned in the prompt text
3. The harness intercepts and calls `inject_image_urls_from_refs()` (`harness.rs:1401-1450`), merging three sources:
   - **AttachmentRefs** matched by `title` appearing in the prompt text (`harness.rs:1415-1419`)
   - **image_pool** (generated images) matched by `![name]` label in the prompt (`harness.rs:1421-1430`)
   - **Agent-provided URLs** from the LLM's own `image_urls` argument, deduplicated (`harness.rs:1432-1440`)
4. Separately, `current_image_urls` (NextCloud share links from the most recent message) are conditionally injected in `process_message` (`harness.rs:377-405`).
5. `ImageGenTool` sends the edit prompt + merged `image_urls` to the image provider, which uses the existing image as input.
6. Result is uploaded to WebDAV, stored in `ImageCache`, and returned as a reply — same pipeline as Scenario 1 steps 6-8.

**Text-only LLMs** rely on `current_image_urls` auto-injection and title/name matching — the harness provides URLs automatically even though the LLM cannot see the images. **Vision LLMs** can also reference images by name in their prompt text for more precise matching.

---

## 3. Vision LLM Sees All Images and Cooperates During Image Gen

**User with a vision-capable LLM (e.g. OpenRouter) sees all images in the conversation and uses that visual context when calling `image_gen`.**

1. Images enter the vision LLM's context through two mechanisms:
   - **Directly in conversation_history:** User-pasted images are stored as `ChatMessage::user_with_images` with multipart content (`data:` URIs). These are present in every context build.
   - **Tool results:** Images fetched by `vision` or `webdav` tools are returned as base64 markdown in tool-result messages — the LLM sees them inline.
   - **Generated images** from `image_gen` also enter `image_pool` with their prompt text as the name (`harness.rs:503-509`), making them available for `inject_image_urls_from_refs` name-matching on subsequent edits.
2. When building context, OpenRouter passes multipart messages through unchanged — `data:` URIs are sent directly in the API request. DeepSeek strips them to `[image]`.
3. With images visible in context, the vision LLM can:
   - Describe image content in detail.
   - Answer questions about what it sees.
   - Call `image_gen` with a prompt informed by visual analysis.
   - Reference images by name in `image_urls` for editing — the harness injects all matched sources via `inject_image_urls_from_refs`.
4. When `image_gen` produces a new image, it enters `image_pool` and the share URL becomes part of the conversation. The vision LLM can then:
   - Describe the generated image (via the share URL in conversation history).
   - Suggest further edits referencing it by `image_key` or `reference_image_key`.
   - Chain multiple generations informed by prior visual output.

### Text-Only vs Vision LLM Comparison

| Aspect | Text-Only (DeepSeek) | Vision (OpenRouter) |
|--------|---------------------|---------------------|
| Images in messages | Stripped → `[image]` | Passed as `data:` URIs |
| LLM sees images | No | Yes |
| Can describe images | No | Yes |
| Editing references | Auto-injected (title/name match + `current_image_urls`) | Visual analysis + auto-injection |
| Image gen cooperation | Tool call only, blind | Visual context informs prompt |

---

## Image Memory Flow

```
INPUT PATHS                          PROCESSING                         OUTPUT
═════════════                        ════════════                       ══════════

User paste/upload
       │
       ▼
AttachmentRef (data URI) ─────▶ conversation_history ─────▶ vision LLM sees
       │                        (user_with_images)           (multipart context)
       │
       │  (matched by title)
       ▼
┌────────────────────────────────────────────┐
│ inject_image_urls_from_refs()              │
│ Merges 3 sources → image_gen image_urls:   │
│   1. AttachmentRefs (by title match)       │
│   2. image_pool (by name/label match)      │
│   3. Agent-provided URLs (deduped)         │
└────────────────────────────────────────────┘
                           │
WebDAV file / Public URL   │
       │                   │
       ▼                   │
vision/webdav tool ───────▶ LLM sees base64 in tool result
(no image_pool caching)      (consumed directly)

current_image_urls (NextCloud share links from most recent message)
       │
       ▼  (injected separately in process_message)
┌────────────────────────────────────────────┐
│ Merged into image_gen image_urls           │
│ (4th source, conditional — non-empty)      │
└────────────────────────────────────────────┘

reference_image_key (image_key of a previously generated image)
       │
       ▼  (looked up in ImageCache, uploaded to provider CDN)
┌────────────────────────────────────────────┐
│ Appended to image_gen image_urls           │
└────────────────────────────────────────────┘

Generated image (image_gen tool result)
       │
       ├────▶ ImageCache (call_id → GeneratedImage)
       │           │
       │           ▼
       │      main.rs: take_last_image_ids → strip_markdown_image_id → share URL reply
       │
       └────▶ image_pool (added with prompt text as name)
                          │
                          ▼ (available for inject_image_urls_from_refs name matching)
```

---

## Source Convergence on `image_urls`

When the LLM calls `image_gen`, image URL sources converge at two points:

**`inject_image_urls_from_refs()`** (`harness.rs:1401-1450`) merges 3 sources:

| Source | Type | When available |
|--------|------|---------------|
| User-attached images | `AttachmentRef` (data URI) | Current message has RocketChat image attachments |
| Generated image pool | `CachedImage` (data URI) | `image_gen` produced an image earlier in this turn |
| Agent-provided URLs | CDN URL or share link | LLM explicitly passes `image_urls` in tool call |

**`process_message`** (`harness.rs:377-405`) adds a 4th source:

| Source | Type | When available |
|--------|------|---------------|
| `current_image_urls` | NextCloud share link | Most recent message had RocketChat image URLs (auto-injected when list is non-empty) |

**`reference_image_key`** (resolved in `ImageGenTool::execute`, `image_gen.rs:196-211`):

| Source | Type | When available |
|--------|------|---------------|
| `reference_image_key` | Uploaded CDN URL | LLM provides the `image_key` of a previously generated image |

All merging is deduplicated — if the same URL appears from multiple sources, it is included once.
