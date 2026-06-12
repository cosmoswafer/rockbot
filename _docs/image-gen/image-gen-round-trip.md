# Image Generation — Full Round-Trip

## 1. Purpose

Tracks the complete data flow from an inbound RocketChat message requesting image
generation to the final bot reply, covering the LLM decision loop, provider
generation (fal.ai submit/poll/download or OpenRouter synchronous POST), WebDAV
storage, NextCloud share link creation, and the reply sent back to RocketChat.

- Upstream: [Agent Loop](../_dfds/agent-loop.md) delivers the `IncomingMessage`
  and sends the `BotReply`
- Downstream: [Agent Harness](../_dfds/agent-harness.md) executes the
  LLM ↔ tools loop
- Downstream: [Image Gen Tool](../_dfds/tools/image-gen.md) handles
  provider generation + WebDAV upload + NextCloud share link + ImageCache storage
- Downstream: [AI Provider](../_dfds/base/ai-provider.md) —
  `FalAiProvider` / `OpenRouterImageProvider` for generation, chat provider for LLM
- Companion: [_docs/image-data-flow.md](./image-data-flow.md) —
  prose summary of image data movement across layers

## 2. Diagram

### 2a. Happy Flow — Full Round-Trip (Level 1)

```mermaid
flowchart TD
    RC[RocketChat]
    LOOP(Harness Agent Loop + main.rs)
    AI[Chat Provider]
    PROVIDER["ImageProvider<br/>(fal or openrouter)"]
    CACHE[(ImageCache)]
    DAV[(NextCloud WebDAV)]
    NC[NextCloud OCS<br/>Create Share Link<br/>7-day expiry]
    ASSEMBLE["Assemble Reply<br/>share_url → markdown<br/>(data_uri fallback)"]

    RC -->|"incoming message"| LOOP
    LOOP -->|"chat request with tool defs"| AI
    AI -->|"tool_call: image_gen"| LOOP
    LOOP -->|"prompt + image_urls + room_id + webdav_dir + image_cache_key"| PROVIDER
    PROVIDER -->|"image bytes"| DAV
    DAV -->|"webdav_path"| PROVIDER
    PROVIDER -->|"webdav_path"| NC
    NC -->|"share_url"| PROVIDER
    PROVIDER -->|"{ image_bytes, mime_type, share_url }"| CACHE
    CACHE -->|"stored by image_cache_key"| PROVIDER
    PROVIDER -->|"tool_result { ok, webdav_path, image_key }"| LOOP
    LOOP -->|"chat request with tool result"| AI
    AI -->|"final text reply"| LOOP
    LOOP -->|"take_last_image_ids()"| CACHE
    CACHE -->|"GeneratedImage"| ASSEMBLE
    ASSEMBLE -->|"bot reply (markdown with share_url)"| RC
```

**Note**: the NextCloud share link is created during image_gen tool execution
(right after WebDAV upload), not as a separate post-processing step. The agent
loop simply reads `share_url` from `ImageCache` and appends it as markdown.

### 2b. Timing Breakdown

Each edge is annotated with its primary bottleneck. Arrows are colour-coded by
latency class (green = sub-second, yellow = seconds, red = 10s–minutes).

```mermaid
flowchart TD
    RC[RocketChat]
    LOOP(Harness Agent Loop + main.rs)
    AI[Chat Provider]
    PROVIDER["ImageProvider"]
    CACHE[(ImageCache)]
    DAV[(NextCloud WebDAV)]
    NC["NextCloud Share<br/>OCS API"]
    ASSEMBLE["Strip image_key<br/>+ append share_url"]

    RC -->|"<100ms"| LOOP
    LOOP -->|"chat API call<br/>1–5s typical"| AI
    AI -->|"tool_calls<br/>streaming: ~200ms"| LOOP
    LOOP -->|"harness args injection<br/><1ms"| PROVIDER
    PROVIDER -->|"generate_image()<br/>fal: 10–120s; openrouter: 5–30s"| DAV
    PROVIDER -->|"WebDAV PUT<br/>~1–5s"| DAV
    DAV -->|"webdav_path"| PROVIDER
    PROVIDER -->|"POST OCS shares<br/>~500ms"| NC
    NC -->|"share_url"| PROVIDER
    PROVIDER -->|"store by key (Mutex)<br/><1ms"| CACHE
    CACHE -->|"stored"| PROVIDER
    PROVIDER -->|"tool result JSON<br/><1ms"| LOOP
    LOOP -->|"chat API call with tool result<br/>1–5s typical"| AI
    AI -->|"final text"| LOOP
    LOOP -->|"Cache.take + strip + append<br/><1ms"| ASSEMBLE
    ASSEMBLE -->|"send_message (REST with short URL)<br/>~200ms"| RC
```

### 2c. Error Handling

```mermaid
flowchart TD
    LOOP(Harness Agent Loop)
    AI[Chat Provider]
    PROVIDER["ImageProvider"]
    ERR_PROVIDER[Provider Error<br/>submit/poll/fetch/download]
    ERR_DAV[WebDAV Error]
    ERR_SHARE["Share Link Error<br/>(share_url = None, falls<br/>back to DDP data URI)"]
    TOOL_ERR["ToolResult { is_error: true }"]

    PROVIDER -.->|"HTTP error / timeout / FAILED"| ERR_PROVIDER
    PROVIDER -.->|"PUT / mkdir failure"| ERR_DAV
    PROVIDER -.->|"OCS API fails"| ERR_SHARE
    ERR_PROVIDER -->|"error message"| TOOL_ERR
    ERR_DAV -->|"error message"| TOOL_ERR
    TOOL_ERR -->|"tool result with error text"| LOOP
    LOOP -->|"chat request (LLM sees error)"| AI
    AI -->|"text reply about the failure"| LOOP
```

**Share link error**: if NextCloud's OCS API fails, `share_url` is set to
`None`. The agent loop then falls back to building a DDP `sendMessage` with a
`data:` URI in the `attachments` field. This is a worst-case path — the reply
still gets delivered, but with a larger payload via DDP.

## 3. Key Latency Points

| Phase                     | Source File:Line                               | Typical      | Worst Case   |
| ------------------------- | ---------------------------------------------- | ------------ | ------------ |
| Chat API call #1          | `harness.rs:257`                               | 1–5 s        | 30 s         |
| fal.ai generate           | `provider/fal.rs:168` (submit + poll + download)| 10–120 s    | **600 s**    |
| OpenRouter generate       | `provider/openrouter.rs:779`                   | 5–30 s       | 60 s         |
| WebDAV PUT                | `tools/image_gen.rs:248`                       | 1–5 s        | 15 s         |
| NextCloud share link      | `webdav/client.rs:76` — POST OCS shares        | ~500 ms      | 5 s          |
| ImageCache store          | `image_cache.rs:20`                            | <1 ms        | <1 ms        |
| Chat API call #2          | `harness.rs:257` (loop iteration)              | 1–5 s        | 30 s         |
| Reply assembly + send     | `main.rs:437–470`                              | <1 ms + ~200ms| 5 s         |
| **Total**                 |                                                | **20–160 s** | **~21 min**  |

## 4. Why NextCloud Share Links

| Approach | Message Size | Works on RC v8.4 | Fate |
|----------|-------------|-------------------|------|
| Base64 in `msg` text | Multi-MB | REST fails (400), DDP works but huge | ✗ Replaced |
| `rooms.upload` REST | Small msg + file upload | Returns 404 (endpoint removed) | ✗ Replaced |
| DDP `attachments` field | Small msg + attachment data | Match failed [400] | ✗ Replaced |
| **NextCloud share URL** | ~40 chars | REST ✅ DDP ✅ | **Current** |

The NextCloud share link is created during `image_gen` tool execution via
`WebDavClient::create_nextcloud_share_link()` — a POST to the OCS sharing API
(`/ocs/v2.php/apps/files_sharing/api/v1/shares`) with `shareType=3` (public
link), `permissions=1` (read-only), and `expireDate={today+7d}`. The result is a
short URL like `https://nc.tokyofy.top/s/abc123` that RocketChat renders as an
inline image preview from standard markdown `![desc](url)`.

The image already lives on NextCloud (uploaded during tool execution), so no
additional upload step is needed. The share link is generated in the same
async call chain, adding only ~500ms overhead. Share links expire after 7 days —
longer than typical chat message relevance.

To observe real timings, restart with `RUST_LOG=debug` — timing logs include
`elapsed_ms` for provider generation, WebDAV upload, share link creation, each
tool execution, each LLM call, and the overall `process_message` duration.
