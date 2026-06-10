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
- Downstream: [Memory Management](base/memory.md) manages per-room conversation history
  (see base/memory.md for archive and threshold flows)
- Downstream: [WebDAV Directory](base/webdav-directory.md) persists image assets
- Downstream: [WebDAV Memory](base/webdav-memory.md) persists conversation archives

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
    ARCHIVE(ArchiveMemory)
    PERSIST(PersistAssets)
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
    LOOP -->|"image asset"| PERSIST
    ARCHIVE -->|"summary prompt"| AI
    AI -->|"summary text"| ARCHIVE
    ARCHIVE -->|"archive file"| PERSIST
    PERSIST -->|"file data"| DAV
    DAV -->|"file data"| PERSIST
    ARCHIVE -->|"pruned history"| HISTORY
    LOOP -->|"updated room state"| ROOMS
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
