# Image Editing — Four Converging Sources

## 1. Purpose

When the LLM calls `image_gen` with an edit prompt, image URLs converge from
four sources at two injection points:

## 2. Diagram

**Injection point A — `inject_image_urls_from_refs()`** (`harness.rs`):
merges 3 sources via name/prompt matching + deduplication:

```mermaid
flowchart TD
    PROMPT[LLM Prompt<br/>e.g. 'edit apple.png to add a hat']
    ATTACH_REF["1. User Attachments<br/>AttachmentRef matched by<br/>title substring in prompt"]
    IMG_POOL["2. WebDAV File / Public URL<br/>CachedImage in image_pool<br/>from vision/webdav tool results<br/>or image_gen results"]
    AGENT_URL["3. Agent-Provided URLs<br/>share_url / https:// from LLM's<br/>explicit image_urls arg"]
    INJECT[inject_image_urls_from_refs]
    DEDUP[Deduplicate by URL string]
    ARGS["args[image_urls]<br/>(merged)"]

    PROMPT -->|"contains 'apple.png'?"| ATTACH_REF
    PROMPT -->|"contains image name?"| IMG_POOL
    AGENT_URL -->|"explicit image_urls"| INJECT
    ATTACH_REF -->|"match → data URI"| INJECT
    IMG_POOL -->|"match → data URI"| INJECT
    INJECT --> DEDUP
    DEDUP --> ARGS
```

**Summary — image_urls at provider dispatch**:

| Source | Injection point | Matching | Data format |
|--------|----------------|----------|-------------|
| User Attachments | `inject_image_urls_from_refs` | Title substring in prompt | `data:` URI |
| WebDAV File / Public URL | `inject_image_urls_from_refs` | Name in prompt | `data:` URI (from `image_pool`) |
| Agent-Provided URLs | `inject_image_urls_from_refs` | Explicit (LLM passes in `image_urls`) | `https://` or `data:` URL |
| Message Image URLs | `process_message` inline | None — unconditional | NextCloud share link |
| reference_image_key | `ImageGenTool::execute` | None — lookup by key | `https://` URL (CDN upload) |

All data URIs are uploaded to the provider's CDN (Fal) via `upload_data_uri`
before the generation request is dispatched. Existing `https://` URLs pass
through directly.
