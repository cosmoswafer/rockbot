# Agent Loop

## 1. Purpose

Shows how all subsystems — messaging platform client, AI provider, tools, memory,
WebDAV, config — are wired together to run the agent harness. This is the
top-level process decomposition of RockBot: a single event loop that connects to
a messaging platform (RocketChat or Matrix, selected by `[platform] name`),
routes incoming messages to the agent harness, executes tool calls,
manages per-room memory, and persists everything to WebDAV.

The messaging platform is abstracted behind a `MessagingClient` trait
(defined in `crate-rockbot/src/platform/mod.rs`). Both `RocketChatPlatform`
and `MatrixPlatform` implement this trait, producing the same `IncomingMessage`
type and consuming `BotReply`. The agent harness and tool execution are
platform-agnostic.

- Upstream: [Configuration Management](../infra/config.md) provides `AppConfig`
- Upstream: [Boot Sequence](boot-sequence.md) covers config loading, provider
  selection, tool registration, and platform connection — all boot steps
  that precede entering this event loop
- Downstream: [Agent Harness](agent-harness.md) receives `IncomingMessage` and
  returns `BotReply` (see agent-harness.md for loop internals and tool execution)
- Downstream: [RocketChat Connection](../infra/rocketchat.md) or
  [Matrix Connection](../infra/matrix.md) — implements `MessagingClient` trait
- Downstream: [RocketChat REST API](../infra/rocketchat-rest.md) handles room name
  resolution and alias message sending (RocketChat-only production path for bot replies)
- Downstream: [AI Provider](../ai/ai-provider.md) handles chat completion requests
- Downstream: [Memory Management](../memory/memory.md) manages per-room conversation history,
  compression (threshold-based, produces summary.md — see [Memory Compression](../memory/memory-compression.md)),
  snapshot persist, and TTL-based room eviction
- Downstream: [WebDAV Tool](../tools/webdav.md) persists image assets

## 2. Diagram

### 2a. Happy Flow (Main Success Path)

```mermaid
flowchart TD
    PLATFORM[Messaging Platform<br/>RocketChat or Matrix]
    AI[AI Provider API]
    DAV[(NextCloud WebDAV)]
    EXA[Exa Search API]
    TIMER[Evict Timer]
    DISPATCH(ReceiveMessage)
    TYPING(ToggleTyping)
    LOOP(AgentLoop)
    DIRTY(MarkSnapshotDirty)
    SNAPSHOT(FlushSnapshots)
    ARCHIVE(CompressMemory)

    CFG[(AppConfig)]
    HISTORY[(ConversationHistory)]
    TOOLS[(ToolRegistry)]
    ROOMS[(RoomStateMap)]

    PLATFORM -->|"incoming message"| DISPATCH
    ROOMS -->|"room state"| DISPATCH
    CFG -->|"app config"| DISPATCH
    DISPATCH -->|"incoming message"| TYPING
    TYPING -->|"typing on"| PLATFORM
    TYPING -->|"incoming message"| LOOP
    CFG -->|"ai config"| LOOP
    HISTORY -->|"conversation history"| LOOP
    TOOLS -->|"tool definitions"| LOOP
    LOOP -->|"chat request"| AI
    AI -->|"completion result"| LOOP
    LOOP -->|"typing off"| PLATFORM
    LOOP -->|"bot reply"| PLATFORM
    LOOP -->|"reply produced<br/>(every response)"| DIRTY
    DIRTY -->|"dirty flag"| ROOMS
    PLATFORM -->|"reply delivered"| ARCHIVE
    ARCHIVE -->|"compress + identify prompt"| AI
    AI -->|"summary.md + used knowledge"| ARCHIVE
    ARCHIVE -->|"also marks dirty"| DIRTY
    ARCHIVE -->|"pruned history"| HISTORY
    LOOP -->|"updated room state"| ROOMS
    TIMER -->|"every persist_interval_secs"| SNAPSHOT
    ROOMS -->|"dirty rooms"| SNAPSHOT
    SNAPSHOT -->|"snapshot.json<br/>(history + summary + soul)"| DAV
    TIMER -->|"every persist_interval_secs"| EVICT_ROOMS
    ROOMS -->|"all rooms"| EVICT_ROOMS
    EVICT_ROOMS -->|"snapshot.json for stale rooms"| DAV
    EVICT_ROOMS -->|"remove stale entries"| ROOMS
```

### 2b. Error Handling & Fallbacks

```mermaid
flowchart TD
    START(BootSystem)
    CONN(MessagingPlatform Connection)
    AI[AI Provider API]
    DAV[(NextCloud WebDAV)]
    RECONNECT(ReconnectWithBackoff)
    FALLBACK(SendFallbackReply)
    SIGINT([SIGINT / Ctrl+C])
    SIGTERM([SIGTERM / pkill])
    SHUTDOWN(GracefulShutdown)
    ABORT_TIMER(AbortMaintenanceTimer)
    FLUSH(FlushAllSnapshots)

    START -.->|"auth failure error"| RECONNECT
    CONN -.->|"connection lost error"| RECONNECT
    CONN -.->|"connection closed ok"| SHUTDOWN
    RECONNECT -.->|"reconnect signal"| CONN
    RECONNECT -.->|"max retries exhausted"| SHUTDOWN
    AI -.->|"api error response"| FALLBACK
    SIGINT -.->|"SIGINT"| SHUTDOWN
    SIGTERM -.->|"SIGTERM"| SHUTDOWN
    SHUTDOWN -->|"1. abort"| ABORT_TIMER
    ABORT_TIMER -->|"2. acquire lock"| FLUSH
    FLUSH -->|"snapshot.json per dirty room"| DAV
```

On graceful shutdown (SIGINT, SIGTERM, normal connection close, or max reconnect retries), the bot:
1. Aborts the periodic maintenance timer to prevent races on the harness mutex.
2. Acquires the harness lock and calls `flush_all_snapshots()`, which iterates every dirty room (soul/knowledge/summary changes), builds a `PersistSnapshot`, serializes to JSON, and uploads `snapshot.json` to WebDAV via `write_file_with_fallback`.

Typing indicator failures are non-critical: if `sender.typing()` returns an error (e.g. WebSocket disconnected), the heartbeat task silently catches it and stops refreshing. The main agent loop is unaffected — it continues processing and sends the reply without typing cleanup.

### 2c. Typing Indicator Heartbeat

Level 2 decomposition of `ToggleTyping` and the typing flows during `AgentLoop`. The bot sends an initial `typing=true` signal before the agent loop begins, then a background task refreshes it every 2 seconds while the loop runs. When the loop produces a reply (or errors out), typing is set to `false`.

```mermaid
flowchart TD
    PLATFORM[Messaging Platform]
    MSG[IncomingMessage]
    START_TYPING(SendTypingOn)
    HEARTBEAT(RefreshTyping)
    TIMER((Every 2s))
    PROCESS(AgentLoop)
    STOP_TYPING(SendTypingOff)
    REPLY[BotReply]

    MSG -->|"dispatch"| START_TYPING
    START_TYPING -->|"typing indicator on"| PLATFORM
    START_TYPING -->|"start loop"| PROCESS
    PROCESS -->|"while processing"| HEARTBEAT
    TIMER -->|"tick"| HEARTBEAT
    HEARTBEAT -->|"typing indicator on"| PLATFORM
    PROCESS -->|"reply ready or error"| STOP_TYPING
    STOP_TYPING -->|"typing indicator off"| PLATFORM
    STOP_TYPING -->|"send"| REPLY
```

The heartbeat task is a `tokio::spawn` that runs concurrently with the agent loop, refreshing the typing indicator every 2 seconds. If the WebSocket disconnects, `sender.typing()` returns an error — the heartbeat task breaks its loop and exits silently. The main agent loop is unaffected.

Typing indicator state is intentionally not retried or persisted — it is a transient UI affordance with no durability requirements.

## 3. Data Structures

#### `MessagingClient` trait (platform/mod.rs)

Async trait defining the contract between the agent loop and any messaging
platform. Both `RocketChatPlatform` and `MatrixPlatform` implement this trait.

```rust
#[async_trait]
pub trait MessagingClient: Send + Sync {
    async fn connect_and_run(
        &self,
        handler: impl Fn(IncomingMessage) -> Fut + Send + Sync,
    ) -> Result<()>;

    async fn send_reply(
        &self,
        room_id: &str,
        text: &str,
        alias: Option<&str>,
    ) -> Result<()>;

    async fn send_typing(
        &self,
        room_id: &str,
        typing: bool,
    ) -> Result<()>;

    fn bot_user_id(&self) -> &str;
}
```

- `connect_and_run` — long-running method that connects to the platform,
  subscribes to messages, and invokes `handler` for each `IncomingMessage`.
  Returns `Err` on connection failure (triggers reconnect in the agent loop).
  For RocketChat: DDP WebSocket connect → auth → subscribe → event loop.
  For Matrix: login → start `/sync` loop → dispatch room messages.
- `send_reply` — send a text message to a room. For RocketChat: REST
  `chat.sendMessage` with alias fallback to DDP. For Matrix:
  `Client::room_send()` with `RoomMessageEventContent::text_plain()`.
- `send_typing` — send typing indicator. For Matrix: `m.typing` state event.
- `bot_user_id` — the platform-specific user ID used for self-message filtering.

> **Design note**: The trait is intentionally narrow — it only covers the
> bidirectional message flow. Platform-specific features (REST alias, file
> uploads, room name resolution) remain on the concrete types and are accessed
> by the agent loop through platform-specific code paths or optional trait
> extensions.

#### `AgentHarness` (harness.rs:55-65)

| Field            | Type                  | Notes                                      |
| ---------------- | --------------------- | ------------------------------------------ |
| `config`         | `Arc<AppConfig>`      | Immutable configuration shared across subsystems |
| `provider`       | `Box<dyn AiProvider>` | AI provider for chat completions           |
| `memory`         | `MemoryManager`       | Per-room conversation history              |
| `tools`          | `ToolRegistry`        | Registered tool definitions                |
| `webdav`         | `Option<WebDavClient>`| Optional WebDAV handle for persistent storage |
| `rest_client`    | `Option<RestApiClient>`| Optional REST API client for alias sends  |
| `max_iterations` | `u32`                 | Max agent loop iterations per message      |
| `max_attachment_bytes` | `u64`           | Max size for attachment download           |
| `image_pool`     | `HashMap<String, Vec<CachedImage>>` | Per-room cached images from vision/webdav tool results and image_gen (for edit name-matching) |
| `image_cache`    | `Arc<ImageCache>`     | Generated image cache (by call_id)         |
| `last_image_ids` | `Vec<String>`         | IDs of images generated this turn          |
| `current_image_urls` | `Vec<String>`     | Image URLs from current message (auto-injected into image_gen) |

#### `RoomState`

| Field           | Type                | Notes                                      |
| --------------- | ------------------- | ------------------------------------------ |
| `room_id`       | `String`            | RocketChat room UUID (in-memory lookup key) |
| `room_name`     | `String`            | URL slug (ASCII)                           |
| `room_fname`    | `String`            | Friendly display name (Unicode)            |
| `is_dm`         | `bool`              | True if direct message room                |
| `history`       | `ConversationHistory`| In-memory message buffer for this room     |
| `last_activity` | `u64`               | Unix timestamp of last interaction; checked against `memory_ttl_secs` for eviction |

`webdav_dir` is not a stored field — it is computed on-the-fly from `room_name`/`room_fname`/`is_dm` via `compute_webdav_dir()`.

The main loop uses `tokio::signal::unix::signal(SignalKind::terminate())` raced with
`tokio::signal::ctrl_c()` for shutdown (both SIGTERM and SIGINT), and a local
`retry_count: u32` variable for reconnect backoff. Graceful shutdown calls
`AgentHarness::flush_all_snapshots()` (harness.rs:1126) to sync dirty per-room
state to WebDAV before exiting.

## 4. Non-Functional Requirements

- **No local file access**: The agent loop and all subsystems MUST NOT read from or
  write to the local filesystem at runtime. The only local file read is `config.toml`
  (and `default.config.toml`) at startup. All persistent state lives in WebDAV.
  Exception: `matrix-rust-sdk` stores an encryption key store and sync state in a
  configurable state directory (`[matrix.server] state_dir`, default `./tmp/matrix-sdk`).
- **No tool touches local files**: Every tool (web_fetch, webdav, calendar,
  vision, web_search, image_gen, edit_soul, knowledge tools) MUST NOT access the
  local filesystem. All I/O goes through WebDAV or HTTP.
- **Config-only startup**: The application only loads `config.toml` (merged with
  `default.config.toml`) on startup. No other local files are read or created.
- **Avatar from URL only**: Avatar changes use the `users.setAvatar` REST API
  (`setAvatarFromService` DDP method) with a URL parameter. Local file paths are
  never used for avatar images.
- **Platform parity**: The `IncomingMessage` type is shared across all platforms.
  Platform-specific fields (e.g. Matrix `event_id`, RocketChat `msg_id`) are mapped
  to the shared `msg_id: Option<String>` field. The agent harness and tools MUST NOT
  depend on platform-specific message fields.
