# Agent Harness

## 1. Purpose

The operational environment that wraps the agent loop — the invariant core
cycle of `LLM → tools → LLM → ...`. The harness layers Tools, Knowledge, and
Context around this loop without modifying it.

### 1a. Micro Harness Scope

rockbot implements a **micro harness**: a minimal harness with only the
mechanisms needed for a single-agent, single-channel chatbot. Three of the six
standard harness mechanisms are present:

| Mechanism   | Coverage | Details |
|-------------|----------|---------|
| **Tools**   | Full     | Abstract tool calling via `ToolRegistry` — individual tools each have their own DFD |
| **Context** | Full     | Per-room conversation history buffer, summarization, archive loading — see [Memory Management](base/memory.md); plus iteration limits, room state routing, system prompt assembly |
| **Knowledge** | Full     | `save_knowledge`, `forget_knowledge`, `recall_knowledge`; retrieval via keyword-matching against `when_useful` + `tags` + filename — see [Knowledge Management](base/knowledge.md) |

Intentionally absent — not needed for rockbot's scope:

| Mechanism       | Reason |
|-----------------|--------|
| **Permissions** | Single-user bot — no sandbox or approval flows |
| **Extensions**  | No plugin/hook system — tools are statically registered |
| **Coordination**| Single agent — no subagents, teams, or worktrees |

- Upstream: [Agent Loop](agent-loop.md) feeds `IncomingMessage`
  into the loop and consumes `BotReply`
- Downstream: [AI Provider](base/ai-provider.md) receives `ChatRequest` and returns
  `CompletionResult` with tool calls or final text
- Downstream: [Memory Management](base/memory.md) provides `ConversationHistory` per
  room and receives new messages for archival
- Downstream: [Knowledge Management](base/knowledge.md) extracts and persists
  domain facts, loads entries into agent context on room init
 - Downstream: Individual tools (see `tools/` directory) are registered in
   `ToolRegistry` and invoked by the agent loop via `execute_by_name()`
 - Shared: `ImageCache` (`image_cache.rs`) stores `GeneratedImage` entries keyed by call_id for the image upload pipeline (§2i)

## 2. Diagram

### 2a. Agent Loop (Main Success Path)

```mermaid
flowchart TD
    RC[RocketChat]
    ROUTE(RouteByRoom)
    CTX(BuildContext)
    MEM[(ConversationHistory)]
    TOOLS_DEF[(ToolRegistry)]
    INTERACT(InteractWithAi)
    AI[AiProvider]
    MRK_DIRTY(MarkSnapshotDirty)

    RC -->|"incoming message"| ROUTE
    ROUTE -->|"routed message"| CTX
    MEM -->|"history for room"| CTX
    TOOLS_DEF -->|"tool definitions"| INTERACT
    CTX -->|"chat request"| INTERACT
    INTERACT -->|"chat request"| AI
    AI -->|"completion result"| INTERACT
    INTERACT -->|"bot reply"| RC
    INTERACT -->|"reply produced"| MRK_DIRTY
```

After every response (including errors and fallbacks), the room is marked dirty for
snapshot persistence. The room is also marked dirty immediately when a new user message
is appended to history. The periodic maintenance timer (every `persist_interval_secs`)
flushes all dirty snapshots to WebDAV.

### 2b. Error Handling & Fallbacks

```mermaid
flowchart TD
    AI[AiProvider]
    TOOL_EXEC(ExecuteTool)
    LOOP_LIMIT(CheckMaxIterations)
    APPEND(AppendToolResult)
    FALLBACK(SendFallbackReply)
    COMPRESS{ContextLength<br/>Exceeded?}
    STRIP(CompressHistoryForRetry<br/>strip images + prune to 6 msgs)
    REBUILD(HardTruncate<br/>keep system prefix + last 2 msgs)
    RETRY(Retry LLM Call)
    REPLY[BotReply]

    AI -.->|"api error response"| COMPRESS
    COMPRESS -.->|"yes (first time)"| STRIP
    COMPRESS -.->|"no (other error)"| FALLBACK
    STRIP -.->|"rebuilt messages"| REBUILD
    REBUILD -.->|"retry"| RETRY
    RETRY -.-> AI
    TOOL_EXEC -.->|"tool execution error"| APPEND
    LOOP_LIMIT -.->|"max iterations exceeded"| FALLBACK
    FALLBACK -->|"error reply text"| REPLY
```

### 2c. Agent Loop Deep Dive

Level 2 decomposition of the invariant agent loop (`while True: LLM → tools →
LLM`): queries the AI provider, executes any tool calls, feeds results back, and
loops until a final text reply is produced.

```mermaid
flowchart TD
    CTX[BuildContext]
    AI[AiProvider]
    ASSESS(AssessCompletion)
    EXEC(ExecuteTool)
    APPEND(AppendToolResult)
    LIMIT(CheckIterationLimit)
    TRUNC(TruncateAndSummarize)
    CTX_ERR{ContextLength<br/>Exceeded?}
    STRIP_COMPRESS(CompressHistoryForRetry<br/>strip images + prune to 6 msgs)
    REBUILD(HardTruncate<br/>keep system prefix + last 2 msgs)
    REPLY_OUT[BotReply]

    CTX -->|"chat request"| AI
    AI -->|"completion result"| ASSESS
    ASSESS -->|"tool calls"| EXEC
    ASSESS -->|"final reply text"| REPLY_OUT
    EXEC -->|"tool result"| APPEND
    APPEND -->|"updated messages"| CTX
    CTX -->|"context byte size"| LIMIT
    LIMIT -.->|"exceeds max_context_bytes"| TRUNC
    TRUNC -->|"summarized messages"| CTX
    EXEC -.->|"tool execution error"| APPEND
    AI -.->|"api error"| CTX_ERR
    CTX_ERR -.->|"yes (first time)"| STRIP_COMPRESS
    CTX_ERR -.->|"no (other error)"| REPLY_OUT
    STRIP_COMPRESS -.->|"stripped history"| REBUILD
    REBUILD -.->|"retry request"| CTX
```

### 2d. Tool Execution Deep Dive

Room context (`room_id` UUID + `webdav_dir` path key) is injected into
stateful tools that need it (tools backed by WebDAV or room-scoped storage).
Stateless tools (web search, fetch, datetime, etc.) receive raw arguments
without room context. The `ToolRegistry` maps tool names to implementations;
calls are dispatched generically via `execute_by_name()`.

```mermaid
flowchart TD
    CALL[ToolCall]
    INJECT{Stateful?}
    ROOM_CTX[(RoomState<br/>room_id + webdav_dir)]
    REG[(ToolRegistry)]
    EXEC(ExecuteToolByName)
    RESULT[ToolResult]

    CALL -->|"tool name + args"| INJECT
    ROOM_CTX -->|"room_id + webdav_dir"| INJECT
    INJECT -->|"stateful: enriched args"| EXEC
    INJECT -->|"stateless: raw args"| EXEC
    REG -->|"tool implementations"| EXEC
    EXEC -->|"formatted result"| RESULT
```

### 2e. Auto-Attachment Vision Pipeline

When an incoming message contains image attachments (`IncomingMessage.attachments`
is non-empty), the harness downloads each attachment, encodes it as a base64 data
URI, and embeds it directly in the user's `ChatMessage` as `ContentPart::ImageUrl`
parts. The agent harness natively "sees" these images — no tool call is involved.
The vision tool is only invoked by the LLM for images at public URLs or WebDAV
file URLs.

```mermaid
flowchart TD
    RC[IncomingMessage]
    CHECK{HasAttachments?}
    EXTRACT(Extract image URLs)
    DOWNLOAD(Download attachments)
    ENCODE(Base64 encode)
    EMBED(ChatMessage::user_with_images)
    HIST[(ConversationHistory)]
    BUILD(BuildContext)
    AI[AiProvider]

    RC -->|"message with text + attachments"| CHECK
    CHECK -->|"yes"| EXTRACT
    CHECK -->|"no"| BUILD
    EXTRACT -->|"full download URLs"| DOWNLOAD
    DOWNLOAD -->|"image bytes"| ENCODE
    ENCODE -->|"data uris"| EMBED
    RC -->|"user text"| EMBED
    EMBED -->|"user msg + ImageUrl parts"| HIST
    HIST -->|"messages with images"| BUILD
    BUILD -->|"chat request with images"| AI
```

**Image selection**: uses `attachments[0].title_link` (original file) over
`image_url` (thumbnail). The server base URL is prepended to construct the full
download URL: `{server_config.host()}{title_link}`. Multiple attachments are
supported — all are encoded and embedded in the same message.

**Prompt construction**: if the user included text with the image (e.g. "B78"),
that text is prepended with the sender name (e.g. "User: B78"). If no text is
present, the prompt becomes `"SenderName: Describe this image in detail."`.

**Chat history preservation**: when `build_context()` builds messages for the AI
provider, `ContentPart::ImageUrl` parts are preserved only on the most recent
user message. Earlier user messages with images are collapsed to `[image]` text
placeholders (see `memory.rs:strip_images_from_message`).

**Text-only LLM handling**: after context is built, text-only providers (DeepSeek)
additionally strip all `ImageUrl` parts from every message — including the
most recent — replacing them with `[image]` placeholders via
`strip_message_images()` at the provider layer. This is a provider-level
concern separate from memory compression; the harness always embeds images
in `ChatMessage` regardless of the provider. See
[ai-provider.md §2c](../base/ai-provider.md#2c-vision-payload-deep-dive).

### 2f. Per-Room State Routing

Each room maintains independent state — conversation history, agent context, and
WebDAV archive path. The agent routes incoming messages to the correct room's
pipeline. Room context (`room_id` UUID + `webdav_dir` path key) is computed from
`room_name`, `room_fname`, and `is_dm` and injected into stateful tool calls
(tools backed by WebDAV or room-scoped storage).

```mermaid
flowchart TD
    RC[IncomingMessage]
    ROOM_MAP[(RoomStateMap)]
    RESOLVE(ResolveRoomState)
    NEW_ROOM(InitializeRoom)
    MEM[(InMemoryHistory)]
    COMPUTE(ComputeWebdavDir)
    INJECT(InjectRoomContext)
    INACT(InteractWithAi)
    REPLY[BotReply]
    DAV[(WebDAV room memory)]

    RC -->|"room id"| ROOM_MAP
    ROOM_MAP -->|"room state or not found"| RESOLVE
    RESOLVE -->|"new room context"| NEW_ROOM
    RESOLVE -->|"existing room context"| INACT
    NEW_ROOM -->|"load archives request"| DAV
    DAV -->|"archive files"| NEW_ROOM
    NEW_ROOM -->|"archived messages"| MEM
    NEW_ROOM -->|"initialized state"| ROOM_MAP
    MEM -->|"conversation history"| INACT
    MEM -->|"room_name + room_fname + is_dm"| COMPUTE
    COMPUTE -->|"webdav_dir"| INJECT
    INJECT -->|"enriched tool args"| INACT
    INACT -->|"bot reply"| REPLY
```

### 2g. Vision Image Injection

After the vision tool returns, the harness intercepts the tool result, parses
base64 data URIs from markdown image tags, caches them in a per-room image pool,
and injects them as `ContentPart::ImageUrl` parts in a synthetic user message
before the next LLM call. The pool is drained on each injection — images are
ephemeral, used for a single LLM cycle.

```mermaid
flowchart TD
    VISION_RESULT["Vision Tool Result<br/>(image data URIs)"]
    CACHE(CacheVisionImages<br/>parse data URIs)
    POOL[(ImagePool<br/>HashMap<room_id, Vec<CachedImage>>)]
    BUILD(BuildContext)
    INJECT(InjectVisionImages<br/>drain pool, label photoN.ext)
    AI[AiProvider]

    VISION_RESULT -->|"tool content text"| CACHE
    CACHE -->|"CachedImage { data_uri, name }"| POOL
    BUILD -->|"context messages"| INJECT
    POOL -->|"drain images"| INJECT
    INJECT -->|"user msg + ImageUrl parts"| BUILD
    BUILD -->|"chat request with images"| AI
```

This bridges the gap between the vision tool (which returns plain text) and
the AI provider's multimodal requirement (which needs structured
`ContentPart::ImageUrl` parts). The same pipeline also intercepts `webdav`
tool results — when the webdav tool reads an image file, it returns a base64
markdown tag (`![name](data:...)`), and the harness caches it identically to
vision results. This lets the LLM inspect images from WebDAV storage without
a separate vision tool call. Injection happens at two points in the agent loop:
(1) before the first LLM call for a message, and (2) after each tool-execution
iteration before the next LLM call.

### 2g2. Image Interception for Editing — inject_image_urls_from_refs

When the LLM calls `image_gen` for editing, the harness intercepts the arguments
and injects real image data from **four sources** into `image_urls`:

1. **User attachments** — downloaded from RocketChat as `data:` URIs, matched by
   filename substring in the LLM prompt (e.g. "edit apple.png")
2. **Vision/webdav-fetched images** — stored in `image_pool` as
   `CachedImage { data_uri, name }`, matched by name or `![name]` label in prompt
3. **Agent-provided URLs** — any `share_url` or `https://` URL the LLM explicitly
   includes in `image_urls` (e.g. from a previous `image_gen` result)
4. **Message image URLs** — from `IncomingMessage.urls` (filtered by
   `content_type: image/*`). Auto-injected unconditionally — no prompt matching
   required — so text-only models can edit images without vision.

```mermaid
flowchart TD
    ATTACH["1. User Attachments<br/>download_attachment_refs"]
    VISION["2. Vision/WebDAV Fetch<br/>cache_vision_images"]
    AGENT_URL["3. Agent-Provided URLs<br/>share_url from prev result"]
    MSG_URL["4. Message Image URLs<br/>current_image_urls<br/>(from DDP urls field,<br/>filtered content_type image/*)"]
    POOL[(ImagePool<br/>room_id → Vec<CachedImage>)]
    INJECT[inject_image_urls_from_refs]

    ATTACH -->|"data: URIs"| INJECT
    VISION -->|"parse ![name](data:...)"| POOL
    POOL -->|"match by name"| INJECT
    AGENT_URL -->|"https:// or data: URLs"| INJECT
    MSG_URL -->|"auto-inject (unconditional)"| INJECT
    INJECT -->|"args['image_urls']"| IMG_GEN[image_gen Tool]
```

All four sources converge in the `image_gen` argument injection: attachment data
URIs and image_pool entries matched by prompt text, agent-provided URLs merged
with deduplication, and message image URLs auto-injected unconditionally. The
`image_gen` tool then uploads `data:` URIs to the provider's CDN (Fal) or passes
`https://` URLs directly. Deduplication is by URL string equality.

This covers all five image sources for editing:
- Previous `image_gen` results (via agent-provided `share_url` in `image_urls`)
- User-attached images (via `AttachmentRef` title match)
- Vision-fetched images from public URLs (via `image_pool` name match)
- WebDAV-read images (via `image_pool` name match, same pipeline as vision)
- DDP message URLs with image content types (via `current_image_urls` — auto-injected without prompt matching)

### 2h. Daily Summary Review — Knowledge Retrieval Ordering

After each daily summary write (archive) and during periodic maintenance, the
harness calls `review_knowledge_priorities()` which iterates over all rooms
but delegates to `KnowledgeManager::review_priorities()` — currently a no-op
that returns `Ok(false)`.
Knowledge retrieval uses `match_relevant()` which scores entries by keyword
overlap against `when_useful`, `tags`, and filename-derived title. Priority-based
scoring was removed when the knowledge index was simplified.

```mermaid
flowchart TD
    ARCHIVE["archive_room_if_needed()<br/>(after upsert_daily_summary)"]
    TIMER["Maintenance Timer<br/>(every persist_interval_secs)"]
    REVIEW["review_knowledge_priorities()<br/>(no-op)"]
    SKIP["(no action)"]

    ARCHIVE -->|"post-archive trigger"| REVIEW
    TIMER -->|"periodic trigger"| REVIEW
    REVIEW --> SKIP
```

### 2i. Inline Context Overflow — truncate_and_summarize

Before each LLM call, the harness checks if the total JSON byte size of the
messages exceeds `max_context_bytes`. If so, it summarises older messages
inline — replacing them with a concise AI-generated summary — without
touching WebDAV. This is a separate mechanism from the Layer 1→Layer 2
archive (which writes daily summaries to WebDAV).

```mermaid
flowchart TD
    CTX[BuildContext Messages]
    CHECK{"current_bytes<br/>> max_context_bytes?"}
    SPLIT["Split messages:<br/>prefix (system) + older + suffix (last 2)"]
    SUMMARIZE["AI Summarize<br/>(older messages → 1-3 sentences)"]
    AI[AiProvider<br/>one-shot, tools=off]
    REPLACE["Replace older messages<br/>with [summary] system msg"]
    OUT[Return Summarized Messages]

    CTX --> CHECK
    CHECK -->|"no"| OUT
    CHECK -->|"yes"| SPLIT
    SPLIT -->|"older message texts (trimmed)"| SUMMARIZE
    SUMMARIZE -->|"summary prompt"| AI
    AI -->|"summary text"| REPLACE
    REPLACE --> OUT
```

**Fallback**: if the AI summarization fails, falls back to plain text
`"N earlier messages (truncated due to context limit)"`. At least the
last 2 messages plus the system prompt are always preserved. If the total
message count is ≤ system prefix + 4, summarization is skipped entirely
regardless of byte limit.

### 2i2. Context-Length-Exceeded Retry — Provider-Triggered Compression

When the AI provider returns a `ContextLengthExceeded` error (HTTP 400 with
"context length" or "maximum context" in the error message), the harness
performs aggressive memory compression and retries the request once. This
is a provider-initiated recovery path that complements the proactive
`truncate_and_summarize` byte-check.

```mermaid
flowchart TD
    AI[AiProvider]
    CHECK{ContextLengthExceeded?}
    COMPRESS["CompressHistoryForRetry<br/>(strip images + prune to 6 msgs)"]
    REBUILD["HardTruncate<br/>(keep system prefix + last 2 msgs)"]
    RETRY["Retry LLM Call"]
    FALLBACK["SendErrorFallback<br/>(already compressed once)"]
    REPLY[BotReply]

    AI -->|"error response"| CHECK
    CHECK -->|"yes (first time)"| COMPRESS
    CHECK -->|"yes (already compressed)"| FALLBACK
    CHECK -->|"no (other error)"| FALLBACK
    COMPRESS -->|"stripped history"| REBUILD
    REBUILD -->|"rebuilt messages"| RETRY
    RETRY --> AI
    FALLBACK --> REPLY
```

**Aggressive compression** (`compress_history_for_retry`): 
1. Strips all `ContentPart::ImageUrl` parts from every message except the last
   one, converting them to `[image]` text placeholders
2. Prunes chat history to the last 6 messages to reduce text token load

Then rebuilds context with `max_history: Some(4)` and applies **hard truncation**
(no LLM summarization — that call would also exceed context if it included the
oversized text): keep system/front-matter messages at the front, and only the
last 2 conversation messages at the end. After hard truncation, **per-message
content truncation** caps each remaining conversation message at 200K chars to
handle cases where individual tool results or user pastes are themselves
enormous.

**Retry limit**: compression is attempted at most once per call. If the
provider still returns `ContextLengthExceeded` after compression, the
harness falls back to the standard error reply. The `context_compressed`
flag is per-`process_message` call, not per-room.

This recovery path handles token-limit breaches that the byte-based
`max_context_bytes` check cannot catch (e.g., base64-encoded images that
are small in bytes but consume many tokens).

### 2j. Generated Image Sharing via NextCloud Share Links

The `image_gen` tool creates a NextCloud public share link (7-day expiry)
during tool execution and stores it in `ImageCache`. The harness records the
call IDs in `last_image_ids` and returns the LLM's reply text with the
`image_key` placeholder still intact — **the harness does not modify the
reply text for images**. After `process_message()` returns, the agent loop
(main.rs) retrieves the IDs, takes images from the cache, replaces the
placeholder with the share URL markdown, and sends the final message.

```mermaid
flowchart TD
    LLM_TEXT["text reply<br/>(contains ![](image_key))"]
    IMG_IDS["last_image_ids<br/>(take_last_image_ids)"]
    CACHE[(ImageCache)]
    TAKE(CacheTake<br/>by call_id)
    SHARE{"share_url<br/>present?"}
    MARKDOWN["Append<br/>![Generated image](share_url)"]
    FALLBACK["Build DDP attachment<br/>with data_uri()"]
    STRIP["StripMarkdownImageId<br/>(remove ![](image_key))"]
    PICK["final_reply =<br/>reply_text (images) or<br/>reply (no images)"]
    SEND(SendReply<br/>via REST or DDP)
    RC[RocketChat]

    LLM_TEXT --> STRIP
    IMG_IDS --> TAKE
    TAKE -->|"lookup by call_id"| CACHE
    CACHE -->|"GeneratedImage"| SHARE
    STRIP -->|"remove ![](image_key)"| PICK
    SHARE -->|"yes (preferred)"| MARKDOWN
    SHARE -->|"no (fallback)"| FALLBACK
    MARKDOWN -->|"append ![Generated image](share_url)"| PICK
    FALLBACK --> SEND
    PICK -->|"final_reply"| SEND
    SEND -->|"chat.sendMessage"| RC
```

**Harness role** (`process_message`): tracks `image_ids_this_turn` during
the agent loop, stores them in `last_image_ids` before returning, and exposes
them via `take_last_image_ids()`. Returns the LLM's reply text unmodified —
the `image_key` placeholder is still present for main.rs to replace.

**ImageGenTool role**: calls `create_nextcloud_share_link()` on `WebDavClient`
right after WebDAV upload — `POST /ocs/v2.php/apps/files_sharing/api/v1/shares`
with `shareType=3`, `permissions=1`, `expireDate={today+7d}`. Stores the
result (with `/download` suffix for direct raw image access) as
`GeneratedImage.share_url`. Stores the entire `GeneratedImage` in `ImageCache`
keyed by `call_id`.

**Agent loop role** (main.rs): after `process_message()` returns, calls
`take_last_image_ids()` to get the call IDs, then `take_image()` for each one.
If `share_url` is present, appends `![Generated image]({share_url}/download)`
to the reply text and strips the original `![desc](image_key)` markdown.
If `share_url` is `None`, falls back to a DDP attachment with `data_uri()`.
Uses `final_reply` (the modified text or original) for the outgoing message.

**Fallback**: if share generation fails (`share_url` is `None`), the agent loop
builds a DDP `sendMessage` with a `data:` URI in the `attachments` field, using
`GeneratedImage::data_uri()`. This is a worst-case path for when NextCloud's
sharing API is unavailable.

**Design rationale**: NextCloud share URLs are short — the `/download` endpoint
returns raw image bytes with correct `Content-Type` for inline rendering in
RocketChat. This eliminates both the `Message_MaxAllowedSize` REST limit
(short URL, no base64 in msg text) and the DDP attachments schema restriction
(Match failed [400]). Share links expire after 7 days, longer than typical
chat message lifetimes.

## 3. Data Structures

- **AgentContext** — does not exist as a struct. The harness constructs these values on the fly: `system_prompt` is built by `build_system_prompt()`, `history` by `build_context()`, `tools` by `ToolRegistry::definitions()`, `room_id` is a method parameter, `webdav_dir` is computed by `compute_webdav_dir()`.

#### `ToolResult`

| Field      | Type     | Notes                                      |
| ---------- | -------- | ------------------------------------------ |
| `call_id`  | `String` | Matches `ToolCall.id`                      |
| `name`     | `String` | Tool name                                  |
| `content`  | `String` | Result text (returned to LLM as tool msg)  |
| `is_error` | `bool`   | True if tool execution failed              |

#### `ToolRegistry`

| Field      | Type                    | Notes                          |
| ---------- | ----------------------- | ------------------------------ |
| `tools`    | `HashMap<String, Box<dyn Tool>>` | Name → implementation |

#### `ToolDef`

| Field           | Type            | Notes                                   |
| --------------- | --------------- | --------------------------------------- |
| `tool_type`     | `String`        | Always `"function"`                     |
| `function`      | `FunctionDef`   | Nested function definition object       |

#### `FunctionDef`

| Field        | Type            | Notes                                   |
| ------------ | --------------- | --------------------------------------- |
| `name`       | `String`        | Function name                           |
| `description`| `Option<String>`| Human-readable description for the LLM  |
| `parameters` | `Option<Value>` | JSON Schema for arguments               |
| `strict`     | `Option<bool>`  | Whether to enforce strict schema        |

#### `GeneratedImage` (ImageCache Entry)

Stored in `Arc<Mutex<HashMap<String, GeneratedImage>>>` keyed by tool call_id.

| Field          | Type           | Description                                   |
| -------------- | -------------- | --------------------------------------------- |
| `webdav_path`  | `string`       | WebDAV path where the image was persisted     |
| `image_bytes`  | `Vec<u8>`      | Raw image bytes for fallback data URI         |
| `mime_type`    | `string`       | MIME type, e.g. `image/png`                  |
| `share_url`    | `Option<string>`| NextCloud public share link (7-day expiry)    |

#### Registered Tools

Tools are registered at startup via `ToolRegistry::register()`. Each tool
implements the `Tool` trait (`name`, `description`, `parameters`, `execute`).
The registry exposes `definitions()` for the LLM and dispatches calls via
`execute_by_name()`. See individual tool DFDs under `tools/` for each tool's
implementation.
