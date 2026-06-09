# Agent Orchestrator

## 1. Purpose

Shows how all subsystems — RocketChat client, AI provider, tools, memory,
WebDAV, config — are wired together to run the agent harness. This is the
top-level process decomposition of RockBot: a single event loop that connects to
RocketChat, routes incoming messages to the agent harness, executes tool calls,
manages per-room memory, and persists everything to WebDAV.

- Upstream: [Configuration Management](config.md) provides `AppConfig`
- Downstream: [Agent Loop](agent-harness.md) receives `IncomingMessage` and
  returns `BotReply`
- Downstream: [RocketChat Connection](rocketchat.md) handles auth, WebSocket
  streaming, and message filtering
- Downstream: [AI Provider](ai-provider.md) handles chat completion requests
- Downstream: [Memory Management](memory.md) manages per-room conversation history
- Downstream: [WebDAV Storage](webdav.md) persists archives and image assets

## 2. Diagram

### 2a. Happy Flow (Main Success Path)

```mermaid
flowchart TD
    RC[RocketChat Server]
    AI[AI Provider API]
    DAV[(NextCloud WebDAV)]
    EXA[Exa Search API]

    DISPATCH(ReceiveMessage)
    LOOP(AgentLoop)
    TOOL_EXEC(ExecuteTools)
    ARCHIVE(ArchiveMemory)
    PERSIST(PersistAssets)

    CFG[(AppConfig)]
    HISTORY[(ConversationHistory)]
    TOOLS[(ToolRegistry)]
    ROOMS[(RoomStateMap)]

    RC -->|"DM / @mention"| DISPATCH
    ROOMS -->|"room state"| DISPATCH
    DISPATCH -->|"IncomingMessage"| LOOP
    HISTORY -->|"conversation history"| LOOP
    TOOLS -->|"tool definitions"| LOOP
    CFG -->|"AI provider config"| LOOP
    LOOP -->|"ChatRequest"| AI
    AI -->|"CompletionResult"| LOOP
    LOOP -->|"tool calls"| TOOL_EXEC
    TOOL_EXEC -->|"search query"| EXA
    EXA -->|"search results"| TOOL_EXEC
    TOOL_EXEC -->|"image asset"| PERSIST
    PERSIST -->|"read/write"| DAV
    TOOL_EXEC -->|"ToolResult"| LOOP
    LOOP -->|"BotReply"| RC
    LOOP -->|"message to archive"| ARCHIVE
    ARCHIVE -->|"summary prompt"| AI
    AI -->|"summary text"| ARCHIVE
    ARCHIVE -->|"archive .md"| PERSIST
    ARCHIVE -->|"pruned history"| HISTORY
    LOOP -->|"updated state"| ROOMS
```

### 2b. Startup Sequence

```mermaid
flowchart TD
    START["main()"]
    CFG(LoadConfig)
    MIGRATE{Legacy JSON?}
    TOML["config.toml"]
    JSON["config.json"]
    VALIDATE(ValidateConfig)
    LOGIN(LoginRocketChat)
    CONNECT(ConnectWebSocket)
    DAV[(NextCloud WebDAV)]
    LIST_MEM(ListMemoryArchives)
    SEED(SeedAllRooms)
    LOOP[AgentLoop]
    CFG_STORE[(AppConfig)]

    START -->|"config path"| CFG
    CFG -->|"load TOML"| TOML
    CFG -->|"check legacy"| MIGRATE
    MIGRATE -->|"found"| JSON
    JSON -->|"migrate"| TOML
    TOML -->|"raw config"| VALIDATE
    VALIDATE -->|"AppConfig"| CFG_STORE
    CFG_STORE -->|"server credentials"| LOGIN
    LOGIN -->|"auth token"| CONNECT
    CONNECT -->|"connected"| DAV
    CFG_STORE -->|"WebDAV credentials"| DAV
    DAV -->|"PROPFIND"| LIST_MEM
    LIST_MEM -->|"archive list"| SEED
    SEED -->|"ready"| LOOP
```

### 2c. Error Handling & Fallbacks

```mermaid
flowchart TD
    START[Boot]
    WS(WebSocket Stream)
    AI[AI Provider API]
    DAV[(NextCloud WebDAV)]

    AUTH_FAIL[Startup: Auth Failed]
    WS_CLOSED[Runtime: WS Disconnect]
    AI_ERROR[Runtime: AI Error]
    DAV_ERROR[Runtime: WebDAV Error]

    RECONNECT(ReconnectWithBackoff)
    FALLBACK(SendFallbackReply)
    RETRY(RetryWithBackoff)
    SHUTDOWN(GracefulShutdown)
    SIG[OS Signals]

    START -->|"auth failure"| AUTH_FAIL
    AUTH_FAIL -->|"exponential backoff"| RECONNECT
    WS -->|"closed / error"| WS_CLOSED
    WS_CLOSED -->|"reconnect"| RECONNECT
    RECONNECT -->|"max retries"| SHUTDOWN
    AI -->|"API error"| AI_ERROR
    AI_ERROR -->|"error message reply"| FALLBACK
    DAV -->|"connection lost"| DAV_ERROR
    DAV_ERROR -->|"retry"| RETRY
    RETRY -->|"max retries"| FALLBACK
    SIG -->|"SIGTERM / SIGINT"| SHUTDOWN
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
| `room_id`    | `String`            | RocketChat room/channel identifier         |
| `is_dm`      | `bool`              | True if direct message room                |
| `history`    | `ConversationHistory`| In-memory message buffer for this room     |
| `webdav_root`| `String`            | `/{root}/{room_id}/` path prefix           |

#### `LifecycleSignal`

| Variant     | Fields             | Notes                                      |
| ----------- | ------------------ | ------------------------------------------ |
| `Startup`   | —                  | Bot is initializing                        |
| `Running`   | —                  | Main event loop active                     |
| `Shutdown`  | `exit_code: i32`   | Graceful shutdown triggered                |
| `Reconnect` | `attempt: u32`     | WebSocket reconnection in progress         |
