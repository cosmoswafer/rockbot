# Image Data Flow

End-to-end summary of how image data moves from RocketChat attachments through
the harness to LLM context, to image generation, and finally back to RocketChat
as a NextCloud share link.

---

## Layer 1: Within Harness — LLM sees images

**DFD**: `_dfds/agent-harness.md` §2e (Auto-Attachment Vision Pipeline)
**Code**: `crate-rockbot/src/harness.rs:161–183`, `:381–457`

```
RocketChat attachment → download → base64 encode → embed in ChatMessage
```

1. User sends image → harness downloads it via RocketChat REST API
   (`download_attachment_refs`)
2. Encoded as `data:image/png;base64,...` data URIs
3. **Two representations injected into LLM context:**
   - **Markdown tags** in message text: `![photo.png](photo.png)` — the LLM
     reads these as human-readable image references
   - **`ContentPart::ImageUrl`** data URIs — the AI provider "sees" the actual
     image pixels for multimodal vision (`ChatMessage::user_with_images`)
4. Message text format: `SenderName: text\nAttached: ![file1.png](file1.png)`
5. Only the **latest** user message keeps full `ImageUrl` parts; older messages
   are collapsed to `[image]` text placeholders (per `_dfds/tools/vision.md`
   §2e, `memory.rs:275–326`)

---

## Layer 2: Harness → Image Gen Tool injection

**DFD**: `_dfds/tools/image-gen.md` §2d (Harness Attachment Injection)
**Code**: `crate-rockbot/src/harness.rs:272–279`, `:994–1037`

```
LLM prompt mentions title? → inject matching data URIs into image_urls
```

1. System prompt tells LLM: *"user-attached images appear as markdown
   `![image_name](image_name)`. Reference the image by image_name in your
   prompt. The harness will automatically resolve image_name references."*
   (`harness.rs:27–31`)
2. When LLM calls `image_gen`, harness intercepts at `harness.rs:272`
   (`inject_image_urls_from_refs`)
3. Scans prompt text for attachment titles (e.g. `photo.png`)
4. Injects matching data URIs into the `image_urls` array of the tool arguments
5. **Merge rule**: if LLM also provides `image_urls` (e.g. fal CDN URLs from
   prior generations), those are merged with harness-injected URIs

---

## Layer 3: Image Gen Tool → Provider → ImageCache + NextCloud Share

**DFD**: `_dfds/tools/image-gen.md` §2a (Happy Flow), §2c (Provider Selection)
**Code**: `crate-rockbot/src/tools/image_gen.rs:219–280`,
`provider/fal.rs:326–386`, `provider/openrouter.rs:779–910`,
`crate-webdav/src/client.rs:76–140`

```
data URI? → upload to provider storage → provider.generate_image() → Vec<u8>
  → WebDAV PUT → create NextCloud share link (7-day expiry) → store in ImageCache
```

1. Image gen tool receives `image_urls` (mix of data URIs and regular URLs)
2. **Data URI handling** differs per provider:
   - **fal.ai**: all data URIs are uploaded to fal storage via two-step
     initiate+PUT protocol (`fal.rs:326–386`). Queue API receives only hosted URLs.
   - **OpenRouter**: data URIs pass inline as `ContentPart::ImageUrl` parts
     in the request messages. No pre-upload needed (`openrouter.rs:912–915`).
3. Regular HTTP(S) URLs pass through directly on both providers
4. `provider.generate_image()` returns raw `Vec<u8>` bytes
5. Image bytes are uploaded to NextCloud WebDAV via `write_file_with_fallback()`
6. **A NextCloud public share link is created** via OCS API
   (`POST /ocs/v2.php/apps/files_sharing/api/v1/shares`) with `shareType=3`
   (public link), `permissions=1` (read-only), and `expireDate={today+7d}`.
   This generates a short URL like `https://nc.tokyofy.top/s/abc123`.
7. Image bytes + share URL are stored in `ImageCache` (keyed by `call_id`) via
   `ImageCache::store()`. The `GeneratedImage` struct holds both `image_bytes`
   (for future direct access) and `share_url: Option<String>`.
8. **Tool returns minimal JSON**: `{"ok": true, "webdav_path": "...", "image_key": "call_...}"`
   — NO base64, NO data URI. The LLM context stays small.

---

## Layer 4: Agent Loop → Reply Assembly (main.rs)

**DFD**: `_dfds/agent-loop.md`
**Code**: `crate-rockbot/src/main.rs:437–470`

```
take_last_image_ids() → ImageCache.take(call_id) → share_url or data_uri → build reply text
```

1. After `process_message()` returns, the agent loop retrieves image call IDs
   from the harness via `take_last_image_ids()`.
2. For each call ID, takes the `GeneratedImage` from `ImageCache`.
3. **Primary path**: if `share_url` is available (NextCloud share link),
   appends `![Generated image](share_url)` to the reply text. The message
   is small and works with both REST and DDP.
4. **Fallback path**: if `share_url` is `None` (share generation failed),
   builds a DDP attachment with the base64 `data_uri()` and sends via
   `reply_with_attachments()`.
5. The `image_key` placeholder is removed from the reply text via
   `strip_markdown_image_id()`.

**Design rationale**: NextCloud share links are short URLs (~40 chars) that
RocketChat renders as inline image previews. This eliminates both the
`Message_MaxAllowedSize` REST limit and the DDP attachments schema issue.
Share links expire after 7 days — longer than typical chat message lifetimes.

---

## Layer 5: Vision Tool Image Injection

**DFD**: `_dfds/tools/vision.md` §2d (Harness Vision Injection), `_dfds/agent-harness.md` §2g
**Code**: `crate-rockbot/src/harness.rs:468–522`

```
vision tool result → cache_vision_images → image pool → inject_vision_images → LLM
```

1. Vision tool returns plain markdown `![photo.png](data:image/png;base64,...)`
2. `cache_vision_images()` parses the markdown, extracts data URIs into a
   per-room `HashMap<String, Vec<CachedImage>>`
3. Before the next LLM call, `inject_vision_images()` drains the pool and
   appends a synthetic user message with `ContentPart::ImageUrl` parts (labelled
   `photo1.png`, `photo2.png`, etc.)
4. The pool is consumed on each injection — images are ephemeral, used for a
   single LLM cycle
5. Size limit uses `max_attachment_bytes` from config (default 25 MB)
