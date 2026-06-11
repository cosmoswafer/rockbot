# Image Data Flow

End-to-end summary of how image data moves from RocketChat attachments through
the harness to LLM context and finally to fal.ai generation.

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

## Layer 3: Image Gen Tool → fal.ai

**DFD**: `_dfds/tools/image-gen.md` §2c (Model Selection), §2b (Error Handling)
**Code**: `crate-rockbot/src/tools/image_gen.rs:219–240`, `provider/fal.rs:326–386`

```
data URI? → upload to fal storage → hosted URL → fal queue API
```

1. Image gen tool receives `image_urls` (mix of data URIs and regular URLs)
2. **All data URIs** (`starts_with("data:")`) are uploaded to fal.ai storage
   via two-step initiate+PUT protocol (`fal.rs:326–386`)
3. Regular HTTP(S) URLs pass through directly
4. fal queue API receives only hosted URLs — **no inline base64** in the
   request body
5. Returns: `{"ok": true, "fal_url": "...", "webdav_path": "..."}` — LLM
   shares `fal_url` as `![desc](fal_url)` markdown

---

## Remaining gap

`inject_image_urls_from_refs` at `harness.rs:1008` has a **25 MB guard** that
silently skips data URIs larger than 25 MB — they never reach image_gen at
all. Since image_gen now uploads all data URIs to fal storage, this guard
should be removed so large images are not silently lost.

```rust
// harness.rs:1008
const MAX_DATA_URI_BYTES: usize = 25_000_000;
// ...
if r.data_uri.len() <= MAX_DATA_URI_BYTES {
    injected.push(...);
} else {
    warn!("Skipping oversized data URI");
}
```
