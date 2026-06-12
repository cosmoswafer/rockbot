# Image Generation — Full Round-Trip

## 1. Purpose

Tracks the complete data flow from an inbound RocketChat message requesting image
generation to the final bot reply, covering the LLM decision loop, provider
generation (fal.ai submit/poll/download or OpenRouter synchronous POST), WebDAV
storage, RocketChat attachment upload, and the reply sent back.

- Upstream: [Agent Loop](../_dfds/agent-loop.md) delivers the `IncomingMessage`
  and sends the `BotReply`
- Downstream: [Agent Harness](../_dfds/agent-harness.md) executes the
  LLM ↔ tools loop
- Downstream: [Image Gen Tool](../_dfds/tools/image-gen.md) handles
  provider generation + WebDAV upload + ImageCache storage
- Downstream: [AI Provider](../_dfds/base/ai-provider.md) —
  `FalAiProvider` / `OpenRouterImageProvider` for generation, chat provider for LLM
- Companion: [_docs/image-data-flow.md](./image-data-flow.md) —
  prose summary of image data movement across layers

## 2. Diagram

### 2a. Happy Flow — Full Round-Trip (Level 1)

```mermaid
flowchart TD
    RC[RocketChat]
    LOOP(Harness Agent Loop)
    AI[Chat Provider]
    PROVIDER["ImageProvider<br/>(fal or openrouter)"]
    CACHE[(ImageCache)]
    DAV[(NextCloud WebDAV)]
    UPLOAD(UploadToRocketChat<br/>POST rooms.upload)

    RC -->|"incoming message"| LOOP
    LOOP -->|"chat request with tool defs"| AI
    AI -->|"tool_call: image_gen"| LOOP
    LOOP -->|"prompt + image_urls + room_id + webdav_dir + image_cache_key"| PROVIDER
    PROVIDER -->|"image bytes"| CACHE
    PROVIDER -->|"image bytes"| DAV
    DAV -->|"webdav_path"| PROVIDER
    CACHE -->|"stored by image_cache_key"| PROVIDER
    PROVIDER -->|"tool_result { ok, webdav_path, image_key }"| LOOP
    LOOP -->|"chat request with tool result"| AI
    AI -->|"final text reply"| LOOP
    LOOP -->|"image_bytes"| UPLOAD
    UPLOAD -->|"attachment URL"| LOOP
    LOOP -->|"bot reply (markdown with attachment URL)"| RC
```

### 2b. Timing Breakdown

Each edge is annotated with its primary bottleneck. Arrows are colour-coded by
latency class (green = sub-second, yellow = seconds, red = 10s–minutes).

```mermaid
flowchart TD
    RC[RocketChat]
    LOOP(Harness Agent Loop)
    AI[Chat Provider]
    PROVIDER["ImageProvider"]
    CACHE[(ImageCache)]
    DAV[(NextCloud WebDAV)]
    UPLOAD(UploadToRocketChat<br/>POST rooms.upload)

    RC -->|"<100ms"| LOOP
    LOOP -->|"chat API call<br/>1–5s typical"| AI
    AI -->|"tool_calls<br/>streaming: ~200ms"| LOOP
    LOOP -->|"harness args injection<br/><1ms"| PROVIDER
    PROVIDER -->|"generate_image()<br/>fal: 10–120s; openrouter: 5–30s"| CACHE
    PROVIDER -->|"WebDAV PUT<br/>~1–5s"| DAV
    DAV -->|"webdav_path"| PROVIDER
    CACHE -->|"store by key (Mutex)<br/><1ms"| PROVIDER
    PROVIDER -->|"tool result JSON<br/><1ms"| LOOP
    LOOP -->|"chat API call with tool result<br/>1–5s typical"| AI
    AI -->|"final text"| LOOP
    LOOP -->|"Cache.take + rooms.upload<br/>~1–5s"| UPLOAD
    UPLOAD -->|"attachment URL"| LOOP
    LOOP -->|"send_message (REST or DDP)<br/>~500ms"| RC
```

### 2c. Error Handling — Tool Returns Error

```mermaid
flowchart TD
    LOOP(Harness Agent Loop)
    AI[Chat Provider]
    PROVIDER["ImageProvider"]
    ERR_PROVIDER[Provider Error<br/>submit/poll/fetch/download]
    ERR_DAV[WebDAV Error]
    ERR_UPLOAD["Attachment Upload Error<br/>(fallback to data URI)"]
    TOOL_ERR["ToolResult { is_error: true }"]

    PROVIDER -.->|"HTTP error / timeout / FAILED"| ERR_PROVIDER
    PROVIDER -.->|"PUT / mkdir failure"| ERR_DAV
    ERR_PROVIDER -->|"error message"| TOOL_ERR
    ERR_DAV -->|"error message"| TOOL_ERR
    TOOL_ERR -->|"tool result with error text"| LOOP
    LOOP -->|"chat request (LLM sees error)"| AI
    LOOP -.->|"upload fails"| ERR_UPLOAD
    AI -->|"text reply about the failure"| LOOP
```

## 3. Key Latency Points

| Phase                   | Source File:Line                         | Typical      | Worst Case   |
| ----------------------- | ---------------------------------------- | ------------ | ------------ |
| Chat API call #1        | `harness.rs:257`                         | 1–5 s        | 30 s         |
| fal.ai generate         | `provider/fal.rs:168` (submit + poll + download) | 10–120 s  | **600 s**    |
| OpenRouter generate     | `provider/openrouter.rs:779`             | 5–30 s       | 60 s         |
| WebDAV upload           | `tools/image_gen.rs:111`                 | 1–5 s        | 15 s         |
| ImageCache store        | `image_cache.rs:20`                      | <1 ms        | <1 ms        |
| Chat API call #2        | `harness.rs:257` (loop iteration)        | 1–5 s        | 30 s         |
| RocketChat file upload  | `rest.rs:293` — POST rooms.upload        | 1–5 s        | 15 s         |
| Send reply              | `main.rs:408` (REST) / `:423` (DDP)      | ~300 ms      | 5 s          |
| **Total**               |                                          | **20–160 s** | **~21 min**  |

The two 600-second timeouts (`fal.rs:217` poll + `image_gen.rs:89` download) are
independent and stack — worst-case is ~21 minutes for a fal.ai generation before
the bot gives up. OpenRouter is faster (single synchronous POST, 5–30s typical).

After the LLM produces the final text, the harness uploads the image bytes to
RocketChat as a file attachment (1–5s typical). This replaces the old approach
of embedding multi-megabyte base64 data URIs in the message text, which exceeded
RocketChat's `Message_MaxAllowedSize` and caused HTTP 400 errors.

To observe real timings, restart with `RUST_LOG=debug` — timing logs include
`elapsed_ms` for provider generation, WebDAV upload, each tool execution,
each LLM call, and the overall `process_message` duration.
