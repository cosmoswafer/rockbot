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
  archive (threshold-based daily compress), and snapshot persist (timer-based)
- Downstream: [WebDAV Tool](tools/webdav.md) persists image assets

## 2. Diagram

### 2a. Happy Flow (Main Success Path)

```mermaid
flowchart TD
    RC[RocketChat Server]
    AI[AI Provider API]
    DAV[(NextCloud WebDAV)]
    EXA[Exa Search API]
    TIMER[5-Minute Timer]
    DISPATCH(ReceiveMessage)
    LOOP(AgentLoop)
    ARCHIVE(CompressDaily)
    PERSIST_SNAP(PersistSnapshot)
    PERSIST_ASSETS(PersistAssets)
    CFG[(AppConfig)]
    HISTORY[(ConversationHistory)]
    TOOLS[(ToolRegistry)]
    ROOMS[(RoomStateMap)]

    RC -->|"incoming message"| DISPATCH
    ROOMS -->|"room state"| DISPATCH
    CFG -->|"app config"| DISPATCH
    DISPATCH -->|"incoming message"| LOOP
    CFG -->|"ai config"| LOOP
    HISTORY -->|"conversation history"| LOOP
    TOOLS -->|"tool definitions"| LOOP
    LOOP -->|"chat request"| AI
    AI -->|"completion result"| LOOP
    LOOP -->|"search query"| EXA
    EXA -->|"search results"| LOOP
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
    TIMER -->|"tick"| PERSIST_SNAP
    ROOMS -->|"all rooms with messages"| PERSIST_SNAP
    PERSIST_SNAP -->|"snapshot JSON"| DAV
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

## 3. Data Structures

#### `HarnessState`

| Field    | Type                       | Notes                                       |
| -------- | -------------------------- | ------------------------------------------- |
| `config` | `Arc<AppConfig>`           | Immutable configuration shared across subsystems |
| `rooms`  | `HashMap<String, RoomState>` | Per-room state map (room_id → state)     |
| `client` | `rocketchat::Client`       | RocketChat connection handle                |
| `memory` | `MemoryManager`            | Per-room conversation history               |
| `webdav` | `WebDavClient`             | WebDAV handle for persistent storage        |

#### `RoomState`

| Field        | Type                | Notes                                      |
| ------------ | ------------------- | ------------------------------------------ |
| `room_id`    | `String`            | RocketChat room UUID (in-memory lookup key, not a path segment) |
| `is_dm`      | `bool`              | True if direct message room                |
| `history`    | `ConversationHistory`| In-memory message buffer for this room     |
| `webdav_dir` | `String`            | Type-prefixed WebDAV path key (`r-`/`d-`), computed from `room_name`/`room_fname`/`is_dm` |

#### `LifecycleSignal`

| Variant     | Fields             | Notes                                      |
| ----------- | ------------------ | ------------------------------------------ |
| `Startup`   | —                  | Bot is initializing                        |
| `Running`   | —                  | Main event loop active                     |
| `Shutdown`  | `exit_code: i32`   | Graceful shutdown triggered                |
| `Reconnect` | `attempt: u32`     | WebSocket reconnection in progress         |
