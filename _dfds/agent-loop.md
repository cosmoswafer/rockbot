# Agent Loop

## 1. Purpose

Shows how all subsystems — RocketChat client, AI provider, tools, memory,
WebDAV, config — are wired together to run the agent harness. This is the
top-level process decomposition of RockBot: a single event loop that connects to
RocketChat, routes incoming messages to the agent harness, executes tool calls,
manages per-room memory, and persists everything to WebDAV.

- Upstream: [Configuration Management](base/config.md) provides `AppConfig`
- Downstream: [Agent Harness](agent-harness.md) receives `IncomingMessage` and
  returns `BotReply` (see agent-harness.md for loop internals and tool execution)
- Downstream: [RocketChat Connection](base/rocketchat.md) handles auth, WebSocket
  streaming, and message filtering
- Downstream: [AI Provider](base/ai-provider.md) handles chat completion requests
- Downstream: [Memory Management](base/memory.md) manages per-room conversation history,
  archive (threshold-based daily compress), snapshot persist, and TTL-based room eviction
- Downstream: [WebDAV Tool](tools/webdav.md) persists image assets

## 2. Diagram

### 2a. Happy Flow (Main Success Path)

```mermaid
flowchart TD
    RC[RocketChat Server]
    AI[AI Provider API]
    DAV[(NextCloud WebDAV)]
    EXA[Exa Search API]
    TIMER[Evict Timer]
    DISPATCH(ReceiveMessage)
    TYPING(ToggleTyping)
    LOOP(AgentLoop)
    ARCHIVE(CompressDaily)
    EVICT_ROOMS(EvictStaleRooms)
    PERSIST_ASSETS(PersistAssets)
    CFG[(AppConfig)]
    HISTORY[(ConversationHistory)]
    TOOLS[(ToolRegistry)]
    ROOMS[(RoomStateMap)]

    RC -->|"incoming message"| DISPATCH
    ROOMS -->|"room state"| DISPATCH
    CFG -->|"app config"| DISPATCH
    DISPATCH -->|"incoming message"| TYPING
    TYPING -->|"typing on"| RC
    TYPING -->|"incoming message"| LOOP
    CFG -->|"ai config"| LOOP
    HISTORY -->|"conversation history"| LOOP
    TOOLS -->|"tool definitions"| LOOP
    LOOP -->|"chat request"| AI
    AI -->|"completion result"| LOOP
    LOOP -->|"search query"| EXA
    EXA -->|"search results"| LOOP
    LOOP -->|"typing off"| RC
    LOOP -->|"bot reply"| RC
    LOOP -->|"new message"| ARCHIVE
    LOOP -->|"image asset"| PERSIST_ASSETS
    ARCHIVE -->|"summary prompt"| AI
    AI -->|"summary text"| ARCHIVE
    ARCHIVE -->|"daily summary + soul"| PERSIST_ASSETS
    PERSIST_ASSETS -->|"file data"| DAV
    DAV -->|"file data"| PERSIST_ASSETS
    ARCHIVE -->|"pruned history"| HISTORY
    LOOP -->|"updated room state"| ROOMS
    TIMER -->|"every persist_interval_secs"| EVICT_ROOMS
    ROOMS -->|"all rooms"| EVICT_ROOMS
    EVICT_ROOMS -->|"snapshot.json for stale rooms"| DAV
    EVICT_ROOMS -->|"remove stale entries"| ROOMS
```

### 2b. Error Handling & Fallbacks

```mermaid
flowchart TD
    START(BootSystem)
    WS(StreamWebSocket)
    AI[AI Provider API]
    DAV[(NextCloud WebDAV)]
    RECONNECT(ReconnectWithBackoff)
    FALLBACK(SendFallbackReply)
    RETRY(RetryWithBackoff)
    SHUTDOWN(GracefulShutdown)
    SIG[OS Signals]

    START -.->|"auth failure error"| RECONNECT
    WS -.->|"ws disconnect error"| RECONNECT
    RECONNECT -.->|"reconnect signal"| WS
    RECONNECT -.->|"max retries exhausted"| SHUTDOWN
    AI -.->|"api error response"| FALLBACK
    DAV -.->|"connection lost error"| RETRY
    RETRY -.->|"retries exhausted"| FALLBACK
    SIG -.->|"shutdown signal"| SHUTDOWN
```

Typing indicator failures are non-critical: if `sender.typing()` returns an error (e.g. WebSocket disconnected), the heartbeat task silently catches it and stops refreshing. The main agent loop is unaffected — it continues processing and sends the reply without typing cleanup.

### 2c. Startup Sequence

```mermaid
flowchart TD
    START["main()"]
    CFG(LoadConfig)
    TOML[(Config File)]
    VALIDATE(ValidateConfig)
    LOGIN(LoginRocketChat)
    CONNECT(ConnectWebSocket)
    DAV[(NextCloud WebDAV)]
    LIST_MEM(ListMemoryArchives)
    SEED(SeedAllRooms)
    LOOP[AgentLoop]
    CFG_STORE[(AppConfig)]

    START -->|"config path"| CFG
    CFG -->|"load toml"| TOML
    TOML -->|"raw config"| VALIDATE
    VALIDATE -->|"appconfig"| CFG_STORE
    CFG_STORE -->|"server credentials"| LOGIN
    LOGIN -->|"auth token"| CONNECT
    CONNECT -->|"connected"| DAV
    CFG_STORE -->|"webdav credentials"| DAV
    DAV -->|"archive list"| LIST_MEM
    LIST_MEM -->|"archived messages"| SEED
    SEED -->|"ready"| LOOP
```

Note: History loading is lazy — each room's archives are restored on first message via `restore_history()`, not eagerly at startup. The `ListMemoryArchives` and `SeedAllRooms` steps shown above are illustrative of what happens per room, not a boot-time batch.

### 2d. Typing Indicator Heartbeat

Level 2 decomposition of `ToggleTyping` and the typing flows during `AgentLoop`. The bot sends an initial `typing=true` signal before the agent loop begins, then a background task refreshes it every 2 seconds while the loop runs. When the loop produces a reply (or errors out), typing is set to `false`.

```mermaid
flowchart TD
    RC[RocketChat DDP]
    MSG[IncomingMessage]
    START_TYPING(SendTypingOn)
    HEARTBEAT(RefreshTyping)
    TIMER((Every 2s))
    PROCESS(AgentLoop)
    STOP_TYPING(SendTypingOff)
    REPLY[BotReply]

    MSG -->|"dispatch"| START_TYPING
    START_TYPING -->|"stream-notify-room/typing true"| RC
    START_TYPING -->|"start loop"| PROCESS
    PROCESS -->|"while processing"| HEARTBEAT
    TIMER -->|"tick"| HEARTBEAT
    HEARTBEAT -->|"stream-notify-room/typing true"| RC
    PROCESS -->|"reply ready or error"| STOP_TYPING
    STOP_TYPING -->|"stream-notify-room/typing false"| RC
    STOP_TYPING -->|"send"| REPLY
```

The heartbeat task is a `tokio::spawn` that runs concurrently with the agent loop, refreshing the typing indicator every 2 seconds. If the WebSocket disconnects, `sender.typing()` returns an error — the heartbeat task breaks its loop and exits silently. The main agent loop is unaffected.

Typing indicator state is intentionally not retried or persisted — it is a transient UI affordance with no durability requirements.

## 3. Data Structures

#### `AgentHarness` (harness.rs:37-44)

| Field            | Type                  | Notes                                      |
| ---------------- | --------------------- | ------------------------------------------ |
| `config`         | `Arc<AppConfig>`      | Immutable configuration shared across subsystems |
| `provider`       | `Box<dyn AiProvider>` | AI provider for chat completions           |
| `memory`         | `MemoryManager`       | Per-room conversation history              |
| `tools`          | `ToolRegistry`        | Registered tool definitions                |
| `webdav`         | `Option<WebDavClient>`| Optional WebDAV handle for persistent storage |
| `max_iterations` | `u32`                 | Max agent loop iterations per message      |

#### `RoomState`

| Field           | Type                | Notes                                      |
| --------------- | ------------------- | ------------------------------------------ |
| `room_id`       | `String`            | RocketChat room UUID (in-memory lookup key, not a path segment) |
| `is_dm`         | `bool`              | True if direct message room                |
| `history`       | `ConversationHistory`| In-memory message buffer for this room     |
| `webdav_dir`    | `String`            | Computed on-the-fly from `room_name`/`room_fname`/`is_dm` via `compute_webdav_dir()`; not a stored field |
| `last_activity` | `u64`               | Unix timestamp of last interaction; checked against `memory_ttl_secs` for eviction |

No `LifecycleSignal` enum exists. The main loop uses inline `tokio::signal::ctrl_c()` for shutdown and a local `retry_count: u32` variable for reconnect backoff.
