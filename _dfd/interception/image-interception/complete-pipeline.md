# Complete Interception Pipeline

## 1. Purpose

The harness transparently intercepts image data from every input source —
user attachments, vision tool results, WebDAV reads, previous `image_gen`
results, and message image URLs — and delivers it to consumers: the LLM context
(`ChatMessage::user_with_images` for attachments, `ChatMessage::tool` with
inline markdown images for tool results), the `image_gen` tool's `image_urls`
parameter, and the bot reply.

## 2. Diagram

```mermaid
flowchart TD
    subgraph "Input Sources"
        ATTACH["User Attachments<br/>download_attachment_refs"]
        VISION["Vision Tool Result<br/>![name](data:...)<br/>in tool-role message"]
        WEBDAV_READ["WebDAV Read (image files)<br/>![name](data:...)<br/>in tool-role message"]
        PREV_GEN["Previous image_gen Result<br/>(share_url in ImageCache)"]
        GEN_POOL["image_gen Success<br/>(added to ImagePool<br/>for name-based matching)"]
        MSG_URLS["Message Image URLs<br/>IncomingMessage.urls<br/>filtered: content_type image/*"]
    end

    subgraph "Interception Layer"
        REFS["AttachmentRef list<br/>{title, data_uri}"]
        POOL[(ImagePool<br/>room_id → Vec<CachedImage><br/>vision/webdav/image_gen)]
        INJECT_URLS[inject_image_urls_from_refs<br/>match by name → image_urls]
    end

    subgraph "Consumers"
        CTX_ATTACH["LLM Context (attachments)<br/>ChatMessage::user_with_images"]
        CTX_TOOL["LLM Context (tool results)<br/>ChatMessage::tool<br/>markdown data URI inline"]
        IMG_GEN[image_gen Tool<br/>params.image_urls]
        REPLY[Bot Reply<br/>main.rs → share_url or data_uri]
    end

    ATTACH -->|"download → data: URIs"| REFS
    REFS -->|"match title in prompt"| INJECT_URLS
    REFS -->|"user msg + ImageUrl parts"| CTX_ATTACH
    VISION -->|"LLM sees data URI directly"| CTX_TOOL
    WEBDAV_READ -->|"LLM sees data URI directly"| CTX_TOOL
    GEN_POOL -->|"CachedImage {data_uri, prompt}"| POOL
    POOL -->|"match name in prompt"| INJECT_URLS
    PREV_GEN -->|"LLM passes share_url or reference_image_key"| INJECT_URLS
    MSG_URLS -->|"auto-inject (no matching)"| INJECT_URLS
    INJECT_URLS -->|"args['image_urls']"| IMG_GEN
    IMG_GEN -->|"GeneratedImage"| REPLY
```
