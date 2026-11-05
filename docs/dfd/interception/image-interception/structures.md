# Image Interception — Shared Structures

## 1. Overview

The harness transparently intercepts image data at multiple points in the agent
loop, bridging the gap between text-only tool results and multimodal AI
providers. Four interception points enable the LLM to see, generate, edit, and
share images without handling raw bytes directly.

- Upstream: [Configuration Management](../../infra/config/main-path.md) provides RocketChat
  server URL for attachment downloads
- Upstream: [Agent Harness](../../agent/agent-harness/image-interception.md) runs the interception logic
  inside `process_message()` — image injection happens once, before the first
  LLM call, when the user sends an attachment
- Downstream: [AI Provider](../../ai/ai-provider/main-path.md) receives `ChatMessage` with
  `ContentPart::ImageUrl` parts containing `data:` URIs
- Downstream: [Vision Tool](../../tools/vision/main-path.md) returns markdown
  `![name](data:...)` directly in the tool result — the LLM consumes it in the
  tool-role message; no harness re-injection
- Downstream: [Image Gen Tool](../../tools/image-gen/main-path.md) receives `image_urls` in
  its parameters, injected by the harness from attachments + image_pool + agent
  URLs
- Downstream: [WebDAV Tool](../../tools/webdav/main-path.md) — reading image files returns
  markdown `![name](data:...)` that the LLM sees directly
- Downstream: [WebDAV Directory](../../tools/webdav/main-path.md#1a-transparent-path-isolation)
  stores generated images and provides share URLs

## 3. Data Structures

### `AttachmentRef`
| Field     | Type   | Notes                                          |
| --------- | ------ | ---------------------------------------------- |
| `title`   | String | Original filename (e.g. `"apple.png"`)          |
| `data_uri`| String | `"data:image/png;base64,..."`                   |

### `CachedImage` (image_pool entry)
| Field     | Type   | Notes                                          |
| --------- | ------ | ---------------------------------------------- |
| `name`    | String | Prompt-derived name (char-safe: first 77 chars + `"..."` when >80 chars; never byte-sliced — CJK/emoji prompts) |
| `data_uri`| String | `"data:image/png;base64,..."`                   |

### `ImagePool`
`HashMap<String, Vec<CachedImage>>` keyed by `room_id`. Populated by the
harness from three sources: `image_gen` success (prompt-derived name via
`truncate_pool_name`, `harness.rs`), `vision` tool results (filename from
markdown tag), and `webdav` tool results (filename from markdown tag). Enables
name-based matching in subsequent edit calls. Never drained as a whole —
entries persist for the lifetime of the room.

### `ImageCache`
`Arc<Mutex<HashMap<String, GeneratedImage>>>` keyed by tool `call_id`. Stores
generated images for the reply pipeline. Entries are accessed by `get_image()`
(returns a clone) — they persist beyond the reply so that subsequent
`reference_image_key` lookups succeed. Explicit removal via `take_image()`
is used only when the room's context is evicted (memory compaction).

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
| `truncate_pool_name` | `harness.rs` | Char-safe image_pool name truncation (77 chars + `"..."`, never byte-sliced) |
| `current_image_urls injection` | `harness.rs` (inline in `process_message`) | Auto-injects message image URLs into image_gen args (no prompt matching) |
| `create_nextcloud_share_link` | `crate-webdav/src/client.rs` | Creates 7-day public share for generated images |
| `upload_data_uri` | `tools/image_gen.rs` | Uploads `data:` URI to Fal CDN → returns `https://` URL |
| `strip_markdown_image_id` | `utils.rs` | Removes `![desc](image_key)` from reply text |
| `take_last_image_ids` | `harness.rs` | Returns and drains `last_image_ids` |
| `get_image` | `harness.rs` | Returns `GeneratedImage` clone from `ImageCache` by call_id (non-destructive, so `reference_image_key` lookups work) |
| `take_image` | `harness.rs` | Removes `GeneratedImage` from `ImageCache` by call_id (used only during context eviction) |
